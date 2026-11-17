"""Real Git and ledger proofs for concurrent verification and safe reuse."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch
from pipeline.store import Store
from pipeline.execution import Execution
from pipeline.integration import same_merge_tree,resolve_upstream_assessments
from pipeline.verification import COMMANDS,policy
from pipeline.build_cache import select,digest
from pipeline.catalog import discover
from pipeline.ci import completed_refresh

class ConcurrentVerificationTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        self.root=Path(self.tmp.name);self.repo=self.root/'repo';self.repo.mkdir()
        self.git('init','-q');self.git('config','user.name','Test');self.git('config','user.email','test@example.com');self.git('config','core.hooksPath','/dev/null')
        (self.repo/'code').write_text('base\n');self.commit('base');self.base=self.git('rev-parse','HEAD')
        (self.repo/'code').write_text('candidate\n');self.commit('candidate');self.head=self.git('rev-parse','HEAD')
        self.git('branch','candidate',self.head);self.git('checkout','-q','--detach',self.base)
        self.git('commit','--allow-empty','-qm','metadata-only upstream advance');self.newbase=self.git('rev-parse','HEAD');self.git('remote','add','origin',str(self.repo))
        self.s=Store(self.root/'state');self.addCleanup(self.s.close)
        shutil.copytree(self.repo,self.s.directory/'source')
        self.s.put_task('task-001','g','body',{'triaged':'g','dependencies':[],
            'candidate':self.head,'candidate_base':self.base,'review':{'candidate':self.head,'approved':True}})
        self.work=self.s.enqueue('verify','task-001','g',{'candidate':self.head,'base':self.base})
        c=self.s.claim('verify','original-verifier')
        self.receipt={'candidate':self.head,'head':self.head,'base':self.base,'tree':self.git('rev-parse',self.head+'^{tree}'),
            'tested_head':self.head,'passed':True,'engine':'gateway','policy':policy(self.s),
            'checks':[{'command':command,'exit_code':0} for command in COMMANDS]}
        self.s.finish(self.work,c['token'],self.receipt,{'verified':self.receipt})
        self.publish=self.s.enqueue('publish','task-001','g',{'candidate':self.head,'verified':self.receipt})
        self.e=Execution(self.s,self.s.claim('publish','publisher',ident=self.publish))

    def git(self,*args):return subprocess.check_output(['git',*args],cwd=self.repo,text=True,stderr=subprocess.PIPE).strip()
    def commit(self,title):self.git('add','.');self.git('commit','-qm',title)

    def test_identical_resulting_tree_reuses_original_head_and_receipt_after_main_advances(self):
        before=dict(self.receipt)
        self.assertTrue(same_merge_tree(self.e,self.repo,self.newbase,self.receipt))
        self.assertEqual(self.receipt,before)
        self.assertEqual(self.s.task('task-001')['data']['verified']['base'],self.base)
        self.assertEqual(self.git('rev-parse','HEAD'),self.newbase)

    def test_changed_tree_conflict_and_changed_verifier_never_reuse_a_pass(self):
        (self.repo/'other-code').write_text('real upstream change');self.commit('code');changed=self.git('rev-parse','HEAD')
        self.assertFalse(same_merge_tree(self.e,self.repo,changed,self.receipt))
        (self.repo/'code').write_text('conflicting upstream behavior');self.commit('conflict')
        self.assertFalse(same_merge_tree(self.e,self.repo,self.git('rev-parse','HEAD'),self.receipt))
        self.git('checkout','-q','--detach',self.newbase)
        (self.repo/'transient').write_text('untested source-history change');self.commit('edit')
        (self.repo/'transient').unlink();self.commit('revert to identical file tree')
        self.assertFalse(same_merge_tree(self.e,self.repo,self.git('rev-parse','HEAD'),self.receipt))
        with patch('pipeline.verification.policy',return_value='new trusted verifier'):
            self.assertFalse(same_merge_tree(self.e,self.repo,self.newbase,self.receipt))
        self.assertFalse(same_merge_tree(self.e,self.repo,self.newbase,self.receipt|{'policy':None}))

    def test_main_advance_keeps_passed_receipt_but_changed_candidate_requires_verification(self):
        self.assertEqual(discover(self.s,'verify',self.newbase),[])
        from pipeline.eligibility import eligible
        stale=self.s.enqueue('verify','task-001','g',{'candidate':self.head,'base':self.newbase})
        self.assertFalse(eligible(self.s,dict(self.s.db.execute('SELECT * FROM work WHERE id=?',(stale,)).fetchone())))
        self.assertTrue(discover(self.s,'publish',self.newbase))
        task=self.s.task('task-001');self.s.put_task('task-001','g','body',task['data']|{'candidate':self.newbase,'review':{'candidate':self.newbase,'approved':True}})
        self.assertTrue(discover(self.s,'verify',self.newbase))

    def test_pending_ci_has_one_observer_and_no_repeating_publisher_or_verifier(self):
        self.s.finish(self.publish,self.e.claim['token'],{})
        task=self.s.task('task-001');data=task['data']|{'pr':'https://github.com/example/repo/pull/1','published_head':self.head,'published_verified':self.receipt,
            'ci':{'head':self.head,'status':'pending'}};self.s.put_task('task-001','g','body',data)
        self.assertEqual(discover(self.s,'publish',self.newbase),[])
        self.assertEqual(discover(self.s,'verify',self.newbase),[])
        self.assertEqual(len(discover(self.s,'observe',self.newbase)),1)
        data['ci']={'head':self.head,'status':'passed','checks':'green'};self.s.put_task('task-001','g','body',data)
        self.assertEqual(len(discover(self.s,'publish',self.newbase)),1)

    def cache_context(self):
        self.s.finish(self.publish,self.e.claim['token'],{})
        folder=self.s.directory/'build-cache';folder.mkdir()
        key='a'*64;stem=folder/(key+'-'+self.head);archive=stem.with_suffix('.tar.gz');archive.write_bytes(b'compiled dependencies')
        value={'source':self.head,'bytes':archive.stat().st_size,'sha256':digest(archive),'key':key}
        stem.with_suffix('.json').write_text(json.dumps(value))
        self.receipt['cache']={'key':key,'export':{'sha256':value['sha256'],'bytes':value['bytes']}}
        self.s.db.execute('UPDATE work SET result=? WHERE id=?',(json.dumps(self.receipt),self.work))
        retry=self.s.enqueue('verify','task-001','g',{'candidate':self.head,'base':self.newbase})
        return Execution(self.s,self.s.claim('verify','refresh',ident=retry)),key

    def test_unmerged_cache_reused_only_with_completed_full_pass_for_same_approved_candidate(self):
        execution,key=self.cache_context()
        found=select(execution,key,self.newbase)
        self.assertIsNotNone(found);self.assertEqual(found[1]['trust'],'same-approved-candidate')
        data=self.s.task('task-001')['data'];self.s.put_task('task-001','g','body',data|{'review':None})
        self.assertIsNone(select(execution,key,self.newbase))
        self.s.put_task('task-001','g','body',data|{'candidate':self.newbase,'review':{'candidate':self.newbase,'approved':True}})
        execution.claim['input']['candidate']=self.newbase
        self.assertIsNone(select(execution,key,self.newbase))

    def test_partial_wrong_generation_and_other_task_cannot_use_unmerged_cache(self):
        execution,key=self.cache_context()
        receipt=self.receipt|{'checks':self.receipt['checks'][:1]}
        self.s.db.execute('UPDATE work SET result=? WHERE id=?',(json.dumps(receipt),self.work))
        self.assertIsNone(select(execution,key,self.newbase))
        self.s.db.execute('UPDATE work SET result=? WHERE id=?',(json.dumps(self.receipt),self.work))
        self.s.put_task('task-002','g','body',{'candidate':self.head,'review':{'candidate':self.head,'approved':True}})
        execution.claim['task']='task-002';self.assertIsNone(select(execution,key,self.newbase))
        execution.claim['task']='task-001';execution.claim['generation']='different-contract'
        self.assertIsNone(select(execution,key,self.newbase))

    def test_already_completed_ci_refresh_does_not_discard_new_verification(self):
        task=self.s.task('task-001');task['data']['ci_refresh']={'candidate':self.head,'published_head':'old-ci-head',
            'generation':'g','failure_signature':'dns','target_base':self.base}
        observation={'status':'environment_refresh','target_base':self.newbase,'failure_signature':'dns'}
        self.assertTrue(completed_refresh(self.s,task,self.receipt|{'base':self.newbase},{'head':'old-ci-head'},observation))
        self.assertFalse(completed_refresh(self.s,task,self.receipt,{'head':'old-ci-head'},observation))
        self.assertFalse(completed_refresh(self.s,task,self.receipt|{'base':self.newbase},{'head':'old-ci-head'},observation|{'status':'candidate_failed'}))

    def test_upstream_assessment_backfill_requires_real_completed_verification_and_no_active_work(self):
        self.s.finish(self.publish,self.e.claim['token'],{})
        self.git('merge','--no-ff','--no-edit','candidate');main=self.git('rev-parse','HEAD')
        self.s.db.execute("UPDATE tasks SET data=? WHERE name='task-001'",(json.dumps({'candidate':self.head,'requirements':{'fingerprint':'fp'}}),))
        work=self.s.enqueue('assess','task-001','g',{'candidate':self.head,'base':self.base,'requirements':'fp','required_evidence':['task']})
        claim=self.s.claim('assess','assessor');path=self.s.directory/'attempts'/work/str(claim['token']);path.mkdir(parents=True)
        checked={k:v for k,v in self.receipt.items() if k!='policy'}
        checked['publication']={'head':self.head,'tree':self.receipt['tree'],'artifact_commit':None}
        assessment={'candidate':self.head,'requirements':'fp','generation':'g','decision':'still_satisfied','artifact':str(path/'outcome.json'),'verification':checked,'evidence':[{'requirement':'task','file':'code','detail':'real source satisfies the contract'}],'gaps':[]}
        data=self.s.task('task-001')['data']|{'assessment':assessment};self.s.put_task('task-001','g','body',data)
        self.assertEqual(resolve_upstream_assessments(self.s,self.repo,main),[])
        (path/'remote-verification.json').write_text(json.dumps(checked));self.s.finish(work,claim['token'],{'valid':True,'source_valid':True,'agent_exit':0,'stream_exit':0,'assessment':{k:assessment[k] for k in ('candidate','requirements','decision')}})
        self.s.put_task('task-001','g','body',data|{'requirements':{'fingerprint':'fp','errors':['ambiguous binding']}})
        self.assertEqual(resolve_upstream_assessments(self.s,self.repo,main),[])
        self.s.put_task('task-001','g','body',data)
        self.assertEqual(resolve_upstream_assessments(self.s,self.repo,main),['task-001'])
        self.assertEqual(self.s.task('task-001')['data']['resolved']['source'],self.head)
        self.assertIsNone(self.s.task('task-001')['data'].get('delivered'))

if __name__=='__main__':unittest.main()
