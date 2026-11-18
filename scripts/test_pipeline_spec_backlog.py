"""Exercise the actual durable stage chain and real Git publication with a local forge."""
import copy
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from pipeline.store import Store, STAGES
from pipeline.execution import Execution, Attention
from pipeline.catalog import discover, helper, sync_checkout
from pipeline.eligibility import eligible
from pipeline.requirements import digest
from pipeline.spec_backlog import (inventory, validate_audit, validate_plan, validate_manifest,
                                   allocate, GOAL_REF, summary)


class SpecBacklogTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        self.root=Path(self.tmp.name);self.store=Store(self.root/'state');self.addCleanup(self.store.close)
        self.repo=self.store.directory/'source';self.repo.mkdir()
        self.git('init','-q');self.git('config','user.name','Test');self.git('config','user.email','test@example.com')
        for folder in ['specs/system','specs/development','specs/tasks','crates']:(self.repo/folder).mkdir(parents=True,exist_ok=True)
        (self.repo/'specs/GOAL.md').write_text('Production implementations only; no fake closure.\n')
        (self.repo/'specs/system/example.md').write_text('# Example\nContext for this specification.\n\n## Isolation\nReject cross-tenant reads.\n')
        (self.repo/'crates/worker.rs').write_text('fn read() { load_all(); }\n')
        (self.repo/'specs/task-spec-inputs.json').write_text(json.dumps({'sections':{'goal':{'file':'specs/GOAL.md','document':True}},'bindings':{GOAL_REF:['goal']},'shared':[]}))
        self.git('add','.');self.git('commit','-qm','base');self.base=self.git('rev-parse','HEAD')
        self.origin=self.root/'forge.git';subprocess.run(['git','clone','-q','--bare',str(self.repo),str(self.origin)],check=True)
        self.git('remote','add','origin',str(self.origin));self.store.set_setting('repo_url',str(self.origin))
        self.store.set_setting('observed_main',self.base);self.store.set_setting('spec_reconciliation_enabled',True)
        inventory(self.store,self.repo,self.base);self.name=next(t['name'] for t in self.store.tasks() if t['data'].get('spec_planner'))
        self.cli=helper('dev-pipeline')

    def git(self,*args,cwd=None):
        return subprocess.check_output(['git',*args],cwd=cwd or self.repo,text=True,stderr=subprocess.PIPE).strip()

    def checkout(self,execution,source):
        path=execution.directory/'checkout'
        if not path.exists():subprocess.run(['git','clone','-q',str(self.origin),str(path)],check=True)
        self.git('checkout','-q','--detach',source,cwd=path)
        return path

    def audit(self,gap=True):
        task=self.store.task(self.name);rows=[]
        for identity,heading in task['data']['sections'].items():
            normative=heading[-1]=='Isolation'
            rows.append({'id':identity,'decision':'gap' if gap and normative else 'satisfied' if normative else 'context',
                'requirement_quote':'Reject cross-tenant reads.' if normative else 'Context for this specification.',
                'detail':'Production read omits tenant filtering' if normative else 'Introductory rationale',
                'evidence':[{'file':'crates/worker.rs','quote':'load_all();','detail':'Read path has no tenant predicate'}]})
        return {'base':self.base,'requirements':task['generation'],'sections':rows}

    def run_stage(self,stage,report=None):
        queued=discover(self.store,stage,self.base);self.assertEqual(len(queued),1)
        claim=self.store.claim(stage,'test',eligible=lambda w:eligible(self.store,w));self.assertIsNotNone(claim)
        task=self.store.task(claim['task'])
        result={'valid':True,'source_valid':True,'agent_exit':0,'stream_exit':0,'task_body':task['body'],
                'head':self.base,'base':self.base,'planning':report,'published':True,'branch':'private'}
        with patch.object(Execution,'cloud_step',return_value=result),patch('pipeline.stages.prompt',return_value='read-only'),patch('pipeline.stages.checkout',side_effect=self.checkout):
            self.cli.run_claim(self.store,claim)
        work=dict(self.store.db.execute('SELECT * FROM work WHERE id=?',(claim['id'],)).fetchone())
        self.assertEqual(work['state'],'succeeded',work.get('result'))
        return work

    def planned(self):
        self.run_stage('spec_audit',self.audit())
        audit=self.store.task(self.name)['data']['spec_audit']
        plan={'audit':digest(audit),'tasks':[{'key':'tenant-read','title':'Enforce tenant-scoped reads',
            'covers':[g['id'] for g in audit['gaps']],'required_behavior':'Reject reads of another tenant’s records.',
            'acceptance':['Cross-tenant reads return Forbidden.'],'negative_probe':'Remove the tenant predicate; cross-tenant denial probe must fail.',
            'dependencies':[],'existing_task':None}]}
        self.run_stage('decompose',plan)
        return plan

    def reviewed(self):
        plan=self.planned();task=self.store.task(self.name)
        review={'plan':digest(plan),'approved':True,'findings':[],
                'checks':[{'id':i,'detail':'Independently inspected read call path and canonical requirement.'} for i in task['data']['sections']]}
        self.run_stage('plan_review',review)
        return plan

    def test_full_chain_creates_reviewable_git_backlog_before_releasing_product_work(self):
        self.reviewed();self.run_stage('backlog')
        data=self.store.task(self.name)['data'];publication=self.store.task(data['backlog_publication'])
        generated=data['backlog_tasks'][0]
        self.assertFalse(self.store.db.execute('SELECT 1 FROM tasks WHERE name=?',(generated,)).fetchone())
        self.assertIsNone(publication['data'].get('delivered'))
        self.assertIn(publication['name'],[self.store.db.execute('SELECT task FROM work WHERE id=?',(w,)).fetchone()[0] for w in discover(self.store,'review',self.base)])
        self.assertEqual(discover(self.store,'implement',self.base),[])
        self.assertEqual(discover(self.store,'backlog',self.base),[])
        head=publication['data']['candidate'];self.git('fetch','-q','origin',head);self.git('checkout','-q','--detach',head)
        validate_manifest(self.repo,publication['data']['backlog_manifest'])
        changed=self.git('diff','--name-only',self.base,head).splitlines()
        self.assertTrue(all(p.startswith('specs/tasks/') or p=='specs/task-spec-inputs.json' for p in changed))
        self.assertIn('progress: not-started',(self.repo/'specs/tasks'/f'{generated}.md').read_text())
        # Stand in for the normal reviewed/verified GitHub merge: only source import releases work.
        sync_checkout(self.store,self.repo,head)
        work=discover(self.store,'triage',head);self.assertTrue(work)
        self.assertFalse(self.store.task(generated)['data']['requirements']['errors'])
        self.assertEqual(self.store.db.execute('SELECT count(*) FROM backlog_publications').fetchone()[0],1)

    def test_missing_heading_forged_evidence_and_wrong_source_are_rejected(self):
        for mutate in ['heading','evidence','source','quote']:
            report=self.audit()
            if mutate=='heading':report['sections'].pop()
            if mutate=='evidence':report['sections'][0]['evidence'][0]['quote']='invented source'
            if mutate=='source':report['base']='wrong'
            if mutate=='quote':report['sections'][1]['requirement_quote']='a weaker invented requirement'
            with self.subTest(mutate=mutate),self.assertRaises(ValueError):validate_audit(report,self.store.task(self.name),self.base,self.repo)

    def test_unreviewed_no_gap_audit_is_not_completion_or_coverage_verification(self):
        self.run_stage('spec_audit',self.audit(False))
        self.assertEqual(self.store.task(self.name)['data']['status'],'audited')
        self.assertEqual(discover(self.store,'backlog',self.base),[])
        self.assertTrue(discover(self.store,'plan_review',self.base))
        self.assertIsNone(self.store.task(self.name)['data'].get('resolved'))

    def test_stale_spec_fences_old_claim_and_context_only_main_advance_does_not(self):
        work=discover(self.store,'spec_audit',self.base)[0]
        claim=self.store.claim('spec_audit','auditor',eligible=lambda w:eligible(self.store,w))
        inventory(self.store,self.repo,'unrelated-main');self.assertTrue(eligible(self.store,claim))
        p=self.repo/'specs/system/example.md';p.write_text(p.read_text()+'\n## New requirement\nActually enforce writes.\n')
        inventory(self.store,self.repo,'new-main');self.assertFalse(eligible(self.store,claim))

    def test_stale_code_evidence_discards_plan_before_any_task_or_push(self):
        self.reviewed();(self.repo/'crates/worker.rs').write_text('fn read() { scoped_load(); }\n')
        self.run_stage('backlog')
        self.assertEqual(self.store.task(self.name)['data']['status'],'pending')
        self.assertFalse(any(t['name'].startswith('task-') for t in self.store.tasks()))

    def test_plan_rejects_missing_gaps_duplicate_owners_cycles_and_finished_task_reuse(self):
        plan=self.planned();audit=self.store.task(self.name)['data']['spec_audit']
        variants=[]
        p=copy.deepcopy(plan);p['tasks'][0]['covers']=[];variants.append(p)
        p=copy.deepcopy(plan);p['tasks'].append(copy.deepcopy(p['tasks'][0])|{'key':'duplicate'});variants.append(p)
        p=copy.deepcopy(plan);p['tasks'][0]['dependencies']=['tenant-read'];variants.append(p)
        body='---\ntitle: Old\nspec_ref: old\ndepends_on: []\nprogress: complete\n---\n'
        self.store.put_task('task-004','old',body,{'delivered':{'generation':'old'},'progress':'complete'})
        p=copy.deepcopy(plan);p['tasks'][0]['existing_task']='task-004';variants.append(p)
        for p in variants:
            with self.assertRaises(ValueError):validate_plan(p,audit,self.store)

    def test_concurrency_cap_and_global_slot_admission_are_shared(self):
        self.store.set_setting('spec_planning_concurrency',1)
        discover(self.store,'spec_audit',self.base)
        claim=self.store.claim('spec_audit','one',eligible=lambda w:eligible(self.store,w));self.assertIsNotNone(claim)
        (self.repo/'specs/system/second.md').write_text('# Second\nOther behavior.\n');inventory(self.store,self.repo,self.base)
        discover(self.store,'spec_audit',self.base)
        self.assertIsNone(self.store.claim('spec_audit','two',eligible=lambda w:eligible(self.store,w)))
        self.store.set_setting('slots',0)
        self.assertFalse(self.store.admission_available('decompose'))
        self.assertTrue(self.store.admission_available('backlog'))

    def test_task_id_reservations_cover_unpublished_plans_decisions_and_repairs(self):
        with self.store.transaction():first=allocate(self.store,3,'first plan')
        with self.store.transaction():second=allocate(self.store,1,'operator decision')
        self.assertEqual(first,['task-001','task-002','task-003']);self.assertEqual(second,['task-004'])

    def test_reference_only_spec_is_excluded_without_trusting_coverage_status(self):
        (self.repo/'specs/system/trusted-foundry-integration.md').write_text('# Reference\nDo not implement.\n')
        inventory(self.store,self.repo,self.base)
        self.assertEqual(summary(self.store)['documents'],1)

    def test_review_rejection_returns_concrete_feedback_to_a_new_audit_cycle(self):
        plan=self.planned();task=self.store.task(self.name)
        report={'plan':digest(plan),'approved':False,'findings':['The proposed task omits write-path containment.'],
                'checks':[{'id':i,'detail':'Inspected the real caller and found the missing path.'} for i in task['data']['sections']]}
        self.run_stage('plan_review',report)
        data=self.store.task(self.name)['data'];self.assertIsNone(data['spec_audit']);self.assertEqual(data['plan_revision'],1)
        self.assertTrue(discover(self.store,'spec_audit',self.base))

    def test_published_manifest_rejects_rewritten_bindings_and_task_definitions(self):
        self.reviewed();self.run_stage('backlog');data=self.store.task(self.name)['data']
        pub=self.store.task(data['backlog_publication']);self.git('fetch','-q','origin',pub['data']['candidate']);self.git('checkout','-q','--detach',pub['data']['candidate'])
        manifest=pub['data']['backlog_manifest'];validate_manifest(self.repo,manifest)
        registry=self.repo/'specs/task-spec-inputs.json';old=registry.read_text();content=json.loads(old)
        content['bindings'][GOAL_REF]=[];registry.write_text(json.dumps(content))
        with self.assertRaises(Attention):validate_manifest(self.repo,manifest)
        registry.write_text(old);target=self.repo/next(iter(manifest['files']));target.write_text(target.read_text().replace('Forbidden','success'))
        with self.assertRaises(Attention):validate_manifest(self.repo,manifest)

    def test_lost_response_after_push_reuses_exact_commit_and_reserved_names(self):
        self.reviewed()
        work=discover(self.store,'backlog',self.base)[0]
        claim=self.store.claim('backlog','first',eligible=lambda w:eligible(self.store,w))
        with patch('pipeline.stages.checkout',side_effect=self.checkout),patch.object(self.store,'put_task',side_effect=OSError('simulated crash after push')):
            self.cli.run_claim(self.store,claim)
        row=self.store.db.execute('SELECT names FROM backlog_publications').fetchone();names=json.loads(row[0])
        branches=self.git('ls-remote','--heads','origin','pipeline/backlog/*');first_head=branches.split()[0]
        self.store.db.execute("UPDATE work SET retry_at=0 WHERE id=?",(work,))
        second=self.store.claim('backlog','recovery',eligible=lambda w:eligible(self.store,w))
        with patch('pipeline.stages.checkout',side_effect=self.checkout):self.cli.run_claim(self.store,second)
        state=self.store.db.execute('SELECT state,result FROM work WHERE id=?',(work,)).fetchone()
        self.assertEqual(state['state'],'succeeded',state['result'])
        pub=self.store.task(names['_publication'])
        self.assertEqual(pub['data']['candidate'],first_head)
        self.assertEqual(self.store.db.execute('SELECT count(*) FROM backlog_publications').fetchone()[0],1)

    def test_planning_claims_are_visible_without_inflating_shipped_task_counts(self):
        from pipeline.cockpit import snapshot
        discover(self.store,'spec_audit',self.base)
        self.store.claim('spec_audit','auditor',eligible=lambda w:eligible(self.store,w))
        view=snapshot(self.store)
        self.assertEqual(view['running'],1)
        self.assertEqual(view['tasks'],[])
        self.assertEqual(view['confirmed_merges'],0)
        self.assertEqual(view['spec_reconciliation']['documents'],1)

    def test_same_gap_cannot_be_republished_as_a_reworded_repair_plan(self):
        self.reviewed();self.run_stage('backlog')
        task=self.store.task(self.name);data=copy.deepcopy(task['data'])
        data['decomposition']['tasks'][0]['title']='Reworded task for the identical unfixed gap'
        data['plan_review']['plan']=digest(data['decomposition']);data['status']='reviewed'
        self.store.put_task(self.name,task['generation'],task['body'],data)
        queued=discover(self.store,'backlog',self.base);self.assertEqual(len(queued),1)
        claim=self.store.claim('backlog','second-plan',eligible=lambda w:eligible(self.store,w))
        with patch('pipeline.stages.checkout',side_effect=self.checkout):self.cli.run_claim(self.store,claim)
        row=self.store.db.execute('SELECT state,result FROM work WHERE id=?',(claim['id'],)).fetchone()
        self.assertEqual(row['state'],'attention');self.assertEqual(json.loads(row['result'])['category'],'spec_fidelity')
        self.assertEqual(self.store.db.execute('SELECT count(*) FROM backlog_publications').fetchone()[0],1)

    def test_spec_conflict_routes_to_three_option_human_decision_not_implementation(self):
        from pipeline.decisions import request,create,choose
        from test_pipeline_decisions import REPORT
        report=self.audit();report['sections'][1]['decision']='conflict'
        work=discover(self.store,'spec_audit',self.base)[0]
        claim=self.store.claim('spec_audit','auditor',eligible=lambda w:eligible(self.store,w));task=self.store.task(self.name)
        outcome={'valid':True,'source_valid':True,'agent_exit':0,'stream_exit':0,'task_body':task['body'],'planning':report}
        with patch.object(Execution,'cloud_step',return_value=outcome),patch('pipeline.stages.prompt',return_value='readonly'),patch('pipeline.stages.checkout',return_value=self.repo):
            self.cli.run_claim(self.store,claim)
        self.assertEqual(self.store.db.execute('SELECT state FROM work WHERE id=?',(work,)).fetchone()[0],'attention')
        self.assertEqual(discover(self.store,'implement',self.base),[])
        self.assertEqual(len(discover(self.store,'triage',self.base)),1)
        expected=request(self.store,task);self.assertIsNotNone(expected)
        identity=create(self.store,task,expected,REPORT)
        result=choose(self.store,identity,'contextual',actor='operator')
        amendment=self.store.task(result['application_task'])
        self.assertEqual(amendment['data']['spec_ref'],GOAL_REF)
        self.assertFalse(amendment['data']['requirements']['errors'])

    def test_completed_human_amendment_releases_conflict_even_if_other_spec_changed(self):
        from pipeline.decisions import request,create,choose
        from test_pipeline_decisions import REPORT
        key=self.store.runtime_hold_key(self.name,'spec_audit',self.store.task(self.name)['generation'])
        self.store.set_setting(key,{'category':'requirements_triage','message':'example.md conflicts with another canonical document'})
        task=self.store.task(self.name);identity=create(self.store,task,request(self.store,task),REPORT)
        result=choose(self.store,identity,'contextual')
        amendment=self.store.task(result['application_task']);data=amendment['data']|{'resolved':{'reason':'reviewed amendment merged upstream'}}
        self.store.put_task(amendment['name'],amendment['generation'],amendment['body'],data)
        inventory(self.store,self.repo,'main-with-amendment')
        self.assertIsNone(self.store.setting(key))
        queued=discover(self.store,'spec_audit',self.base);self.assertEqual(len(queued),1)
        self.assertEqual(json.loads(self.store.db.execute('SELECT input FROM work WHERE id=?',(queued[0],)).fetchone()[0])['cycle'],1)

    def test_downstream_planning_is_not_starved_by_a_large_inventory(self):
        from pipeline.priority import info
        plan=self.planned();review_work=discover(self.store,'plan_review',self.base)[0]
        (self.repo/'specs/system/second.md').write_text('# Second\nOther behavior.\n');inventory(self.store,self.repo,self.base)
        audit_work=discover(self.store,'spec_audit',self.base)[0]
        self.store.db.execute('UPDATE work SET ready_since=1 WHERE id=?',(audit_work,))
        review=dict(self.store.db.execute('SELECT * FROM work WHERE id=?',(review_work,)).fetchone())
        audit=dict(self.store.db.execute('SELECT * FROM work WHERE id=?',(audit_work,)).fetchone())
        self.assertGreater(info(self.store,review)['effective'],info(self.store,audit)['effective'])
        self.assertLess(info(self.store,review)['effective'],0)

    def test_deleted_or_renamed_specs_fence_claims_and_do_not_accumulate_phantom_work(self):
        discover(self.store,'spec_audit',self.base)
        claim=self.store.claim('spec_audit','auditor',eligible=lambda w:eligible(self.store,w))
        p=self.repo/'specs/system/example.md';old=p.read_text();p.unlink()
        inventory(self.store,self.repo,'deleted-main')
        self.assertFalse(eligible(self.store,claim));self.assertEqual(summary(self.store)['documents'],0)
        retired=self.store.task(self.name)['generation'];inventory(self.store,self.repo,'still-deleted')
        self.assertEqual(self.store.task(self.name)['generation'],retired)
        p.write_text(old);inventory(self.store,self.repo,'restored-main')
        self.assertEqual(summary(self.store)['documents'],1)
        self.assertFalse(self.store.task(self.name)['data'].get('spec_retired'))

    def test_spec_change_during_push_cannot_release_a_child_task_from_fenced_planning(self):
        self.reviewed();work=discover(self.store,'backlog',self.base)[0]
        claim=self.store.claim('backlog','publisher',eligible=lambda w:eligible(self.store,w))
        original=Execution.command
        def command(execution,*args,**kwargs):
            result=original(execution,*args,**kwargs)
            if args[:2]==('git','push'):
                p=self.repo/'specs/system/example.md';p.write_text(p.read_text()+'\nNew canonical requirement.\n')
                inventory(self.store,self.repo,'spec-changed-during-push')
            return result
        with patch.object(Execution,'command',command),patch('pipeline.stages.checkout',side_effect=self.checkout):
            self.cli.run_claim(self.store,claim)
        self.assertFalse(any(t['name'].startswith('task-') for t in self.store.tasks()))

    def test_reused_tasks_cannot_silently_gain_unapplied_dependency_edges(self):
        plan=self.planned();audit=self.store.task(self.name)['data']['spec_audit']
        for name in ('task-004','task-005'):
            self.store.put_task(name,'g','body',{'dependencies':[],'title':name})
        plan['tasks'][0].update(existing_task='task-004',dependencies=['task-005'])
        with self.assertRaisesRegex(ValueError,'silently add dependencies'):validate_plan(plan,audit,self.store)
