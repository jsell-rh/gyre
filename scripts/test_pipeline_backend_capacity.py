import datetime
import json
import subprocess
import tempfile
import time
import unittest
from unittest.mock import patch

from pipeline.backend_capacity import classify
from pipeline.execution import Execution
from pipeline.store import Store


class BackendCapacityTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        self.s=Store(self.tmp.name);self.addCleanup(self.s.close)
        self.s.put_task('task-001','g','body',{})
        self.work=self.s.enqueue('verify','task-001','g',{'base':'old'})
        self.claim=self.s.claim('verify','worker');self.execution=Execution(self.s,self.claim)
        self.s.reserve('owned',self.work,self.claim['token'],'sandbox',100)
        self.reason={'category':'sandbox_provisioning','message':'create failed'}

    def event(self,reason='FailedScheduling',offset=0,namespace='scope',message='Insufficient memory'):
        return {'involvedObject':{'kind':'Pod','name':'owned','namespace':namespace},'reason':reason,'message':message,
                'lastTimestamp':datetime.datetime.fromtimestamp(time.time()+offset,datetime.timezone.utc).isoformat()}

    def classify(self,events):
        self.s.set_setting('runtime_kube_context','context');self.s.set_setting('runtime_namespace','scope')
        with patch('pipeline.backend_capacity.subprocess.run',return_value=subprocess.CompletedProcess([],0,json.dumps({'items':events}),'')) as call:
            result=classify(self.execution,self.reason)
        self.assertIn('involvedObject.name=owned',call.call_args.args[0])
        self.assertIn('scope',call.call_args.args[0]);self.assertEqual(call.call_args.kwargs['timeout'],8)
        return result

    def test_only_owned_current_unschedulable_capacity_is_reclassified(self):
        result=self.classify([self.event()])
        self.assertEqual(result['category'],'backend_capacity')
        self.assertEqual(result['capacity']['sandbox'],'owned')
        self.assertEqual(result['capacity']['namespace'],'scope')

    def test_stale_foreign_scheduled_or_other_failures_remain_structural(self):
        for events in ([self.event(offset=-120)],[self.event(namespace='foreign')],
                       [self.event(),self.event(reason='Scheduled',offset=1)],
                       [self.event(message='unbound immediate PersistentVolumeClaims')]):
            with self.subTest(events=events):self.assertEqual(self.classify(events),self.reason)

    def test_backoff_survives_main_advance_and_success_clears_it(self):
        self.s.resource_state('owned','absent')
        self.s.retry(self.work,self.claim['token'],{'category':'backend_capacity'})
        key=self.s.capacity_retry_key(self.claim);first=self.s.setting(key)
        self.assertAlmostEqual(first['retry_at']-time.time(),60,delta=1)
        newer=self.s.enqueue('verify','task-001','g',{'base':'advanced'})
        self.assertIsNone(self.s.claim('verify','cannot-bypass',ident=newer))
        with patch('pipeline.store.time.time',return_value=first['retry_at']+1):
            c=self.s.claim('verify','after-backoff',ident=newer)
        self.assertIsNotNone(c)
        self.s.retry(newer,c['token'],{'category':'backend_capacity'})
        second=self.s.setting(key);self.assertEqual(second['failures'],2)
        self.assertAlmostEqual(second['retry_at']-time.time(),120,delta=1)
        with patch('pipeline.store.time.time',return_value=second['retry_at']+1):
            c=self.s.claim('verify','again',ident=newer)
        self.s.finish(newer,c['token'],{})
        self.assertIsNone(self.s.setting(key))
