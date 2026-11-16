"""Real process races prove compute concurrency and the separate merge permit."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from pipeline.store import Store


class IntegrationLaneTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        self.store=Store(self.temp.name);self.addCleanup(self.store.close)
        for i in range(1,9):
            name=f'task-{i:03d}'
            self.store.put_task(name,'g','body',{'candidate':f'candidate-{i}'})
            self.store.enqueue('verify',name,'g',{'candidate':f'candidate-{i}','base':'main'})

    def test_process_race_runs_independent_verifiers_up_to_shared_capacity(self):
        code='''import sys,json
from pipeline.store import Store
s=Store(sys.argv[1]);c=s.claim('verify',sys.argv[2],concurrency=8,capacity=3)
print(json.dumps(c and c['id']))'''
        children=[subprocess.Popen([sys.executable,'-c',code,self.temp.name,str(i)],cwd=Path(__file__).parent,
                   stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True) for i in range(8)]
        admitted=[]
        for child in children:
            out,err=child.communicate(timeout=15);self.assertEqual(child.returncode,0,err);admitted.append(json.loads(out))
        self.assertEqual(sum(bool(x) for x in admitted),3)
        self.assertEqual(self.store.compute_used(),3)
        self.assertIsNone(self.store.integration_lane())

    def test_ci_wait_does_not_hold_compute_or_prevent_other_publications(self):
        first=self.store.claim('verify','verifier');data=self.store.task(first['task'])['data']
        data.update(published_head='published',ci={'head':'published','status':'pending','waiting_since':time.time()-901})
        self.store.finish(first['id'],first['token'],{},data)
        pub=self.store.enqueue('publish',first['task'],'g',{'candidate':'candidate-1'})
        self.assertIsNotNone(self.store.claim('publish','waiting-publisher',ident=pub))
        second=self.store.claim('verify','parallel-verifier')
        self.assertNotEqual(first['task'],second['task'])
        self.assertIsNone(self.store.integration_lane())
        self.assertIsNone(self.store.claim('verify','duplicate-task',task=second['task'],concurrency=8))

    def test_only_actual_merge_requests_serialize_and_await_confirmation(self):
        first=self.store.claim('verify','verifier');self.store.finish(first['id'],first['token'],{})
        pub=self.store.enqueue('publish',first['task'],'g',{'candidate':'candidate-1'})
        owner=self.store.claim('publish','merger',ident=pub,concurrency=8)
        self.assertTrue(self.store.reserve('merge-one',pub,owner['token'],'merge',1,{'head':'head','base':'base','url':'url'}))
        self.assertEqual(self.store.integration_lane()['task'],first['task'])
        other=self.store.claim('verify','unblocked-verifier',concurrency=8)
        self.assertIsNotNone(other);self.store.finish(other['id'],other['token'],{})
        second=self.store.enqueue('publish',other['task'],'g',{'candidate':'candidate-2'})
        publisher=self.store.claim('publish','prepare-another-pr',ident=second,concurrency=8)
        self.assertIsNotNone(publisher)
        self.assertFalse(self.store.reserve('merge-two',second,publisher['token'],'merge',1,{}))
        self.store.retry(pub,owner['token'],{'category':'observation'},count_failure=False)
        self.assertIsNotNone(self.store.integration_lane(),'an uncertain merge retains its actual permit')
        self.store.resource_state('merge-one','absent')
        self.assertTrue(self.store.reserve('merge-two',second,publisher['token'],'merge',1,{}))

    def test_source_safety_hold_and_closed_pr_still_block_their_own_task(self):
        first=self.store.claim('verify','verifier');self.store.retry(first['id'],first['token'],{'category':'verification_timeout'},attention=True)
        newer=self.store.enqueue('verify',first['task'],'g',{'candidate':'candidate-1','base':'new-main'})
        self.assertIsNone(self.store.claim('verify','cannot-bypass',ident=newer,concurrency=8))
        second=self.store.claim('verify','other-task',concurrency=8);self.assertIsNotNone(second)
        data=self.store.task(second['task'])['data']|{'published_head':'published','ci':{'head':'published','status':'closed'}}
        self.store.finish(second['id'],second['token'],{},data)
        work=self.store.enqueue('publish',second['task'],'g',{})
        self.assertIsNone(self.store.claim('publish','cannot-reopen',ident=work))

    def test_old_broad_lane_record_cannot_reinstate_global_verification_lock(self):
        self.store.set_setting('integration_lane',{'task':'task-001','candidate':'candidate-1','generation':'g','entered':0})
        self.assertIsNotNone(self.store.claim('verify','independent',task='task-002'))
        self.assertIsNone(self.store.integration_lane())

if __name__=='__main__':unittest.main()
