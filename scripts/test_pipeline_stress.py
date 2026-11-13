"""Stress protection must preserve active work and ignore historical failures."""
import json
import tempfile
import time
import unittest
from pipeline.store import Store
from pipeline.stress import observe


class StressGuardTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        self.s=Store(self.tmp.name);self.addCleanup(self.s.close)
        self.start=time.time();self.s.set_setting('slots',100)
        self.s.set_setting('stress_watch_enabled',True);self.s.set_setting('stress_watch_started_at',self.start)
        for i in range(4):
            name=f'task-{i}';self.s.put_task(name,'g','body',{})
            self.s.enqueue('implement',name,'g',{})

    def failures(self,category):
        for i in range(3):
            c=self.s.claim('implement','worker',task=f'task-{i}',concurrency=100,capacity=100)
            self.s.retry(c['id'],c['token'],{'category':category},delay=900)

    def test_provisioning_burst_pauses_and_keeps_active_claim_tokens_and_capacity(self):
        active=self.s.claim('implement','active',task='task-3',concurrency=100,capacity=100)
        self.failures('sandbox_provisioning')
        self.s.put_task('task-new','g','body',{})
        self.s.enqueue('implement','task-new','g',{})
        alert=observe(self.s)
        self.assertIn('provisioning',alert['reason'])
        self.assertTrue(self.s.setting('admission_paused'));self.assertEqual(self.s.setting('slots'),100)
        self.s.heartbeat(active['id'],active['token'])
        self.assertEqual(self.s.db.execute('SELECT state FROM work WHERE id=?',(active['id'],)).fetchone()[0],'claimed')
        self.assertIsNone(self.s.claim('implement','new-worker',concurrency=100,capacity=100))
        self.s.enqueue('cleanup','task-0','g',{'resource':'retained'})
        self.assertIsNotNone(self.s.claim('cleanup','cleanup'))

    def test_old_cohort_and_ordinary_candidate_failures_do_not_pause(self):
        self.failures('sandbox_provisioning')
        self.s.set_setting('stress_watch_started_at',time.time()+1)
        self.assertIsNone(observe(self.s,now=time.time()+2))
        self.s.set_setting('stress_watch_started_at',self.start)
        self.s.db.execute("UPDATE events SET detail=? WHERE kind='retry_scheduled'",(json.dumps({'reason':{'category':'code'}}),))
        self.assertIsNone(observe(self.s))
        self.assertFalse(self.s.setting('admission_paused',False))

    def test_one_oom_is_local_but_three_six_gib_ooms_pause_experiment(self):
        for i in range(3):
            self.s.event('compute_oom',{'observed_gib':6},f'task-{i}')
            if i<2:self.assertIsNone(observe(self.s))
        self.assertIn('6 GiB',observe(self.s)['reason'])
