import copy
import json
import tempfile
import unittest
from unittest.mock import patch
from pipeline.store import Store
from pipeline.execution import Execution
from pipeline.stages import publish, cleanup
from pipeline.publication_recovery import superseded, PublicationMismatch


class PublicationRecoveryTests(unittest.TestCase):
    def setUp(self):
        self.task = {'name':'task-001', 'generation':'new', 'data':{
            'pr':'https://github.com/test/repo/pull/1',
            'previous_delivery':{'pr':'https://github.com/test/repo/pull/1', 'sha':'merged',
                                 'candidate':'old-head', 'generation':'old'}}}
        self.observed = {'state':'MERGED','headRefOid':'old-head','mergeCommit':{'oid':'merged'}}
        self.verified = {'head':'new-head'}

    def test_old_delivery_detaches_without_claiming_new_source_shipped(self):
        updates = superseded(self.task, self.observed, self.verified)
        self.assertIsNone(updates['pr'])
        self.assertIsNone(updates['published_verified'])
        self.assertEqual(updates['publication_epoch'], 'merged')
        self.assertNotIn('delivered', updates)
        self.assertEqual(updates['previous_delivery']['sha'], 'merged')

    def test_unknown_changed_head_and_same_generation_cannot_be_detached(self):
        for field,value in [('sha','other-merge'),('candidate','other-head'),('generation','new'),('pr','other-pr')]:
            task = copy.deepcopy(self.task)
            task['data']['previous_delivery'][field] = value
            self.assertIsNone(superseded(task,self.observed,self.verified),field)

    def test_mismatch_becomes_one_durable_hold_instead_of_retrying_forever(self):
        import subprocess
        with tempfile.TemporaryDirectory() as directory:
            s=Store(directory);self.addCleanup(s.close)
            s.put_task('task-001','new','body',self.task['data']|{'previous_delivery':None})
            ident=s.enqueue('publish','task-001','new',{'verified':self.verified})
            claim=s.claim('publish','worker');e=Execution(s,claim)
            with patch.object(e,'command',return_value=subprocess.CompletedProcess([],0,stdout=json.dumps(self.observed))):
                with self.assertRaises(PublicationMismatch) as raised:publish(e,s.task('task-001'))
            s.retry(ident,claim['token'],{'category':raised.exception.category,'message':str(raised.exception)},attention=True)
            another=s.enqueue('publish','task-001','new',{'verified':self.verified,'base':'advanced'})
            self.assertIsNone(s.claim('publish','again',ident=another))

    def test_provider_waits_for_its_sandbox_without_attempting_deletion(self):
        with tempfile.TemporaryDirectory() as directory:
            from pipeline.execution import Wait
            s=Store(directory);self.addCleanup(s.close);s.put_task('task-001','g','body',{})
            ident=s.enqueue('implement','task-001','g',{});claim=s.claim('implement','worker')
            s.reserve('sandbox',ident,claim['token'],'sandbox',10,{})
            s.reserve('provider',ident,claim['token'],'provider',10,{})
            s.finish(ident,claim['token'],{})
            work=s.enqueue('cleanup','task-001','g',{'resource':'provider'});c=s.claim('cleanup','cleaner');e=Execution(s,c)
            with patch.object(e,'login'),patch.object(e,'os') as cli:
                with self.assertRaises(Wait) as raised:cleanup(e,s.task('task-001'))
            cli.assert_not_called();self.assertEqual(raised.exception.retry_delay,900)

    def test_detachment_creates_new_work_identity_even_on_unchanged_base(self):
        from pipeline.catalog import discover
        from pipeline.eligibility import eligible
        with tempfile.TemporaryDirectory() as directory:
            s=Store(directory);self.addCleanup(s.close)
            data=self.task['data']|{'candidate':'new-source','candidate_base':'base','dependencies':[],
                'triaged':'new','verified':self.verified|{'candidate':'new-source','passed':True},
                'review':{'candidate':'new-source','approved':True}}
            s.put_task('task-001','new','body',data)
            first=discover(s,'publish','base')[0];claim=s.claim('publish','first')
            updates=superseded(s.task('task-001'),self.observed,data['verified'])
            s.finish(first,claim['token'],{},updates)
            second=discover(s,'publish','base')[0]
            self.assertNotEqual(first,second)
            self.assertIsNotNone(s.claim('publish','new-pr',ident=second,eligible=lambda w:eligible(s,w)))
