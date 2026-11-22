import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from pipeline.store import Store
from pipeline.decisions import request,create,choose,list_decisions,validate
from pipeline.catalog import discover,helper
from pipeline.eligibility import eligible
from pipeline.execution import Execution

REPORT={'question':'Which navigation should we build?','context':'The task requires a sidebar; the newer spec removes it.',
 'options':[{'id':key,'title':title,'summary':'A concrete change','tradeoffs':'A specific cost and benefit','next_step':'A reviewed task/spec correction'} for key,title in [('contextual','Context-aware navigation'),('current','Keep current navigation'),('fixed','Restore fixed sidebar')]],
 'recommendation':'contextual','sources':['specs/system/nav.md']}
BODY='---\ntitle: Navigation\nspec_ref: nav.md\ndepends_on: []\nprogress: not-started\n---\n\n## Requirements\n\nFollow nav.md.\n'

class DecisionTests(unittest.TestCase):
 def setUp(self):
  self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
  self.s=Store(self.tmp.name);self.addCleanup(self.s.close)
  src=self.s.directory/'source';(src/'specs/system').mkdir(parents=True);(src/'specs/GOAL.md').write_text('Real behavior')
  self.spec=src/'specs/system/nav.md';self.spec.write_text('# Navigation\nNo sidebar\n')
  (src/'specs/task-spec-inputs.json').write_text(json.dumps({'bindings':{'nav.md':['nav']},'sections':{'nav':{'file':'specs/system/nav.md','document':True}}}))
  self.s.put_task('task-082','g',BODY,{'title':'Navigation','spec_ref':'nav.md','requirements':{'fingerprint':'fp'},'assessment_pending':{'changes':{}},'dependencies':[]})
  self.s.set_setting(self.s.runtime_hold_key('task-082','assess','g'),{'category':'requirements_triage','message':'nav.md contradicts the task'})
 def ask(self):
  task=self.s.task('task-082');expected=request(self.s,task);return create(self.s,task,expected,REPORT)
 def test_discovery_routes_semantic_hold_to_read_only_triage_not_implementation(self):
  self.assertEqual(discover(self.s,'implement','main'),[])
  work=discover(self.s,'triage','main')[0]
  claim=self.s.claim('triage','operator-helper',eligible=lambda w:eligible(self.s,w))
  self.assertEqual(claim['id'],work);self.assertTrue(claim['input']['decision_request'])
  task=self.s.task('task-082');fake={'valid':True,'source_valid':True,'agent_exit':0,'task_body':BODY,'decision':REPORT}
  cli=helper('dev-pipeline')
  with patch.object(Execution,'cloud_step',return_value=fake),patch('pipeline.stages.prompt',return_value='read-only decision'):
   cli.run_claim(self.s,claim)
  self.assertEqual(self.s.db.execute('SELECT state FROM work WHERE id=?',(work,)).fetchone()[0],'succeeded')
  self.assertEqual(len(list_decisions(self.s)),1)
  self.assertEqual(discover(self.s,'triage','main'),[])
  self.assertFalse(self.s.task('task-082')['data'].get('delivered'))
 def test_selection_is_durable_idempotent_and_queues_a_scoped_amendment(self):
  identity=self.ask();result=choose(self.s,identity,'contextual',actor='tester')
  self.assertEqual(choose(self.s,identity,'contextual'),result)
  with self.assertRaisesRegex(ValueError,'already recorded'):choose(self.s,identity,'fixed')
  d=list_decisions(self.s)[0];self.assertEqual(d['selection']['actor'],'tester')
  task=self.s.task(result['application_task']);self.assertEqual(task['data']['source_decision'],identity)
  self.assertIn('No production code',task['body']);self.assertIn('contextual',task['body'])
  self.assertIsNotNone(self.s.setting(self.s.runtime_hold_key('task-082','assess','g')))
  self.assertEqual(len(self.s.tasks()),2)
 def test_section_symbol_in_spec_binding_survives_amendment_frontmatter(self):
  ref='nav.md §1'
  body=BODY.replace('spec_ref: nav.md','spec_ref: '+ref)
  task=self.s.task('task-082');data=task['data']|{'spec_ref':ref}
  self.s.put_task('task-082','g',body,data)
  registry=self.s.directory/'source/specs/task-spec-inputs.json'
  content=json.loads(registry.read_text());content['bindings'][ref]=content['bindings']['nav.md'];registry.write_text(json.dumps(content))
  identity=self.ask();result=choose(self.s,identity,'contextual')
  amendment=self.s.task(result['application_task'])
  self.assertEqual(amendment['data']['spec_ref'],ref)
  self.assertEqual(amendment['data']['requirements']['errors'],[])

 def test_changed_spec_or_task_fences_a_stale_choice_without_queueing_work(self):
  identity=self.ask();self.spec.write_text('# Navigation\nNew requirements\n')
  with self.assertRaisesRegex(ValueError,'context changed'):choose(self.s,identity,'current')
  self.assertEqual(len(self.s.tasks()),1);self.assertEqual(list_decisions(self.s),[])
 def test_invalid_options_and_unselected_recommendations_do_not_resume(self):
  for change in ({'options':REPORT['options'][:2]},{'recommendation':'unknown'},{'options':[REPORT['options'][0]]*3}):
   with self.assertRaises(ValueError):validate(REPORT|change)
  self.ask();self.assertEqual(len(self.s.tasks()),1)
  self.assertIsNone(list_decisions(self.s)[0]['selection'])
