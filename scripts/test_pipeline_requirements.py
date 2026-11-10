import json
from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest.mock import Mock, patch

from pipeline.catalog import contract, sync_checkout, discover, generation_for
from pipeline.requirements import snapshot, sections
from pipeline.store import Store
from pipeline.eligibility import eligible
from pipeline.assessment import validate, run
from pipeline.cockpit import snapshot as cockpit

BODY='---\ntitle: Feature\nspec_ref: behavior.md §One\ndepends_on: []\nprogress: complete\n---\n## Acceptance\nReal behavior\n'

class RequirementTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        self.root=Path(self.tmp.name)/'source'
        (self.root/'specs/system').mkdir(parents=True)
        (self.root/'specs/tasks').mkdir()
        (self.root/'specs/GOAL.md').write_text('Real production behavior')
        (self.root/'specs/tasks/task-001.md').write_text(BODY)
        self.spec=self.root/'specs/system/behavior.md'
        self.spec.write_text('# Contract\nCommon\n## One\nMust do one\n### Child\nInherited child\n## Other\nUnrelated\n')
        self.registry=self.root/'specs/task-spec-inputs.json'
        self.registry.write_text(json.dumps({'bindings':{'behavior.md §One':['stable-one']},
            'sections':{'stable-one':{'file':'specs/system/behavior.md','heading':['Contract','One']}}}))
        self.store=Store(self.tmp.name);self.addCleanup(self.store.close)

    def sync(self):sync_checkout(self.store,self.root,'main')
    def snap(self):return snapshot(self.root,BODY,contract)

    def test_unrelated_edits_do_not_invalidate_but_ancestor_and_child_edits_do(self):
        before=self.snap();self.spec.write_text(self.spec.read_text().replace('Unrelated','Different sibling'))
        self.assertEqual(before['fingerprint'],self.snap()['fingerprint'])
        self.spec.write_text(self.spec.read_text().replace('Inherited child','Changed child'))
        self.assertNotEqual(before['fingerprint'],self.snap()['fingerprint'])
        before=self.snap();self.spec.write_text(self.spec.read_text().replace('Common','Changed common'))
        self.assertNotEqual(before['fingerprint'],self.snap()['fingerprint'])

    def test_missing_renamed_ambiguous_heading_requires_triage_not_coding(self):
        self.sync();self.spec.write_text('# Contract\n## Renamed\nChanged\n')
        self.sync();task=self.store.task('task-001')
        self.assertTrue(task['data']['requirements']['errors'])
        self.assertEqual(discover(self.store,'implement','main'),[])
        self.assertEqual(discover(self.store,'assess','main'),[])
        work=discover(self.store,'triage','main')[0]
        self.assertIsNotNone(self.store.claim('triage','owner',eligible=lambda w:eligible(self.store,w)))
        self.assertEqual(cockpit(self.store)['tasks'][0]['state'],'running')
        self.assertEqual(cockpit(self.store)['tasks'][0]['stage'],'triage')
        self.spec.write_text('# Contract\n## One\nFirst\n## One\nSecond\n')
        self.assertIn('ambiguous',self.snap()['errors'][0])

    def test_format_migration_keeps_claim_and_receipts(self):
        self.registry.unlink();self.sync()
        task=self.store.task('task-001');epoch=task['generation']
        self.store.put_task('task-001',epoch,BODY,task['data']|{'delivered':None,'triaged':epoch})
        work=discover(self.store,'implement','main')[0];claim=self.store.claim('implement','owner')
        self.registry.write_text(json.dumps({'bindings':{'behavior.md §One':['x']},'sections':{'x':{'file':'specs/system/behavior.md','heading':['Contract','One']}}}))
        self.sync()
        self.assertEqual(self.store.task('task-001')['generation'],epoch)
        self.assertTrue(eligible(self.store,claim))
        self.assertEqual(generation_for(self.store,BODY,'task-001'),epoch)
        self.assertFalse(self.store.task('task-001')['data'].get('assessment_pending'))

    def test_changed_requirements_fence_claim_and_archive_shipping(self):
        self.sync();task=self.store.task('task-001');epoch=task['generation']
        self.store.put_task('task-001',epoch,BODY,task['data']|{'delivered':{'sha':'shipped'}})
        self.spec.write_text(self.spec.read_text().replace('Must do one','Must do two'))
        self.sync();task=self.store.task('task-001')
        self.assertNotEqual(task['generation'],epoch)
        self.assertIsNone(task['data']['delivered'])
        self.assertEqual(task['data']['previous_delivery']['sha'],'shipped')
        self.assertEqual(discover(self.store,'implement','main'),[])
        work=discover(self.store,'assess','main')[0]
        self.assertEqual(discover(self.store,'assess','main'),[work])
        self.assertIsNotNone(self.store.claim('assess','owner',eligible=lambda w:eligible(self.store,w)))
        view=cockpit(self.store)
        self.assertEqual(view['confirmed_merges'],1)
        self.assertEqual(view['tasks'][0]['state'],'running')
        self.assertEqual(view['tasks'][0]['stage'],'assess')
        with self.assertRaises(sqlite3.IntegrityError):self.store.db.execute('DELETE FROM task_history')

    def test_fenced_code_and_assessment_cannot_commit_old_requirement_receipts(self):
        self.sync();t=self.store.task('task-001')
        self.store.put_task(t['name'],t['generation'],BODY,t['data']|{'delivered':None,'triaged':t['generation']})
        discover(self.store,'implement','main');claim=self.store.claim('implement','owner')
        self.spec.write_text(self.spec.read_text().replace('Must do one','Must do two'));self.sync()
        self.assertFalse(eligible(self.store,claim))
        discover(self.store,'assess','main');claim=self.store.claim('assess','owner')
        self.spec.write_text(self.spec.read_text().replace('Must do two','Must do three'));self.sync()
        self.assertFalse(eligible(self.store,claim))

    def test_shared_input_change_and_dependency_metadata_have_distinct_effects(self):
        before=self.snap()
        body=BODY.replace('depends_on: []','depends_on: [task-002]')
        self.assertEqual(before['fingerprint'],snapshot(self.root,body,contract)['fingerprint'])
        registry=json.loads(self.registry.read_text())
        registry['shared']=['common']
        registry['sections']['common']={'file':'specs/system/behavior.md','heading':['Contract','Other']}
        self.registry.write_text(json.dumps(registry));before=self.snap()
        self.spec.write_text(self.spec.read_text().replace('Unrelated','Shared rule changed'))
        self.assertNotEqual(before['fingerprint'],self.snap()['fingerprint'])

    def test_reconstructs_only_ancestral_receipt_with_equal_scoped_contract(self):
        import subprocess
        from pipeline.requirement_migration import plan,apply
        def git(*args):return subprocess.check_output(['git',*args],cwd=self.root,text=True).strip()
        git('init','-q');git('config','user.name','Test');git('config','user.email','test@example.com')
        git('add','specs');git('-c','core.hooksPath=/dev/null','commit','-qm','Original delivery')
        sha=git('rev-parse','HEAD')
        legacy=contract.generation(BODY,{'specs/system/behavior.md':self.spec.read_text()},(self.root/'specs/GOAL.md').read_text())
        self.store.put_task('task-001',legacy,BODY,{})
        ident=self.store.enqueue('observe','task-001',legacy,{'url':'https://github.com/org/repo/pull/1'})
        claim=self.store.claim('observe','observer')
        self.store.finish(ident,claim['token'],{'status':'merged','sha':sha})
        # A sibling edit caused old whole-file hashing to invalidate this task.
        self.spec.write_text(self.spec.read_text().replace('Unrelated','Sibling amended'))
        self.store.put_task('task-001','invalidated',BODY,{'requirements':self.snap(),
             'repair':{'category':'requirements_changed'},'candidate':'retained-new-work'})
        entries=plan(self.store,self.root)
        self.assertTrue(entries[0]['equivalent'])
        applied=apply(self.store,entries);self.assertEqual(applied,['task-001'])
        data=self.store.task('task-001')['data']
        self.assertEqual(data['delivered']['sha'],sha)
        self.assertEqual(data['candidate'],'retained-new-work')
        self.assertEqual(cockpit(self.store)['confirmed_merges'],1)

    def test_gap_creates_durable_repair_without_running_no_gap_verifier(self):
        ex=Mock();ex.claim={'id':'work','input':{'candidate':'sha','base':'base','requirements':'fp'}}
        ex.directory=Path(self.tmp.name)
        finding={'requirement':'one','file':'src.rs:1','detail':'missing rejection'}
        ex.cloud_step.return_value={'valid':True,'task_body':BODY,'assessment':{
            'candidate':'sha','requirements':'fp','decision':'gap','evidence':[finding],'gaps':[finding]}}
        task={'name':'task-001','generation':'g','body':BODY,'data':{}}
        with patch('pipeline.stages.prompt',return_value='prompt'),patch('pipeline.verification.run') as verify:
            result,updates,findings=run(ex,task)
        verify.assert_not_called();self.assertEqual(findings[0]['category'],'requirements_gap')
        self.assertIsNone(updates['assessment_pending'])

    def test_real_pr_766_taxonomy_note_only_invalidates_trust_inputs(self):
        import re,shutil
        root=Path(__file__).resolve().parents[1]
        before=Path(self.tmp.name)/'historical'
        shutil.copytree(root/'specs',before/'specs')
        path=before/'specs/system/human-system-interface.md'
        text=path.read_text()
        text,count=re.subn(r'> \*\*Taxonomy unification \(user-management.md.*?(?=\n\n)', '', text, count=1, flags=re.S)
        self.assertEqual(count,1)
        # Remove precisely the annotation introduced in #766, retaining its
        # original surrounding paragraph whitespace.
        text=text.replace('\n\n\n\n','\n\n')
        path.write_text(text)
        affected=[]
        names=['002','010','011','012','013','015','054','056','077','082','084','085','086','087','088','089','090','091','092','196','208','209']
        for number in names:
            body=(root/'specs/tasks'/f'task-{number}.md').read_text()
            if snapshot(before,body,contract)['fingerprint']!=snapshot(root,body,contract)['fingerprint']:
                affected.append(number)
        self.assertEqual(affected,['077','085'])

    def test_catalog_compare_and_swap_does_not_erase_a_concurrent_delivery(self):
        self.sync();old=self.store.task('task-001')
        self.store.put_task(old['name'],old['generation'],old['body'],old['data']|{'delivered':{'sha':'just-shipped'}})
        self.assertFalse(self.store.put_task(old['name'],old['generation'],old['body'],old['data'],expected=old))
        self.assertEqual(self.store.task('task-001')['data']['delivered']['sha'],'just-shipped')

    def test_old_broad_repair_discovers_assessment_without_fencing_running_code(self):
        self.store.put_task('task-001','legacy',BODY,{'dependencies':[], 'triaged':'legacy',
            'repair':{'category':'requirements_changed','source':'checkpoint'}})
        discover(self.store,'implement','main');claim=self.store.claim('implement','old-worker')
        self.sync()
        self.assertTrue(eligible(self.store,claim))
        self.assertEqual(discover(self.store,'implement','main'),[])
        self.assertEqual(len(discover(self.store,'assess','main')),1)
        self.assertEqual(self.store.task('task-001')['generation'],'legacy')

    def test_assessment_obeys_compute_admission(self):
        self.sync();task=self.store.task('task-001')
        self.store.put_task('task-001',task['generation'],BODY,task['data']|{'delivered':None,
            'assessment_pending':{'changes':{}}})
        discover(self.store,'assess','main')
        self.assertIsNone(self.store.claim('assess','owner',capacity=0))
        self.store.set_setting('verification_reserved',0)
        claim=self.store.claim('assess','owner',capacity=2)
        self.assertIsNotNone(claim)
        self.assertEqual(self.store.compute_used(),1)

    def test_adopted_registry_cannot_fall_back_to_document_hashing(self):
        self.sync();self.registry.unlink();self.sync()
        task=self.store.task('task-001')
        self.assertTrue(task['data']['requirements']['errors'])
        self.assertEqual(discover(self.store,'implement','main'),[])
        self.assertEqual(discover(self.store,'assess','main'),[])
        self.assertEqual(len(discover(self.store,'triage','main')),1)

    def test_failed_assessment_reports_its_actual_failure(self):
        self.sync();task=self.store.task('task-001')
        self.store.put_task('task-001',task['generation'],BODY,task['data']|{'delivered':None,
            'assessment_pending':{'changes':{}}})
        ident=discover(self.store,'assess','main')[0];claim=self.store.claim('assess','owner')
        self.store.retry(ident,claim['token'],{'category':'assessment','message':'A deterministic check failed'},attention=True)
        view=cockpit(self.store)['tasks'][0]
        self.assertEqual(view['state'],'failed');self.assertEqual(view['condition'],'A deterministic check failed')

    def test_fenced_markdown_headings_are_not_inputs(self):
        nodes=sections('# Real\n```sh\n## Fake\n```\n## Actual\ntext\n')
        self.assertEqual([n['heading'] for n in nodes],['','Real','Actual'])

    def test_no_gap_requires_exact_receipt_and_real_verification(self):
        inputs={'candidate':'sha','base':'base','requirements':'fp'}
        report={'candidate':'sha','requirements':'fp','decision':'still_satisfied','gaps':[],
                'evidence':[{'requirement':'x','file':'src.rs:1','detail':'enforced and tested'}]}
        self.assertEqual(validate(report,inputs),report)
        for mutation in [{'candidate':'wrong'},{'requirements':'wrong'},{'evidence':[]},{'gaps':[{'detail':'gap'}]}]:
            with self.assertRaises(ValueError):validate(report|mutation,inputs)
        ex=Mock();ex.claim={'input':inputs};ex.directory=Path(self.tmp.name)
        ex.command.return_value.stdout='tree';ex.cloud_step.return_value={'valid':True,'task_body':BODY,'assessment':report}
        task={'name':'task-001','generation':'g','body':BODY,'data':{'previous_delivery':{'sha':'old'}}}
        with patch('pipeline.stages.prompt',return_value='prompt'),patch('pipeline.stages.checkout'),patch('pipeline.verification.run',return_value={'passed':True,'checks':['real checks']}) as checks:
            result,updates,findings=run(ex,task)
        checks.assert_called_once()
        self.assertIsNone(updates['assessment_pending']);self.assertEqual(updates['delivered']['sha'],'old')
        from pipeline.execution import Attention
        with patch('pipeline.stages.prompt',return_value='prompt'),patch('pipeline.stages.checkout'),patch('pipeline.verification.run',return_value={'passed':False}):
            with self.assertRaises(Attention):run(ex,task)

if __name__=='__main__':unittest.main()
