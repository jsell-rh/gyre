"""Exercise the actual durable stage chain and real Git publication with a local forge."""
import copy
import json
from pathlib import Path
import subprocess
import sys
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

    def run_stage(self,stage,report=None,expected_state='succeeded'):
        queued=discover(self.store,stage,self.base);self.assertEqual(len(queued),1)
        claim=self.store.claim(stage,'test',eligible=lambda w:eligible(self.store,w));self.assertIsNotNone(claim)
        task=self.store.task(claim['task'])
        result={'valid':True,'source_valid':True,'agent_exit':0,'stream_exit':0,'task_body':task['body'],
                'head':self.base,'base':self.base,'planning':report,'published':True,'branch':'private'}
        with patch.object(Execution,'cloud_step',return_value=result),patch('pipeline.stages.prompt',return_value='read-only'),patch('pipeline.stages.checkout',side_effect=self.checkout):
            self.cli.run_claim(self.store,claim)
        work=dict(self.store.db.execute('SELECT * FROM work WHERE id=?',(claim['id'],)).fetchone())
        self.assertEqual(work['state'],expected_state,work.get('result'))
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
        plan=self.reviewed();self.run_stage('backlog')
        data=self.store.task(self.name)['data'];publication=self.store.task(data['backlog_publication'])
        generated=data['backlog_tasks'][0]
        from pipeline.spec_backlog import catalog_context
        self.assertNotIn(publication['name'],[t['task'] for t in catalog_context(self.store,'specs/system/example.md')])
        invalid=copy.deepcopy(plan);invalid['tasks'][0]['existing_task']=publication['name']
        with self.assertRaisesRegex(ValueError,'publication is not a product'):
            validate_plan(invalid,data['spec_audit'],self.store)
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

    def reject_backlog(self):
        data=self.store.task(self.name)['data'];name=data['backlog_publication']
        publication=self.store.task(name);head=publication['data']['candidate']
        discover(self.store,'review',self.base)
        claim=self.store.claim('review','test',task=name,eligible=lambda w:eligible(self.store,w))
        result={'valid':True,'source_valid':True,'agent_exit':0,'stream_exit':0,
                'task_body':publication['body'],'head':head,'base':self.base,'branch':'private',
                'verdict':{'candidate':head,'approved':False,'findings':[
                    {'file':'specs/tasks/'+data['backlog_tasks'][0]+'.md','detail':'Required nested query path is missing'}]}}
        with patch.object(Execution,'cloud_step',return_value=result),patch('pipeline.stages.prompt',return_value='review'),patch('pipeline.stages.checkout',side_effect=self.checkout):
            self.cli.run_claim(self.store,claim)
        return name,dict(self.store.db.execute('SELECT * FROM work WHERE id=?',(claim['id'],)).fetchone())

    def test_backlog_semantic_rejection_replans_without_code_repair_or_duplicate_publication(self):
        plan=self.reviewed();self.run_stage('backlog')
        original=self.store.task(self.name)['data'];reserved=original['backlog_tasks']
        name,work=self.reject_backlog();self.assertEqual(work['state'],'succeeded')
        rejected=self.store.task(name)['data'];self.assertTrue(rejected['backlog_rejected'])
        self.assertIsNone(rejected['repair']);self.assertFalse(rejected.get('delivered'))
        self.assertEqual(discover(self.store,'implement',self.base),[])
        parent=self.store.task(self.name)['data'];self.assertEqual(parent['status'],'pending')
        self.assertEqual(parent['publication_rejections'],1);self.assertEqual(parent['audit_cycle'],1)
        self.assertIn('nested query',json.dumps(parent['plan_feedback']))
        self.assertIn(digest(plan),parent['rejected_plans'])
        self.assertTrue(all(not self.store.db.execute('SELECT 1 FROM tasks WHERE name=?',(n,)).fetchone() for n in reserved))
        first=discover(self.store,'spec_audit',self.base);self.assertEqual(first,discover(self.store,'spec_audit',self.base))
        from pipeline.cockpit import snapshot as cockpit
        visible=next(t for t in cockpit(self.store)['tasks'] if t['name']==name)
        self.assertEqual(visible['state'],'retired');self.assertFalse(visible['current_satisfied'])
        self.run_stage('spec_audit',self.audit());audit=self.store.task(self.name)['data']['spec_audit']
        revised=copy.deepcopy(plan);revised['audit']=digest(audit)
        revised['tasks'][0]['required_behavior']+=' Enforce the same containment on nested queries.'
        self.run_stage('decompose',revised)
        self.run_stage('plan_review',{'plan':digest(revised),'approved':True,'findings':[],
            'checks':[{'id':k,'detail':'Rechecked nested query containment.'} for k in self.store.task(self.name)['data']['sections']]})
        self.run_stage('backlog');new=self.store.task(self.name)['data']
        self.assertNotEqual(new['backlog_publication'],name);self.assertFalse(set(new['backlog_tasks'])&set(reserved))
        self.assertEqual(self.store.db.execute('SELECT count(*) FROM backlog_publications').fetchone()[0],2)
        self.assertEqual(discover(self.store,'implement',self.base),[])

    def test_rejected_plan_cannot_be_resubmitted_unchanged_after_a_new_audit(self):
        plan=self.reviewed();self.run_stage('backlog');self.reject_backlog()
        discover(self.store,'implement',self.base);self.run_stage('spec_audit',self.audit())
        work=self.run_stage('decompose',plan,expected_state='retry')
        self.assertIn('exact plan was rejected',json.loads(work['result'])['message'])
        self.assertEqual(self.store.task(self.name)['data']['status'],'audited')
        self.assertEqual(self.store.db.execute('SELECT count(*) FROM backlog_publications').fetchone()[0],1)

    def test_old_backlog_rejection_does_not_reset_a_new_specification_epoch(self):
        self.reviewed();self.run_stage('backlog');self.reject_backlog()
        (self.repo/'specs/system/example.md').write_text('# Example\nContext for this specification.\n\n## Isolation\nReject cross-tenant reads and writes.\n')
        inventory(self.store,self.repo,self.base);new=self.store.task(self.name)
        discover(self.store,'spec_audit',self.base);again=self.store.task(self.name)
        self.assertEqual(again['generation'],new['generation']);self.assertEqual(again['data']['status'],'pending')
        self.assertNotIn('publication_rejections',again['data']);self.assertNotIn('plan_feedback',again['data'])

    def test_transport_failure_cannot_readopt_a_rejected_report_but_keeps_draft_and_new_valid_receipts(self):
        from pipeline.fullsend import adopt
        from pipeline.spec_backlog import previous_report
        plan=self.planned();task=self.store.task(self.name)
        report={'plan':digest(plan),'approved':True,'findings':['Omitted containment requirement'],
                'checks':[{'id':k,'detail':'Inspected source.'} for k in task['data']['sections']]}
        work=self.run_stage('plan_review',report,expected_state='retry');inputs=json.loads(work['input'])
        old=self.store.directory/'attempts'/work['id']/str(work['token']);(old/'bundle').mkdir(exist_ok=True)
        job={'stage':'plan_review','task':self.name,'input':inputs,'branch':'private'}
        (old/'bundle/job.json').write_text(json.dumps(job))
        receipt={'valid':True,'source_valid':True,'agent_exit':0,'published':True,'head':self.base,'planning':report}
        (old/'outcome.json').write_text(json.dumps(receipt))
        self.store.db.execute("UPDATE work SET result=?,retry_at=0 WHERE id=?",(json.dumps({'category':'gateway_transport','message':'collection failed'}),work['id']))
        claim=self.store.claim('plan_review','retry',eligible=lambda w:eligible(self.store,w));execution=Execution(self.store,claim)
        self.assertIsNone(adopt(execution,task))
        draft=previous_report(execution);self.assertEqual(draft['draft'],report)
        self.assertIn('unresolved findings',draft['error'])
        # Recovery of a later, different, valid report is still permitted.
        (execution.directory/'bundle').mkdir();(execution.directory/'bundle/job.json').write_text(json.dumps(job))
        corrected=receipt|{'planning':report|{'findings':[]}}
        (execution.directory/'outcome.json').write_text(json.dumps(corrected))
        self.assertEqual(adopt(execution,task)['planning'],corrected['planning'])

    def test_third_backlog_semantic_rejection_requires_attention_instead_of_unbounded_replanning(self):
        self.reviewed();self.run_stage('backlog');task=self.store.task(self.name)
        self.store.put_task(self.name,task['generation'],task['body'],task['data']|{'publication_rejections':2})
        name,work=self.reject_backlog();self.assertEqual(work['state'],'attention')
        self.assertEqual(json.loads(work['result'])['category'],'spec_plan')
        self.assertIsNone(self.store.task(name)['data'].get('backlog_rejected'))
        self.assertEqual(self.store.task(self.name)['data']['status'],'assigned')
        self.assertEqual(discover(self.store,'implement',self.base),[])

    def test_rejected_revision_retains_one_bounded_draft_without_treating_it_as_current_evidence(self):
        from pipeline.spec_backlog import planning_context,revision_draft
        from pipeline.stages import prompt
        from types import SimpleNamespace
        plan=self.planned();task=self.store.task(self.name);audit=task['data']['spec_audit']
        self.run_stage('plan_review',{'plan':digest(plan),'approved':False,'findings':['Nested containment is omitted'],
            'checks':[{'id':i,'detail':'Rechecked source against canonical requirements.'} for i in task['data']['sections']]})
        current=self.store.task(self.name);context=planning_context(current,self.store)
        self.assertEqual(context['revision_draft'],{'audit':{k:audit[k] for k in ('base','requirements','sections')},'plan':plan})
        self.assertIsNone(context['audit']);self.assertIsNone(context['plan'])
        self.assertIn('Nested containment',json.dumps(context['review_feedback']))
        claim_id=discover(self.store,'spec_audit',self.base)[0]
        claim=self.store.claim('spec_audit','test',eligible=lambda w:eligible(self.store,w));execution=Execution(self.store,claim)
        with patch.object(Execution,'command',return_value=SimpleNamespace(stdout='task contract')):
            text=prompt(execution,current)
        self.assertIn('revision_draft_available',text)
        self.assertNotIn('Production read omits tenant filtering',text)
        self.assertIsNone(revision_draft({'spec_audit':{'base':self.base,'requirements':task['generation'],'sections':[{'detail':'x'*131073}]}}))
        # A new canonical epoch must not carry this previous-contract draft.
        (self.repo/'specs/system/example.md').write_text('# Example\nNew requirements.\n')
        inventory(self.store,self.repo,self.base)
        self.assertIsNone(planning_context(self.store.task(self.name),self.store).get('revision_draft'))

    def test_plan_only_review_correction_retains_the_audit_and_schedules_decomposition(self):
        plan=self.planned();before=self.store.task(self.name)['data'];audit=before['spec_audit']
        finding={'id':audit['gaps'][0]['id']+':missing-replay-check','detail':'Replay serialization needs an acceptance probe'}
        self.run_stage('plan_review',{'plan':digest(plan),'approved':False,'revision_target':'plan','audit_approved':True,
            'findings':[finding],'checks':[{'id':i,'detail':'Audit is sound; task criteria omit the replay path.'} for i in before['sections']]})
        after=self.store.task(self.name)['data'];self.assertEqual(after['spec_audit'],audit)
        self.assertEqual(after['audit_cycle'],before['audit_cycle']);self.assertEqual(after['status'],'audited')
        self.assertEqual(discover(self.store,'spec_audit',self.base),[])
        self.assertEqual(len(discover(self.store,'decompose',self.base)),1)
        self.assertEqual(after['revision_draft']['plan'],plan)

    def test_distinct_review_improvements_continue_past_three_but_repeated_issues_stop(self):
        from pipeline.spec_backlog import review_revision
        plan=self.planned();data=self.store.task(self.name)['data'];identity=data['spec_audit']['gaps'][0]['id']
        report={'revision_target':'plan','audit_approved':True}
        for i in range(4):
            result=review_revision(data,report|{'findings':[{'id':identity+':issue-'+str(i),'detail':'New missing path'}]})
            data=data|result|{'decomposition':plan}
        self.assertEqual(data['plan_revision'],4);self.assertEqual(data['review_budget']['revisions'],4)
        repeated=report|{'findings':[{'id':identity+':issue-3','detail':'Same unresolved issue, different wording'}]}
        data=data|review_revision(data,repeated)|{'decomposition':plan}
        with self.assertRaisesRegex(Attention,'repeated unresolved'):review_revision(data,repeated)
        exhausted=data|{'review_budget':data['review_budget']|{'revisions':12}}
        with self.assertRaisesRegex(Attention,'twelve revisions'):review_revision(exhausted,report|{'findings':[{'id':identity+':new-issue','detail':'New'}]})

    def test_plan_only_revision_cannot_waive_audit_validation_or_forge_finding_ids(self):
        from pipeline.spec_backlog import review_revision
        self.planned();data=self.store.task(self.name)['data']
        with self.assertRaisesRegex(ValueError,'explicit confirmation'):
            review_revision(data,{'revision_target':'plan','findings':['Missing behavior']})
        with self.assertRaisesRegex(ValueError,'known heading'):
            review_revision(data,{'findings':[{'id':'0'*24+':invented','detail':'Unknown heading'}]})

    def test_reauditing_identical_code_cannot_resubmit_unchanged_rejected_task_contracts(self):
        plan=self.planned();task=self.store.task(self.name)
        self.run_stage('plan_review',{'plan':digest(plan),'approved':False,'findings':['Missing nested query path'],
            'checks':[{'id':i,'detail':'Plan is incomplete.'} for i in task['data']['sections']]})
        revised_audit=self.audit();revised_audit['sections'][1]['detail']+=' Reinspected this same code.'
        self.run_stage('spec_audit',revised_audit);new=self.store.task(self.name)['data']['spec_audit']
        unchanged=copy.deepcopy(plan);unchanged['audit']=digest(new)
        work=self.run_stage('decompose',unchanged,expected_state='retry')
        self.assertIn('Unchanged rejected task contracts',json.loads(work['result'])['message'])

    def test_planning_prompt_references_one_complete_machine_context_without_duplicate_reports(self):
        from pipeline.stages import prompt
        from pipeline.spec_backlog import planning_context
        from types import SimpleNamespace
        plan=self.planned();task=self.store.task(self.name)
        discover(self.store,'plan_review',self.base)
        claim=self.store.claim('plan_review','test',eligible=lambda w:eligible(self.store,w));execution=Execution(self.store,claim)
        with patch.object(Execution,'command',return_value=SimpleNamespace(stdout='task contract')):
            text=prompt(execution,task)
        self.assertNotIn('Production read omits tenant filtering',text)
        self.assertNotIn(plan['tasks'][0]['required_behavior'],text)
        summary=json.loads(text.split('Planning context:\n',1)[1].split('\n\n',1)[0])
        self.assertTrue(summary['audit_available']);self.assertTrue(summary['plan_available'])
        self.assertEqual(summary['heading_count'],len(task['data']['sections']))
        self.assertEqual(summary['machine_context'],'/tmp/stage/planning-context.json')
        context=planning_context(task,self.store)
        self.assertEqual(context['audit'],task['data']['spec_audit']);self.assertEqual(context['plan'],plan)
        correction=task|{'data':task['data']|{'decomposition':None,'revision_draft':{'audit':context['audit'],'plan':plan}}}
        compact=planning_context(correction,self.store)
        self.assertEqual(compact['audit'],context['audit']);self.assertEqual(compact['revision_draft'],{'plan':plan})

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

    def test_spec_cockpit_reflects_operator_priority_instead_of_a_default_label(self):
        from pipeline.priority import set_task
        set_task(self.store,self.name,'urgent')
        item=summary(self.store)['items'][0]
        self.assertEqual(item['priority']['class'],'urgent');self.assertEqual(item['priority']['source'],'operator')

    def test_malformed_report_retry_gets_one_trusted_same_input_draft_not_stale_history(self):
        from pipeline.spec_backlog import previous_report
        work=discover(self.store,'spec_audit',self.base)[0]
        claim=self.store.claim('spec_audit','one',eligible=lambda w:eligible(self.store,w))
        execution=Execution(self.store,claim);bundle=execution.directory/'bundle';bundle.mkdir()
        job={'stage':claim['stage'],'task':claim['task'],'input':claim['input']}
        (bundle/'job.json').write_text(json.dumps(job))
        (execution.directory/'outcome.json').write_text(json.dumps({'valid':True,'source_valid':True,'agent_exit':0,'planning':self.audit()}))
        self.store.retry(work,claim['token'],{'category':'spec_report','message':'One quote needs correction','fresh_model':True},delay=0)
        retry=self.store.claim('spec_audit','two',eligible=lambda w:eligible(self.store,w));second=Execution(self.store,retry)
        self.assertEqual(previous_report(second)['draft'],self.audit())
        job['input']['base']='different-source';(bundle/'job.json').write_text(json.dumps(job))
        self.assertIsNone(previous_report(second))

    def test_rationale_needs_literal_spec_context_but_not_fabricated_production_evidence(self):
        report=self.audit();report['sections'][0]['evidence']=[]
        validated=validate_audit(report,self.store.task(self.name),self.base,self.repo)
        self.assertEqual(len(validated['gaps']),1)
        report['sections'][1]['evidence']=[]
        with self.assertRaises(ValueError):validate_audit(report,self.store.task(self.name),self.base,self.repo)

    def test_main_advances_preserve_audit_retry_identity_and_base_until_cycle_finishes(self):
        first=discover(self.store,'spec_audit',self.base)
        self.assertEqual(discover(self.store,'spec_audit','new-unrelated-main'),first)
        claim=self.store.claim('spec_audit','auditor',eligible=lambda w:eligible(self.store,w))
        self.store.retry(first[0],claim['token'],{'category':'spec_report','fresh_model':True},delay=0)
        self.assertEqual(discover(self.store,'spec_audit','another-main'),first)
        row=self.store.db.execute('SELECT input,state FROM work WHERE id=?',(first[0],)).fetchone()
        self.assertEqual(json.loads(row['input'])['base'],self.base);self.assertEqual(row['state'],'retry')

    def test_sandbox_citation_preflight_rejects_forged_quotes_and_empty_envelopes(self):
        script=Path(__file__).with_name('pipeline-evidence.py');report=self.root/'planning.json'
        def check(value):
            report.write_text(json.dumps(value))
            return subprocess.run(['python3',str(script),str(report),'--root',str(self.repo)],capture_output=True,text=True)
        self.assertEqual(check(self.audit()).returncode,0)
        forged=self.audit();forged['sections'][1]['evidence'][0]['quote']='invented source'
        self.assertEqual(check(forged).returncode,1)
        self.assertEqual(check({'planning':self.audit()}).returncode,2)

    def test_workflow_behavior_evidence_passes_both_validators_and_tracks_configuration_changes(self):
        from pipeline.spec_backlog import current_evidence
        (self.repo/'specs/system/example.md').write_text(
            '# Example\nContext for this specification.\n\n## CI\nGitHub Actions must execute the mechanical guard.\n')
        workflow=self.repo/'.github/workflows/ci.yml';workflow.parent.mkdir(parents=True)
        workflow.write_text('name: CI\non: [push]\njobs:\n  checks:\n    runs-on: ubuntu-24.04\n'
                            '    steps:\n      - run: bash scripts/check-arch.sh\n')
        self.git('add','.');self.git('commit','-qm','CI contract and implementation')
        self.base=self.git('rev-parse','HEAD');inventory(self.store,self.repo,self.base)
        task=self.store.task(self.name);rows=[]
        for identity,heading in task['data']['sections'].items():
            normative=heading[-1]=='CI'
            rows.append({'id':identity,'decision':'satisfied' if normative else 'context',
                'requirement_quote':'GitHub Actions must execute the mechanical guard.' if normative else 'Context for this specification.',
                'detail':'The workflow executes the guard on push.' if normative else 'Introductory rationale',
                'evidence':[{'file':'.github/workflows/ci.yml','quote':'- run: bash scripts/check-arch.sh',
                             'detail':'The Actions job invokes the actual mechanical guard.'}] if normative else []})
        report={'base':self.base,'requirements':task['generation'],'sections':rows}
        validated=validate_audit(report,task,self.base,self.repo)
        self.assertEqual([r['file'] for r in validated['source_evidence']],['.github/workflows/ci.yml'])
        path=self.root/'ci-audit.json';path.write_text(json.dumps(report))
        command=[sys.executable,str(Path(__file__).with_name('pipeline-evidence.py')),str(path),'--root',str(self.repo)]
        self.assertEqual(subprocess.run(command,capture_output=True).returncode,0)
        self.assertTrue(current_evidence(self.repo,validated['source_evidence']))
        workflow.write_text('name: CI\non: [push]\njobs: {}\n')
        self.assertFalse(current_evidence(self.repo,validated['source_evidence']))
        with self.assertRaises(ValueError):validate_audit(report,task,self.base,self.repo)
        self.assertEqual(subprocess.run(command,capture_output=True).returncode,1)

    def test_public_build_configuration_is_evidence_but_private_paths_remain_rejected(self):
        module=helper('pipeline-evidence')
        for name in ('Cargo.toml','Cargo.lock','Dockerfile','flake.nix','.cargo/config.toml',
                     'docker/gyre-agent/entrypoint.sh','docker/dev-worker/policy.yaml'):
            with self.subTest(path=name):
                path=self.repo/name;path.parent.mkdir(parents=True,exist_ok=True);path.write_text('public configuration\n')
                self.assertTrue(module.production_source(name))
                self.assertEqual(module.evidence(self.repo,{'file':name,'quote':'public configuration','detail':'Inspected configuration'})['file'],name)
        for name in ('.env','credentials.json','.aws/credentials','.github/secrets.yml',
                     '.github/workflows/api-token.env','docker/dev-worker/auth.json','docker/dev-worker/private.key',
                     'docker/dev-worker/.ssh/config'):
            with self.subTest(path=name):
                path=self.repo/name;path.parent.mkdir(parents=True,exist_ok=True);path.write_text('private fixture\n')
                self.assertFalse(module.production_source(name))
                with self.assertRaises(ValueError):module.evidence(self.repo,{'file':name,'quote':'private fixture','detail':'Must not admit this file'})

    def test_full_inventory_plan_index_stays_small_and_preserves_full_details_on_demand(self):
        from pipeline.spec_backlog import planning_context
        task=self.store.task(self.name)
        plan={'audit':'source','tasks':[{'key':'work-'+str(i),'title':'Concrete deliverable '+str(i),
                'required_behavior':'real requirement '*600,'acceptance':['meaningful acceptance '*50],
                'negative_probe':'real negative probe '*100,'covers':[format(i,'024x')],'dependencies':[]} for i in range(8)]}
        for i in range(50):
            name='spec-'+format(i,'024x');data=task['data']|{'document':'specs/system/other-'+str(i)+'.md',
                'status':'planned','decomposition':plan}
            self.store.put_task(name,str(i),task['body'],data)
        documents={};context=planning_context(task,self.store,documents)
        self.assertEqual(len(context['other_plans']),50);self.assertEqual(len(documents),50)
        self.assertLess(len(json.dumps(context)),100000)
        self.assertGreater(sum(len(json.dumps(v)) for v in documents.values()),2000000)
        for item in context['other_plans']:
            artifact=documents[Path(item['path']).name]
            self.assertEqual(digest(artifact['plan']),item['plan_id']);self.assertEqual(artifact['plan'],plan)
        finished=self.store.task('spec-'+format(0,'024x'))
        self.store.put_task('task-999','done','task',{'delivered':{'sha':'merged'}})
        self.store.put_task(finished['name'],finished['generation'],finished['body'],finished['data']|{'status':'assigned','backlog_tasks':['task-999']})
        retired=self.store.task('spec-'+format(1,'024x'))
        self.store.put_task(retired['name'],retired['generation'],retired['body'],retired['data']|{'spec_retired':True})
        self.assertEqual(len(planning_context(task,self.store)['other_plans']),48)

    def test_real_fullsend_bundle_supplies_machine_context_and_shared_citation_checker(self):
        import os,sys
        from pipeline.fullsend import prepare
        own=self.store.task(self.name);other='spec-'+'f'*24
        plan={'audit':'other-source','tasks':[{'key':'related','required_behavior':'Complete duplicate-work evidence'}]}
        self.store.put_task(other,'other',own['body'],own['data']|{'document':'specs/system/other.md','status':'planned','decomposition':plan})
        discover(self.store,'spec_audit',self.base)
        claim=self.store.claim('spec_audit','planner',eligible=lambda w:eligible(self.store,w));execution=Execution(self.store,claim)
        with patch.dict(os.environ,{'OPENSHELL':sys.executable}):
            cfg,repo,_=prepare(execution,self.store.task(self.name),'Read the immutable planning assignment.')
        bundle=execution.directory/'bundle'
        context=json.loads((bundle/'planning-context.json').read_text())
        self.assertEqual(context['assignment'],claim['input']);self.assertEqual(context['requirements'],claim['generation'])
        self.assertEqual(context['sections'],self.store.task(self.name)['data']['sections'])
        self.assertTrue((bundle/'pipeline-evidence.py').is_file())
        item=context['other_plans'][0];artifact=bundle/'other-plans'/Path(item['path']).name
        self.assertEqual(json.loads(artifact.read_text())['plan'],plan)
        harness=json.loads((cfg/'harness/spec_audit.yaml').read_text())
        self.assertIn(item['path'],[f['dest'] for f in harness['host_files']])
