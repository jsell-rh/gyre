"""Policy-qualified recovery must release runnable work without clearing safety holds."""
import json
import tempfile
import time
import unittest

from pipeline.store import Store
from pipeline.compute import record_oom, profile
from pipeline.catalog import helper
from pipeline.execution import Execution
from pipeline.fullsend import adopt
from pipeline.recovery_policy import reconcile, checkbox_delta

contract=helper('dev-contract')


class RecoveryPolicyTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        self.s=Store(self.tmp.name);self.addCleanup(self.s.close)
        self.s.set_setting('slots',100)
        self.s.put_task('task-001','g','body',{})

    def hold(self,reason,stage='verify'):
        work=self.s.enqueue(stage,'task-001','g',{'base':'old'})
        c=self.s.claim(stage,'worker')
        self.s.retry(work,c['token'],reason,attention=True)
        return work

    def test_health_hold_needs_fresh_gateway_and_recovers_once_on_current_inputs(self):
        work=self.hold({'category':'gateway_transport','message':'Cannot establish verifier health; inspect logs'})
        current=self.s.enqueue('verify','task-001','g',{'base':'new'})
        self.assertIsNone(self.s.claim('verify','before',ident=current))
        self.assertEqual(reconcile(self.s),[])
        self.s.event('gateway_inventory',{'sandboxes':0})
        self.s.set_setting('dispatcher_health_at',time.time())
        self.assertEqual(len(reconcile(self.s)),1)
        self.assertEqual(reconcile(self.s),[])
        self.assertEqual(self.s.db.execute('SELECT state FROM work WHERE id=?',(work,)).fetchone()[0],'obsolete')
        self.assertIsNotNone(self.s.claim('verify','after',ident=current))
        self.assertEqual(self.s.setting('slots'),100)
        self.assertEqual(self.s.db.execute("SELECT count(*) FROM events WHERE kind='needs_attention'").fetchone()[0],1)

    def oom_hold(self,gib):
        self.s.set_setting('worker_memory_max_gib',4)
        work=self.s.enqueue('implement','task-001','g',{'base':'old'})
        c=self.s.claim('implement','worker')
        self.s.reserve('owned',work,c['token'],'sandbox',100,{'compute':{'memory_gib':gib}})
        evidence={'sandbox':'owned','reason':'OOMKilled','exit_code':137,'resources':{'limits':{'memory':f'{gib}Gi'}}}
        error=record_oom(self.s,'task-001','implement',gib,evidence)
        self.s.retry(work,c['token'],{'category':error.category,'message':str(error),'diagnostic':evidence},attention=True)
        self.s.resource_state('owned','absent')
        self.s.set_setting('worker_memory_max_gib',6)
        return work

    def test_old_four_gib_hold_uses_owned_evidence_and_new_six_gib_policy(self):
        self.oom_hold(4)
        self.assertEqual(profile(self.s,'task-001','implement')['memory_gib'],6)
        self.assertEqual(len(reconcile(self.s)),1)
        work=self.s.enqueue('implement','task-001','g',{'base':'new'})
        c=self.s.claim('implement','new',ident=work,capacity=100)
        r=self.s.db.execute('SELECT data FROM resources WHERE name=?',(self.s.admission_name(work,c['token']),)).fetchone()
        self.assertEqual(json.loads(r['data'])['compute']['memory'],'6Gi')

    def test_six_gib_oom_and_unscoped_evidence_never_auto_resume(self):
        self.oom_hold(6)
        self.assertEqual(reconcile(self.s),[])
        key=self.s.runtime_hold_key('task-001','implement','g')
        self.assertIsNotNone(self.s.setting(key))
        self.s.set_setting(key,{'category':'sandbox_oom','diagnostic':{'reason':'OOMKilled','exit_code':137,'resources':{'limits':{'memory':'4Gi'}}}})
        self.assertEqual(reconcile(self.s),[])

    def test_operator_pause_and_semantic_triage_remain_held(self):
        self.oom_hold(4);self.s.set_setting('task_paused:task-001',True)
        self.assertEqual(reconcile(self.s),[])
        self.s.set_setting('task_paused:task-001',False)
        self.s.set_setting(self.s.runtime_hold_key('task-001','implement','g'),{'category':'requirements_triage'})
        self.assertEqual(reconcile(self.s),[])

    def test_checkbox_repair_recovery_is_specific_and_budgeted(self):
        delta='--- assigned\n+++ checkpoint\n@@ -1 +1 @@\n-- [ ] Enforce authorization.\n+- [x] Enforce authorization.\n'
        self.assertTrue(checkbox_delta(delta))
        self.assertFalse(checkbox_delta(delta.replace('+- [x] Enforce authorization.','+- [x] Skip authorization.')))
        self.s.put_task('task-001','g','body',{'repair':{'category':'contract','log_tail':delta}})
        work=self.hold({'category':'repair_stalled'},stage='implement')
        prior=self.s.directory/'attempts'/work/'1';(prior/'bundle').mkdir(parents=True)
        (prior/'bundle/job.json').write_text(json.dumps({'stage':'implement','task':'task-001','branch':'old'}))
        (prior/'outcome.json').write_text(json.dumps({'published':True,'head':'already-rejected'}))
        self.assertEqual(len(reconcile(self.s)),1)
        self.assertEqual(reconcile(self.s),[])
        self.s.enqueue('implement','task-001','g',{'base':'new'})
        claim=self.s.claim('implement','fresh',ident=work)
        self.assertTrue(json.loads(claim['result'])['fresh_model'])
        self.assertIsNone(adopt(Execution(self.s,claim),self.s.task('task-001')))

    def test_stale_generations_do_not_release_current_hold(self):
        self.oom_hold(4)
        self.s.put_task('task-001','new','new body',{})
        key=self.s.runtime_hold_key('task-001','implement','new')
        self.s.set_setting(key,{'category':'sandbox_oom','diagnostic':{'reason':'OOMKilled','exit_code':137,'resources':{'limits':{'memory':'4Gi'}}}})
        self.assertEqual(reconcile(self.s),[])
        self.assertIsNotNone(self.s.setting(key))


class CheckboxRestorationTests(unittest.TestCase):
    body='---\ntitle: Task\nspec_ref: example.md\ndepends_on: []\nprogress: not-started\n---\n\n## Acceptance Criteria\n\n- [ ] Enforce authorization.\n\n## Shipped\n\nPrevious notes.\n'

    def test_restore_exact_markers_and_keep_progress_and_operational_notes(self):
        observed=self.body.replace('not-started','ready-for-review').replace('- [ ]','- [x]')+'\n- [x] Operational checklist.\n'
        actual=contract.restore_checkbox_markers(self.body,observed)
        self.assertIn('progress: ready-for-review',actual)
        self.assertIn('- [ ] Enforce authorization.',actual)
        self.assertIn('- [x] Operational checklist.',actual)
        self.assertEqual(contract.requirement_parts(actual),contract.requirement_parts(self.body))

    def test_other_requirement_edits_are_retained_for_strict_rejection(self):
        for old,new in [('Enforce authorization.','Skip authorization.'),('depends_on: []','depends_on: [task-002]'),('example.md','other.md')]:
            observed=self.body.replace(old,new).replace('- [ ]','- [x]')
            self.assertEqual(contract.restore_checkbox_markers(self.body,observed),observed)
            self.assertNotEqual(contract.requirement_parts(observed),contract.requirement_parts(self.body))
        truncated='---\ntitle: Task\n'
        self.assertEqual(contract.restore_checkbox_markers(self.body,truncated),truncated)
