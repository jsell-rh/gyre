import tempfile
import time
import unittest
from pipeline.store import Store
from pipeline.cockpit import snapshot


class ProgressTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        self.s=Store(self.tmp.name);self.addCleanup(self.s.close)

    def test_dependency_blocker_names_the_task_and_stalled_delivery_is_not_healthy(self):
        self.s.put_task('task-001','g','body',{'dependencies':['task-002']})
        self.s.put_task('task-002','g','body',{})
        self.s.set_setting(self.s.runtime_hold_key('task-002','review','g'),{'category':'sandbox_stopped','message':'inspect terminal sandbox'})
        self.s.event('completed',{'merged':'sha'})
        self.s.db.execute('UPDATE events SET at=?',(time.time()-7200,))
        view=snapshot(self.s)
        self.assertIn('task-002',view['tasks'][0]['condition'])
        self.assertEqual(view['tasks'][0]['blockers'][0]['code'],'dependencies')
        self.assertEqual(view['health']['delivery']['condition'],'Stalled')
        self.assertEqual(view['health']['delivery']['blockers']['sandbox_stopped'],1)

    def test_satisfied_task_hides_historical_failures(self):
        self.s.put_task('task-001','g','body',{'delivered':{'generation':'g','sha':'sha'}})
        work=self.s.enqueue('assess','task-001','g',{});c=self.s.claim('assess','old')
        self.s.retry(work,c['token'],{'category':'assessment','message':'stale evidence'},attention=True)
        t=snapshot(self.s)['tasks'][0]
        self.assertEqual(t['blockers'],[])
        self.assertIsNone(t['condition'])
