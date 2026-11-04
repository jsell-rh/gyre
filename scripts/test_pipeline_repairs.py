import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from pipeline.catalog import discover
from pipeline.eligibility import eligible
from pipeline.execution import Execution, Retry
from pipeline.repairs import probe, recheck
from pipeline.store import Store


class RepairReconciliationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.store = Store(self.temp.name)
        self.addCleanup(self.store.close)
        self.command = ['bash','scripts/check-task-commit-attribution.sh']
        for name in ('task-001','task-002'):
            self.store.put_task(name,'g','body',{'baseline_key':'old-env-'+name,
                    'baseline_probe':self.command,'dependencies':[],'triaged':'g'})

    def run_probe(self, passed):
        self.store.set_setting('observed_main','main')
        work = discover(self.store,'triage','main')[0]
        claim = self.store.claim('triage','fixture',ident=work,eligible=lambda w:eligible(self.store,w))
        execution = Execution(self.store,claim)
        receipt={'candidate':'main','base':'main','head':'main','tree':'tree','sandbox':'remote',
                 'passed':passed,'checks':[{'command':self.command,'exit_code':0 if passed else 1}]}
        with patch('pipeline.stages.checkout', return_value=Path(self.temp.name)), \
             patch.object(execution,'command',return_value=subprocess.CompletedProcess([],0,'tree\n','')), \
             patch('pipeline.verification.run',return_value=receipt) as remote:
            result,updates,_=recheck(execution,self.store.task(claim['task']))
        self.store.finish(work,claim['token'],result,updates)
        return claim['task'], remote.call_count

    def test_passing_remote_probe_resolves_two_repairs_with_one_remote_call_and_no_merge(self):
        first,calls=self.run_probe(True)
        self.assertEqual(calls,1)
        # Even a full/drained pool needs no extra admission to apply a shared
        # authoritative receipt to the second alias.
        self.store.set_setting('slots',0)
        self.assertEqual(discover(self.store,'triage','main'),[])
        second=next(t['name'] for t in self.store.tasks() if t['name']!=first)
        for name in (first,second):
            data=self.store.task(name)['data']
            self.assertEqual(data['resolved']['base'],'main')
            self.assertNotIn('delivered',data)
        self.assertEqual(discover(self.store,'implement','main'),[])
        self.assertEqual(discover(self.store,'publish','main'),[])
        self.store.put_task('task-003','g','body',{'dependencies':[first,second],'triaged':'g'})
        self.assertEqual(len(discover(self.store,'implement','main')),1)

    def test_failure_keeps_repair_workable_and_new_main_requires_new_probe(self):
        name,_=self.run_probe(False)
        self.assertNotIn('resolved',self.store.task(name)['data'])
        work=discover(self.store,'implement','main')
        self.assertEqual(len(work),2)
        self.store.set_setting('observed_main','new-main')
        row=dict(self.store.db.execute('SELECT * FROM work WHERE id=?',(work[0],)).fetchone())
        self.assertFalse(eligible(self.store,row))
        self.assertEqual(discover(self.store,'implement','new-main'),[])

    def test_resolved_baseline_reverifies_reviewed_candidate_without_another_implementation(self):
        prerequisite,_=self.run_probe(True)
        self.store.put_task('task-003','g','body',{'dependencies':[prerequisite],'triaged':'g',
                'candidate':'reviewed-source','candidate_base':'old-main',
                'review':{'candidate':'reviewed-source','approved':True},
                'repair':{'category':'baseline','source':'reviewed-source','prerequisite':prerequisite}})
        self.assertEqual(discover(self.store,'implement','main'),[])
        ready=discover(self.store,'verify','main')
        self.assertEqual(len(ready),1)
        data=self.store.task('task-003')['data']
        self.assertIsNone(data['repair'])
        self.assertEqual(data['candidate'],'reviewed-source')
        self.assertTrue(data['review']['approved'])
        self.assertNotIn('delivered',data)

    def test_historical_failure_cannot_execute_an_arbitrary_command(self):
        task=self.store.task('task-001')
        task['data']['baseline_probe']=['bash','/tmp/arbitrary-script']
        self.assertIsNone(probe(task))

    def test_matching_rechecks_share_one_claim_before_a_remote_receipt_exists(self):
        discover(self.store,'triage','main')
        first=self.store.claim('triage','first',concurrency=2,capacity=2,eligible=lambda w:eligible(self.store,w))
        self.assertIsNotNone(first)
        self.assertIsNone(self.store.claim('triage','second',concurrency=2,capacity=2,eligible=lambda w:eligible(self.store,w)))
        self.assertEqual(self.store.compute_used(),1)

    def test_infrastructure_timeout_does_not_resolve_or_cache_a_repair(self):
        work=discover(self.store,'triage','main')[0]
        execution=Execution(self.store,self.store.claim('triage','fixture',ident=work))
        receipt={'passed':False,'checks':[{'exit_code':124}]}
        with patch('pipeline.stages.checkout',return_value=Path(self.temp.name)), \
             patch.object(execution,'command',return_value=subprocess.CompletedProcess([],0,'tree\n','')), \
             patch('pipeline.verification.run',return_value=receipt):
            with self.assertRaises(Retry):recheck(execution,self.store.task(execution.claim['task']))
        self.assertFalse(self.store.task(execution.claim['task'])['data'].get('resolved'))
        self.assertEqual(self.store.db.execute("SELECT count(*) FROM settings WHERE key LIKE 'baseline-check:%'").fetchone()[0],0)


if __name__ == '__main__':
    unittest.main()
