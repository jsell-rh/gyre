"""Exercise real queue order, live labels, and atomic reserved capacity."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

from pipeline.catalog import discover,helper
from pipeline.ci import observe
from pipeline.eligibility import eligible
from pipeline.execution import Execution,Wait
from pipeline.priority import define,info,set_task,task_priority,PriorityError
from pipeline.store import Store


class PriorityFixture:
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        self.store=Store(self.temp.name);self.addCleanup(self.store.close)

    def task(self,name,stage='implement',**data):
        self.store.put_task(name,'g','requirements',{'dependencies':[],'triaged':'g',**data})
        return self.store.enqueue(stage,name,'g',{})


class PriorityTests(PriorityFixture,unittest.TestCase):

    def test_repair_is_selected_before_ordinary_review_and_fifo_within_class(self):
        review=self.task('task-001','review')
        self.store.db.execute('UPDATE work SET priority=999999 WHERE id=?',(review,))
        repair=self.task('task-002',repair={'category':'ci'})
        other=self.task('task-003',repair={'category':'verification'})
        self.assertEqual([r['id'] for r in self.store.ordered_ready()],[repair,other,review])
        self.assertEqual(self.store.claim('implement','worker')['id'],repair)

    def test_aging_eventually_serves_low_priority_without_outranking_verification(self):
        old=self.task('task-001')
        urgent=self.task('task-002');set_task(self.store,'task-002','urgent')
        verify=self.task('task-003','verify')
        self.store.db.execute('UPDATE work SET ready_since=? WHERE id=?',(time.time()-301*60,old))
        since=self.store.db.execute('SELECT ready_since FROM work WHERE id=?',(old,)).fetchone()[0]
        self.store.enqueue('implement','task-001','g',{})
        self.assertEqual(self.store.db.execute('SELECT ready_since FROM work WHERE id=?',(old,)).fetchone()[0],since)
        self.assertEqual([r['id'] for r in self.store.ordered_ready()],[verify,old,urgent])

    def test_operator_and_github_labels_reprioritize_without_invalidating_claim_or_review(self):
        work=self.task('task-001',review={'candidate':'code','approved':True},priority_labels=['priority/high'])
        claim=self.store.claim('implement','running')
        self.assertEqual(task_priority(self.store,self.store.task('task-001'))['class'],'high')
        set_task(self.store,'task-001','urgent')
        self.assertEqual(task_priority(self.store,self.store.task('task-001'))['source'],'operator')
        self.store._current(work,claim['token'])
        self.assertTrue(self.store.task('task-001')['data']['review']['approved'])
        self.assertEqual(self.store.task('task-001')['generation'],'g')
        set_task(self.store,'task-001',None)
        self.assertEqual(task_priority(self.store,self.store.task('task-001'))['class'],'high')

    def test_invalid_classes_are_not_silently_admitted_and_do_not_block_cleanup(self):
        self.task('task-001',priority_labels=['priority/unknown'])
        self.assertIsNone(self.store.claim('implement','worker',capacity=2))
        with self.assertRaises(PriorityError):set_task(self.store,'task-001','missing')
        define(self.store,'hotfix',900);set_task(self.store,'task-001','hotfix')
        self.assertIsNotNone(self.store.claim('implement','worker',capacity=2))
        with self.assertRaises(PriorityError):define(self.store,'system/verification',1001)

    def test_retry_backoff_does_not_accumulate_age_or_bypass_backoff(self):
        work=self.task('task-001')
        self.store.db.execute('UPDATE work SET ready_since=? WHERE id=?',(time.time()-99999,work))
        claim=self.store.claim('implement','worker')
        self.store.retry(work,claim['token'],{'category':'infrastructure'},delay=600)
        row=dict(self.store.db.execute('SELECT * FROM work WHERE id=?',(work,)).fetchone())
        self.assertEqual(info(self.store,row)['age_seconds'],0)
        self.assertEqual(self.store.ordered_ready(),[])

    def test_partial_implementation_retains_priority_of_unresolved_ci_finding(self):
        work=self.task('task-001')
        claim=self.store.claim('implement','worker')
        self.store.finish(work,claim['token'],{}, {'repair':{'category':'implementation'}},
                          [{'category':'ci','source':'failed-head'}])
        self.assertEqual(task_priority(self.store,self.store.task('task-001'))['class'],'repair')

    def test_failed_pr_label_is_refreshed_without_repeating_ci_or_mutating_repair(self):
        self.task('task-001',pr='https://github.com/example/repo/pull/1',repair={'category':'ci'})
        work=discover(self.store,'observe','main')[0]
        observer=Execution(self.store,self.store.claim('observe','watcher',ident=work))
        result=subprocess.CompletedProcess([],0,json.dumps({'labels':[{'name':'priority/urgent'}]}),'')
        with patch.object(observer,'command',return_value=result) as command,self.assertRaises(Wait):
            observe(observer,self.store.task('task-001'))
        self.assertEqual(command.call_count,1)
        data=self.store.task('task-001')['data']
        self.assertEqual(data['repair'],{'category':'ci'})
        self.assertEqual(task_priority(self.store,self.store.task('task-001'))['class'],'urgent')

    def test_priority_change_rejects_a_stale_dispatch_request_without_recreating_work(self):
        adapter=helper('dispatch-adapter')
        work=self.task('task-001')
        request=adapter.job(self.store,dict(self.store.db.execute('SELECT * FROM work WHERE id=?',(work,)).fetchone()))
        self.assertTrue(adapter.authorize(self.store,request))
        set_task(self.store,'task-001','urgent')
        self.assertFalse(adapter.authorize(self.store,request))
        fresh=adapter.job(self.store,dict(self.store.db.execute('SELECT * FROM work WHERE id=?',(work,)).fetchone()))
        self.assertTrue(adapter.authorize(self.store,fresh))
        self.assertEqual(self.store.db.execute('SELECT count(*) FROM work').fetchone()[0],1)


class VerificationReservationTests(PriorityFixture,unittest.TestCase):
    def test_separate_processes_cannot_borrow_verification_reserve(self):
        for i in range(1,9):self.task(f'task-{i:03d}')
        code='''import json,sys
from pipeline.store import Store
s=Store(sys.argv[1]);c=s.claim('implement',sys.argv[2],concurrency=8,capacity=4)
print(json.dumps(c and c['id']))'''
        children=[subprocess.Popen([sys.executable,'-c',code,self.temp.name,str(i)],cwd=Path(__file__).parent,
                   stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True) for i in range(8)]
        admitted=[]
        for child in children:
            out,err=child.communicate(timeout=15);self.assertEqual(child.returncode,0,err);admitted.append(json.loads(out))
        self.assertEqual(sum(bool(x) for x in admitted),3)
        verify=self.task('task-100','verify')
        claim=self.store.claim('verify','integration',ident=verify,capacity=4)
        self.assertIsNotNone(claim)
        self.assertEqual(self.store.compute_used(),4)

    def test_pending_deletion_remains_charged_and_controls_run_at_full_capacity(self):
        work=self.task('task-001');claim=self.store.claim('implement','model',capacity=2)
        self.store.reserve('model',work,claim['token'],'sandbox',2)
        self.task('task-002')
        self.assertIsNone(self.store.claim('implement','blocked',capacity=2))
        verify=self.task('task-003','verify');integration=self.store.claim('verify','verify',ident=verify,capacity=2)
        self.assertIsNotNone(integration)
        self.store.finish(work,claim['token'],{})
        self.store.resource_state('model','deleting')
        self.assertIsNone(self.store.claim('implement','still blocked',capacity=2))
        cleanup=self.store.enqueue('cleanup','task-001','g',{'resource':'model'})
        self.assertIsNotNone(self.store.claim('cleanup','reaper',ident=cleanup,capacity=2))
        self.store.resource_state('model','absent')
        self.assertIsNotNone(self.store.claim('implement','replacement',capacity=2))

    def test_single_slot_can_bootstrap_and_zero_slots_drain(self):
        work=self.task('task-001')
        self.assertEqual(self.store.verification_reserve(1),0)
        self.assertIsNone(self.store.claim('implement','drain',capacity=0))
        self.assertIsNotNone(self.store.claim('implement','one worker',capacity=1))


if __name__=='__main__':unittest.main()
