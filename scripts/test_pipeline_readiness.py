"""Delivery labels must describe current ledger state, not an old green CI run."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

from pipeline.store import Store

spec = importlib.util.spec_from_file_location('readiness_adapter', Path(__file__).with_name('dispatch-adapter.py'))
adapter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(adapter)


class DeliveryReadinessTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(); self.addCleanup(self.tmp.cleanup)
        self.store = Store(self.tmp.name); self.addCleanup(self.store.close)
        self.receipt = {'candidate':'source','head':'published','base':'main','passed':True}
        self.data = {'candidate':'source','candidate_base':'main','triaged':'g','dependencies':[],
                     'review':{'candidate':'source','approved':True},'verified':self.receipt,
                     'published_verified':self.receipt,'published_head':'published',
                     'pr':'https://github.com/example/repo/pull/1',
                     'ci':{'head':'published','status':'passed','checks':'green','merge_state':'CLEAN'}}
        self.store.set_setting('observed_main','main')
        self.save()
        self.publish = self.store.enqueue('publish','task-001','g',{'candidate':'source','verified':self.receipt})
        claim = self.store.claim('publish','publisher')
        self.store.finish(self.publish,claim['token'],{'published':True})

    def save(self, **updates):
        self.data.update(updates)
        self.store.put_task('task-001','g','requirements',self.data)

    def readiness(self):
        return next(r['code_readiness'] for r in adapter.read_rows(self.store) if r['key']==self.publish)

    def test_recorded_delivery_and_resolution_override_stale_green_ci_without_rewriting_outcome(self):
        self.assertEqual(self.readiness()['status'],'passed')
        outcome = self.store.db.execute('SELECT result FROM work WHERE id=?',(self.publish,)).fetchone()[0]
        self.save(delivered={'sha':'actual-merge','generation':'g'})
        self.assertEqual(self.readiness(),{'status':'merged'})
        self.save(delivered=None,resolved={'source':'upstream','generation':'g'})
        self.assertEqual(self.readiness(),{'status':'resolved'})
        self.assertEqual(self.store.db.execute('SELECT result FROM work WHERE id=?',(self.publish,)).fetchone()[0],outcome)
        self.save(resolved=None,previous_delivery={'sha':'historical-merge'})
        self.assertEqual(self.readiness()['status'],'passed','historical delivery must not complete new work')

    def test_green_ci_with_oom_hold_reports_blocker_instead_of_waiting_for_merge(self):
        self.save(verified=None)
        self.store.set_setting('observed_main','latest')
        work = self.store.enqueue('verify','task-001','g',{'candidate':'source','base':'latest'})
        claim = self.store.claim('verify','verifier')
        self.store.retry(work,claim['token'],{'category':'sandbox_oom','message':'Fixed memory budget exhausted'},attention=True)
        changes = self.store.db.total_changes
        self.assertEqual(self.readiness()['status'],'verification_blocked')
        self.assertEqual(self.readiness()['blocker'],'sandbox_oom')
        self.assertEqual(self.store.db.total_changes,changes,'display must never clear or retry a hold')

    def test_refresh_tracks_required_queued_and_running_then_drops_old_candidate_ci(self):
        self.store.set_setting('observed_main','latest')
        self.assertEqual(self.readiness()['status'],'refresh_required')
        self.save(verified=None)
        work = self.store.enqueue('verify','task-001','g',{'candidate':'source','base':'latest'})
        self.assertEqual(self.readiness()['status'],'refresh_queued')
        claim = self.store.claim('verify','verifier')
        self.assertIsNotNone(claim)
        self.assertEqual(self.readiness()['status'],'refresh_running')
        self.save(candidate='replacement',review=None)
        self.assertIsNone(self.readiness(),'CI on the previous candidate must not describe its replacement')

    def test_repair_and_merge_confirmation_remain_visible_without_inventing_delivery(self):
        self.save(repair={'category':'rebase','source':'source'})
        self.assertEqual(self.readiness()['status'],'repair_pending')
        self.save(repair=None,ci=self.data['ci'] | {'status':'merged'})
        self.assertEqual(self.readiness()['status'],'merge_confirmation')
        self.assertFalse(self.store.task('task-001')['data'].get('delivered'))
