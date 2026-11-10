"""Namespace memory policy is a configuration gate, not a provisioning retry."""
import json
import subprocess
import tempfile
import time
import unittest
from types import SimpleNamespace
from unittest.mock import patch

from pipeline.catalog import helper
from pipeline.compute import check_admission, profile
from pipeline.execution import Execution, Attention, Retry, Wait
from pipeline.store import Store


class AdmissionTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        self.store=Store(self.temp.name);self.addCleanup(self.store.close)
        self.store.set_setting('runtime_kube_context','gyre-context')
        self.store.set_setting('runtime_namespace','gyre-namespace')
        self.store.put_task('task-001','g','spec',{})
        self.work=self.store.enqueue('implement','task-001','g',{})
        self.claim=self.store.claim('implement','worker',capacity=10)
        self.execution=Execution(self.store,self.claim)

    def policy(self,*maxima):
        return SimpleNamespace(stdout=json.dumps({'items':[{'metadata':{'name':f'limit-{i}'},
            'spec':{'limits':[{'type':'Container','max':{'memory':maximum}}]}} for i,maximum in enumerate(maxima)]}))

    def test_default_worker_passes_and_escalation_passes_after_policy_change(self):
        with patch('pipeline.compute.subprocess.run',return_value=self.policy('4Gi')) as query:
            check_admission(self.execution)
        args=query.call_args.args[0]
        self.assertIn('gyre-context',args);self.assertIn('gyre-namespace',args)
        self.assertEqual(profile(self.store,'task-001','implement')['memory'],'4Gi')
        self.store.set_setting('compute_memory:task-001:implement',8)
        with patch('pipeline.compute.subprocess.run',return_value=self.policy('8Gi')):
            check_admission(self.execution)
        self.assertEqual(profile(self.store,'other','implement')['memory'],'4Gi')

    def test_real_handler_path_waits_for_policy_without_creating_or_holding_compute(self):
        self.store.set_setting('compute_memory:task-001:implement',8)
        cli=helper('dev-pipeline')
        with patch('pipeline.compute.subprocess.run',return_value=self.policy('4Gi')),patch.dict(cli.HANDLERS,{'implement':lambda *a:self.fail('denied work must not execute')}):
            cli.run_claim(self.store,self.claim)
        row=self.store.db.execute('SELECT state,result FROM work WHERE id=?',(self.work,)).fetchone()
        self.assertEqual(row['state'],'retry')
        reason=json.loads(row['result'])
        self.assertEqual(reason['category'],'sandbox_admission')
        self.assertEqual(reason['diagnostic']['requested'],'8Gi')
        self.assertEqual(reason['diagnostic']['maximum'],'4Gi')
        self.assertIn('Correct the memory policy',reason['message'])
        self.assertEqual(self.store.compute_used(),0)
        self.assertEqual(self.store.db.execute("SELECT count(*) FROM resources WHERE kind='sandbox'").fetchone()[0],0)
        self.assertFalse(self.store.runtime_held(self.claim))
        updated=self.store.db.execute('SELECT failures,retry_at FROM work WHERE id=?',(self.work,)).fetchone()
        self.assertEqual(updated['failures'],0)
        self.assertGreater(updated['retry_at'],time.time()+290)
        self.store.db.execute('UPDATE work SET retry_at=0 WHERE id=?',(self.work,))
        second=self.store.claim('implement','resumed',ident=self.work,capacity=10)
        with patch('pipeline.compute.subprocess.run',return_value=self.policy('8Gi')),patch.dict(cli.HANDLERS,{'implement':lambda *a: ({},{},[])}),patch.object(cli,'eligible',return_value=True):
            cli.run_claim(self.store,second)
        self.assertEqual(self.store.db.execute('SELECT state FROM work WHERE id=?',(self.work,)).fetchone()[0],'succeeded')

    def test_denied_verifier_releases_integration_lane_for_other_tasks(self):
        self.store.finish(self.work,self.claim['token'],{},{'candidate':'code'})
        self.store.resource_state(self.store.admission_name(self.work,self.claim['token']),'absent')
        self.store.set_setting('compute_memory:task-001:verify',8)
        work=self.store.enqueue('verify','task-001','g',{'candidate':'code','base':'main'})
        claim=self.store.claim('verify','verifier',ident=work,capacity=10)
        self.assertIsNotNone(self.store.integration_lane())
        cli=helper('dev-pipeline')
        with patch('pipeline.compute.subprocess.run',return_value=self.policy('4Gi')),patch.dict(cli.HANDLERS,{'verify':lambda *a:self.fail('denied verifier must not run')}):
            cli.run_claim(self.store,claim)
        self.assertIsNone(self.store.integration_lane())
        self.assertEqual(self.store.compute_used(),0)

    def test_all_container_limits_apply_and_equivalent_quantities_compare_correctly(self):
        self.store.set_setting('compute_memory:task-001:implement',8)
        for maximum in ('8192Mi','8589934592','8.589934592G','8589934592000m'):
            with self.subTest(maximum=maximum),patch('pipeline.compute.subprocess.run',return_value=self.policy(maximum)):
                check_admission(self.execution)
        with patch('pipeline.compute.subprocess.run',return_value=self.policy('16Gi','4096Mi')):
            with self.assertRaises(Wait):check_admission(self.execution)

    def test_unknown_policy_retries_without_launching_or_permanent_hold(self):
        for response in ('{}','not json','{"items":null}','{"items":[],"metadata":{"continue":"more"}}'):
            with self.subTest(response=response),patch('pipeline.compute.subprocess.run',return_value=SimpleNamespace(stdout=response)):
                with self.assertRaises(Retry) as error:check_admission(self.execution)
                self.assertNotIsInstance(error.exception,Attention)
        for maximum in ('invalid','NaN','-1Gi'):
            with patch('pipeline.compute.subprocess.run',return_value=self.policy(maximum)):
                with self.assertRaises(Retry) as error:check_admission(self.execution)
                self.assertNotIsInstance(error.exception,Attention)
        with patch('pipeline.compute.subprocess.run',side_effect=subprocess.TimeoutExpired('kubectl',10)):
            with self.assertRaises(Retry):check_admission(self.execution)

    def test_no_maximum_or_non_kubernetes_backend_keeps_existing_behavior(self):
        with patch('pipeline.compute.subprocess.run',return_value=self.policy()):check_admission(self.execution)
        self.store.set_setting('runtime_kube_context',None)
        with patch('pipeline.compute.subprocess.run',side_effect=AssertionError('no Kubernetes query')):
            check_admission(self.execution)


if __name__=='__main__':unittest.main()
