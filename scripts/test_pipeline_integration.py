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

    def test_process_race_admits_one_integration_without_capping_implementation(self):
        code='''import sys,json
from pipeline.store import Store
s=Store(sys.argv[1]);c=s.claim('verify',sys.argv[2],concurrency=8,capacity=8)
print(json.dumps(c and c['id']))'''
        children=[subprocess.Popen([sys.executable,'-c',code,self.temp.name,str(i)],cwd=Path(__file__).parent,
                   stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True) for i in range(8)]
        admitted=[]
        for child in children:
            out,err=child.communicate(timeout=15);self.assertEqual(child.returncode,0,err)
            admitted.append(json.loads(out))
        self.assertEqual(sum(bool(x) for x in admitted),1)
        self.assertEqual(self.store.compute_used(),1)
        self.store.put_task('task-100','g','body',{})
        self.store.enqueue('implement','task-100','g',{})
        self.assertIsNotNone(self.store.claim('implement','parallel-coder',capacity=8))

    def test_lane_yields_on_infrastructure_retry_then_reacquires_safely(self):
        first=self.store.claim('verify','verifier',concurrency=8)
        entered=self.store.integration_lane()['entered']
        self.store.retry(first['id'],first['token'],{'category':'infrastructure'},delay=0)
        other=Store(self.temp.name)
        try:
            self.assertIsNone(other.integration_lane())
            new=other.enqueue('verify',first['task'],'g',{'candidate':'candidate-1','base':'new-main'})
            self.assertIsNotNone(other.claim('verify','new-base',ident=new,concurrency=8))
            self.assertGreaterEqual(other.integration_lane()['entered'],entered)
            self.assertIsNone(other.claim('verify','other-candidate',task='task-002',concurrency=8))
            claim=dict(other.db.execute('SELECT * FROM work WHERE id=?',(new,)).fetchone())
            other.finish(new,claim['token'],{'passed':True})
            pub=other.enqueue('publish',first['task'],'g',{'candidate':'candidate-1'})
            publisher=other.claim('publish','publisher',ident=pub)
            other.finish(pub,publisher['token'],{'merged':'actual-upstream-sha'},{'delivered':{'sha':'actual-upstream-sha'}})
            self.assertIsNone(other.integration_lane())
            self.assertIsNotNone(other.claim('verify','next-candidate',task='task-002',concurrency=8))
        finally:other.close()

    def test_external_ci_wait_yields_lane_until_observer_sees_ready(self):
        first=self.store.claim('verify','verifier')
        data=self.store.task(first['task'])['data']
        data.update(published_head='published',ci={'head':'published','status':'pending','waiting_since':time.time()-901})
        self.store.finish(first['id'],first['token'],{},data)
        self.assertIsNone(self.store.integration_lane())
        publish=self.store.enqueue('publish',first['task'],'g',{'candidate':'candidate-1'})
        self.assertIsNone(self.store.claim('publish','still-waiting',ident=publish))
        next_claim=self.store.claim('verify','next-verifier')
        self.assertNotEqual(next_claim['task'],first['task'])
        # The observer runs outside the lane. Ready CI can rejoin only after
        # the next integration finishes; all ordinary publication gates remain.
        data['ci']={'head':'published','status':'passed','checks':'green'}
        self.store.put_task(first['task'],'g','body',data)
        self.assertIsNone(self.store.claim('publish','ready-but-lane-busy',ident=publish))
        self.store.finish(next_claim['id'],next_claim['token'],{}, {'delivered':{'sha':'next-sha'}})
        self.assertIsNotNone(self.store.claim('publish','rejoin',ident=publish))

    def test_green_behind_pr_rejoins_integration_but_real_external_waits_stay_parked(self):
        from pipeline.catalog import discover
        from pipeline.eligibility import eligible
        data={'candidate':'candidate-1','candidate_base':'old-main','dependencies':[], 'triaged':'g',
              'review':{'candidate':'candidate-1','approved':True},
              'published_head':'published','pr':'https://github.com/example/repo/pull/1',
              'published_verified':{'candidate':'candidate-1','head':'published','base':'old-main'},
              'ci':{'head':'published','checks':'green','status':'policy_pending',
                    'merge_state':'BEHIND','waiting_since':time.time()-3600}}
        self.store.put_task('task-001','g','body',data)
        self.assertFalse(self.store.integration_waiting(data))
        queued=discover(self.store,'verify','current-main')
        refresh=next(dict(r) for r in self.store.db.execute('SELECT * FROM work WHERE id IN (%s)' % ','.join('?' for _ in queued),queued) if r['task']=='task-001')
        claim=self.store.claim('verify','base-refresher',ident=refresh['id'],eligible=lambda w:eligible(self.store,w))
        self.assertIsNotNone(claim)
        self.assertEqual(claim['input']['base'],'current-main')
        self.assertEqual(self.store.task('task-001')['data']['review'],data['review'])
        self.assertEqual(self.store.integration_lane()['task'],'task-001')
        for merge_state,checks in [('BLOCKED','green'),('UNKNOWN','green'),('BEHIND',None)]:
            with self.subTest(merge_state=merge_state,checks=checks):
                data['ci'].update(merge_state=merge_state,checks=checks)
                self.assertTrue(self.store.integration_waiting(data))

    def test_closed_pr_releases_lane_without_reopening_it_or_blocking_other_candidates(self):
        first=self.store.claim('verify','verifier')
        data=self.store.task(first['task'])['data']
        data.update(published_head='published',ci={'head':'published','status':'closed'})
        self.store.finish(first['id'],first['token'],{},data)
        self.assertIsNone(self.store.integration_lane())
        next_claim=self.store.claim('verify','next-verifier')
        self.assertNotEqual(next_claim['task'],first['task'])


if __name__=='__main__':unittest.main()
