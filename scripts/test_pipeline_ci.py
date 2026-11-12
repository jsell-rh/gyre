"""CI handoff must not wait for verification or a stable main tip."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from pipeline.catalog import discover, helper
from pipeline.ci import observe, publication
from pipeline.eligibility import eligible
from pipeline.execution import Execution, Wait
from pipeline.store import Store, StaleClaim


class CIReconciliationTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        self.store=Store(self.temp.name);self.addCleanup(self.store.close)
        self.candidate='a'*40;self.head='b'*40;self.base='c'*40
        self.receipt={'candidate':self.candidate,'head':self.head,'base':self.base,'tree':'d'*40,
                      'passed':True,'checks':[],'branch':'private'}
        self.store.put_task('task-001','g','body',{'dependencies':[],'triaged':'g',
            'candidate':self.candidate,'candidate_base':self.base,
            'review':{'candidate':self.candidate,'approved':True},
            'published_head':self.head,'published_verified':self.receipt,
            'pr':'https://github.com/example/repo/pull/1',
            'ci':{'head':self.head,'checks':'green','status':'policy_pending'}})
        self.checks=[{'status':'COMPLETED','conclusion':'FAILURE','name':'e2e',
                      'detailsUrl':'https://github.com/example/repo/actions/runs/10/job/100'},
                     {'status':'IN_PROGRESS','conclusion':None,'name':'test'}]
        self.pr={'headRefOid':self.head,'headRefName':'pipeline/delivery/task-001','state':'OPEN','mergeStateStatus':'BEHIND','statusCheckRollup':self.checks}
        self.calls=[];self.change_on_confirmation=None

    def command(self,*args,**kwargs):
        self.calls.append(args)
        if args[:3]==('gh','pr','view'):
            pr=dict(self.pr)
            if args[-1]=='headRefOid,statusCheckRollup,state,baseRefOid,mergeStateStatus,mergeable' and self.change_on_confirmation:
                pr.update(self.change_on_confirmation)
            value=pr
        elif args[:2]==('gh','api'):
            value={'workflow_runs':[]}
        elif '--log-failed' in args:
            return subprocess.CompletedProcess(args,0,'tests/e2e/app.spec.js:4:1 › feature defect\n','')
        else:value={'workflowDatabaseId':5,'name':'E2E Tests'}
        return subprocess.CompletedProcess(args,0,json.dumps(value),'')

    def observer(self):
        work=discover(self.store,'observe','new-main')[0]
        claim=self.store.claim('observe','observer',ident=work,eligible=lambda w:eligible(self.store,w))
        self.assertIsNotNone(claim)
        return Execution(self.store,claim)

    def test_failure_handoff_while_verifier_runs_on_a_new_base_and_pool_is_full(self):
        self.store.set_setting('slots',1)
        work=self.store.enqueue('verify','task-001','g',{'candidate':self.candidate,'base':'new-main'})
        verifier=self.store.claim('verify','slow-verifier',capacity=1)
        self.assertIsNotNone(verifier)
        observer=self.observer()
        self.assertEqual(self.store.compute_used(),1)
        with patch.object(observer,'command',side_effect=self.command):
            result,updates,findings=observe(observer,self.store.task('task-001'))
        self.assertEqual(result['status'],'candidate_failed')
        self.store.finish(observer.claim['id'],observer.claim['token'],result,updates,findings)
        data=self.store.task('task-001')['data']
        self.assertEqual(data['repair']['category'],'ci')
        self.assertEqual(data['candidate'],self.head)
        self.assertEqual(data['candidate_base'],self.base)
        self.assertIsNone(self.store.integration_lane())
        self.assertIn('feature defect',Path(data['repair']['candidate_log']).read_text())
        with self.assertRaises(StaleClaim):
            self.store.finish(work,verifier['token'],{'passed':True},{'verified':{'wrong':'late result'}})
        self.assertEqual(len(discover(self.store,'implement','even-newer-main')),1)

    def test_conflict_handoff_fences_verifier_and_requests_one_worker_on_retained_pr_head(self):
        current_base='e'*40
        self.pr.update(mergeStateStatus='DIRTY',mergeable='CONFLICTING',baseRefOid=current_base)
        verifier_id=self.store.enqueue('verify','task-001','g',{'candidate':self.candidate,'base':'new-main'})
        verifier=self.store.claim('verify','concurrent-verifier')
        observer=self.observer()
        with patch.object(observer,'command',side_effect=self.command):
            result,updates,findings=observe(observer,self.store.task('task-001'))
        self.assertEqual(result['status'],'merge_conflict')
        self.store.finish(observer.claim['id'],observer.claim['token'],result,updates,findings)
        data=self.store.task('task-001')['data']
        self.assertEqual(data['repair']['category'],'rebase')
        self.assertEqual(data['repair']['base'],current_base)
        self.assertEqual(data['candidate'],self.head)
        self.assertEqual(data['candidate_base'],self.base)
        self.assertEqual(data['pr'],'https://github.com/example/repo/pull/1')
        self.assertIsNone(data['review']);self.assertIsNone(data['verified'])
        self.assertIsNone(self.store.integration_lane())
        with self.assertRaises(StaleClaim):self.store.finish(verifier_id,verifier['token'],{})
        first=discover(self.store,'implement',current_base)
        self.assertEqual(len(first),1)
        self.assertEqual(discover(self.store,'implement',current_base),first)
        self.assertEqual(len(self.store.findings('task-001')),1)
        claim=self.store.claim('implement','conflict-worker',ident=first[0],eligible=lambda w:eligible(self.store,w))
        self.assertIsNotNone(claim)
        self.assertEqual(claim['input']['candidate'],self.head)
        # Subsequent polls only watch labels; they cannot emit another repair.
        watch=self.observer()
        with patch.object(watch,'command',side_effect=self.command),self.assertRaises(Wait):
            observe(watch,self.store.task('task-001'))
        self.assertEqual(len(self.store.findings('task-001')),1)

    def test_green_generated_conflict_refreshes_integration_without_a_model_finding(self):
        self.pr.update(mergeStateStatus='DIRTY',mergeable='CONFLICTING',baseRefOid='e'*40,
                       statusCheckRollup=[dict(self.checks[0],conclusion='SUCCESS')])
        observer=self.observer()
        with patch.object(Execution,'command',side_effect=self.command),patch('pipeline.ci.generated_conflict',return_value=(True,['web/dist/index.html'])) as probe:
            with self.assertRaises(Wait):observe(observer,self.store.task('task-001'))
            self.assertEqual(probe.call_count,1)
            self.store.retry(observer.claim['id'],observer.claim['token'],{},delay=0)
            observer=self.observer()
            with self.assertRaises(Wait):observe(observer,self.store.task('task-001'))
            self.assertEqual(probe.call_count,1,'same head/base classification should be reused')
        data=self.store.task('task-001')['data']
        self.assertEqual(data['ci']['status'],'artifact_refresh')
        self.assertFalse(data.get('repair'))
        self.assertEqual(data['candidate'],self.candidate)
        self.assertTrue(data['review']['approved'])
        self.assertEqual(self.store.findings('task-001'),[])
        self.assertEqual(discover(self.store,'implement','new-main'),[])
        self.assertEqual(len(discover(self.store,'verify','new-main')),1)

    def test_stale_or_recovered_conflict_is_not_handed_to_implementation(self):
        self.pr.update(mergeStateStatus='DIRTY',mergeable='CONFLICTING',baseRefOid='e'*40)
        for changed in ({'headRefOid':'f'*40},{'baseRefOid':'d'*40},
                        {'mergeStateStatus':'CLEAN','mergeable':'MERGEABLE'}, {'state':'MERGED'}):
            with self.subTest(change=changed):
                self.change_on_confirmation=changed
                observer=self.observer()
                with patch.object(observer,'command',side_effect=self.command),self.assertRaises(Wait):
                    observe(observer,self.store.task('task-001'))
                self.store.retry(observer.claim['id'],observer.claim['token'],{},delay=0)
                self.assertFalse(self.store.task('task-001')['data'].get('repair'))

    def test_pending_checks_do_not_request_implementation_or_repeat_verification(self):
        self.pr['statusCheckRollup']=self.checks[1:]
        observer=self.observer()
        with patch.object(observer,'command',side_effect=self.command),self.assertRaises(Wait):
            observe(observer,self.store.task('task-001'))
        data=self.store.task('task-001')['data']
        self.assertFalse(data.get('repair'))
        self.assertEqual(data['ci']['status'],'pending')
        self.assertEqual(discover(self.store,'verify','new-main'),[])

    def test_merged_pr_fences_reverification_and_only_confirms_original_receipt(self):
        self.pr.update(state='MERGED',mergeCommit={'oid':'merged-sha'})
        verifier=self.store.enqueue('verify','task-001','g',{'candidate':self.candidate,'base':'merged-sha'})
        claim=self.store.claim('verify','redundant-verifier')
        observer=self.observer()
        with patch.object(observer,'command',side_effect=self.command):
            result,updates,findings=observe(observer,self.store.task('task-001'))
        self.assertEqual(updates['verified'],self.receipt)
        self.store.finish(observer.claim['id'],observer.claim['token'],result,updates,findings)
        self.assertEqual(self.store.db.execute('SELECT state FROM work WHERE id=?',(verifier,)).fetchone()[0],'obsolete')
        self.assertEqual(discover(self.store,'verify','merged-sha'),[])
        publisher=discover(self.store,'publish','merged-sha')
        self.assertEqual(len(publisher),1)
        with self.assertRaises(StaleClaim):self.store.finish(verifier,claim['token'],{})

    def test_pending_observations_preserve_wait_age_for_the_same_head(self):
        self.pr['statusCheckRollup']=self.checks[1:]
        observer=self.observer()
        first=None
        for _ in range(2):
            with patch.object(observer,'command',side_effect=self.command),self.assertRaises(Wait):
                observe(observer,self.store.task('task-001'))
            current=self.store.task('task-001')['data']['ci']
            if first is None:first=current['waiting_since']
            self.assertEqual(current['waiting_since'],first)
        self.pr['statusCheckRollup']=[dict(self.checks[0],conclusion='SUCCESS')]
        self.pr['mergeStateStatus']='CLEAN'
        with patch.object(observer,'command',side_effect=self.command),self.assertRaises(Wait):
            observe(observer,self.store.task('task-001'))
        self.assertNotIn('waiting_since',self.store.task('task-001')['data']['ci'])

    def test_upstream_failure_keeps_original_reviewed_source_for_reverification(self):
        from pipeline.ci import failure
        observer=self.observer()
        log=observer.directory/'baseline.log';log.write_text('upstream defect')
        observed={'status':'baseline_failed','base':self.base,'environment':'github-fixture',
                  'baseline_log':str(log),'log':str(log)}
        with patch('pipeline.stages.baseline_repair',return_value='task-999'):
            _,updates,_=failure(observer,self.store.task('task-001'),self.receipt,observed)
        self.assertEqual(updates['repair']['source'],self.candidate)
        self.assertEqual(updates['repair']['published_head'],self.head)
        self.assertNotIn('candidate',updates)
        self.assertNotIn('review',updates)

    def test_shared_setup_failure_has_one_repair_and_resumes_both_reviewed_candidates(self):
        from pipeline.ci import failure
        source=self.store.directory/'source/specs';source.mkdir(parents=True);(source/'GOAL.md').write_text('Real fixes; never weaken checks.')
        original=self.store.task('task-001')
        self.store.put_task('task-002','g','body',dict(original['data']))
        prerequisites=[]
        for name in ('task-001','task-002'):
            task=self.store.task(name)
            work=self.store.enqueue('observe',name,'g',{'candidate':self.candidate,'url':task['data']['pr'],'published':self.receipt})
            execution=Execution(self.store,self.store.claim('observe','observer',ident=work))
            log=execution.directory/'failure.log';log.write_text('Playwright does not support chromium on ubuntu26.04-x64')
            observation={'status':'environment_failed','base':self.base,'environment':'github-fixture',
                         'failure_signature':'e'*64,'baseline_log':str(log),'log':str(log),
                         'environment_failure':{'kind':'playwright-unsupported-platform'}}
            result,updates,findings=failure(execution,task,self.receipt,observation)
            pipeline=helper('dev-pipeline')
            with patch.dict(pipeline.HANDLERS,{'observe':lambda e,t:failure(e,t,self.receipt,observation)}):
                pipeline.run_claim(self.store,execution.claim)
            self.assertEqual(self.store.db.execute('SELECT state FROM work WHERE id=?',(work,)).fetchone()[0],'succeeded')
            prerequisites.append(updates['repair']['prerequisite'])
            data=self.store.task(name)['data']
            self.assertEqual(data['candidate'],self.candidate)
            self.assertTrue(data['review']['approved'])
            self.assertNotIn('dependencies',updates)
            self.assertEqual(data['dependencies'],[])
            self.assertEqual(self.store.task(name)['generation'],'g')
        self.assertEqual(prerequisites[0],prerequisites[1])
        repair=self.store.task(prerequisites[0])
        self.assertIn('Repair shared CI environment',repair['body'])
        from pipeline.priority import task_priority
        self.assertEqual(task_priority(self.store,repair)['class'],'repair')
        implementations=discover(self.store,'implement',self.base)
        self.assertEqual([self.store.db.execute('SELECT task FROM work WHERE id=?',(w,)).fetchone()[0] for w in implementations],[repair['name']])
        fixed='f'*40
        self.store.put_task(repair['name'],repair['generation'],repair['body'],repair['data']|{'delivered':{'sha':fixed}})
        jobs=discover(self.store,'verify',fixed)
        self.assertEqual({self.store.db.execute('SELECT task FROM work WHERE id=?',(w,)).fetchone()[0] for w in jobs},{'task-001','task-002'})
        for name in ('task-001','task-002'):
            self.assertIsNone(self.store.task(name)['data']['repair'])
            self.assertTrue(self.store.task(name)['data']['review']['approved'])

    def test_environment_refresh_fences_stale_verifier_and_requires_fixed_base(self):
        from pipeline.ci import failure,verification_ready
        target='f'*40
        stale=self.store.enqueue('verify','task-001','g',{'candidate':self.candidate,'base':self.base})
        stale_claim=self.store.claim('verify','old-verifier',ident=stale)
        observer=self.observer()
        observation={'status':'environment_refresh','target_base':target,'failure_signature':'e'*64}
        result,updates,findings=failure(observer,self.store.task('task-001'),self.receipt,observation)
        self.store.finish(observer.claim['id'],observer.claim['token'],result,updates,findings)
        task=self.store.task('task-001')
        self.assertFalse(task['data'].get('repair'));self.assertTrue(task['data']['review']['approved'])
        self.assertIsNone(self.store.integration_lane())
        self.assertFalse(verification_ready(self.store,task,self.base))
        self.assertTrue(verification_ready(self.store,task,target))
        with self.assertRaises(StaleClaim):self.store.finish(stale,stale_claim['token'],{})
        self.assertEqual(discover(self.store,'implement',target),[])
        self.assertEqual(len(discover(self.store,'verify',target)),1)
        changed=dict(task['data']);changed['published_head']='9'*40;changed['published_verified']=self.receipt|{'head':'9'*40};changed['ci']={'status':'pending'}
        self.store.put_task('task-001','g','body',changed)
        self.assertFalse(verification_ready(self.store,self.store.task('task-001'),target))

    def test_environment_refresh_keeps_verifier_already_on_fixed_base(self):
        from pipeline.ci import failure
        target='f'*40
        ident=self.store.enqueue('verify','task-001','g',{'candidate':self.candidate,'base':target})
        claim=self.store.claim('verify','new-verifier',ident=ident)
        observer=self.observer()
        result,updates,findings=failure(observer,self.store.task('task-001'),self.receipt,
                                      {'status':'environment_refresh','target_base':target,'failure_signature':'e'*64})
        self.store.finish(observer.claim['id'],observer.claim['token'],result,updates,findings)
        self.store._current(ident,claim['token'])
        self.assertIsNone(self.store.integration_lane())
        self.store._current(ident,claim['token'])

    def test_shared_repair_creation_is_atomic_across_observers(self):
        import concurrent.futures,threading
        from types import SimpleNamespace
        from pipeline.stages import baseline_repair
        source=self.store.directory/'source/specs';source.mkdir(parents=True);(source/'GOAL.md').write_text('Fix shared infrastructure without weakening checks.')
        log=self.store.directory/'failure.log';log.write_text('Unsupported hosted runner')
        barrier=threading.Barrier(2)
        def create(base):
            store=Store(self.temp.name)
            try:
                barrier.wait()
                return baseline_repair(SimpleNamespace(store=store),{'status':'environment_failed','base':base,'environment':'github-fixture',
                    'failure_signature':'e'*64,'baseline_log':str(log),'environment_failure':{'kind':'playwright-unsupported-platform'}})
            finally:store.close()
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            names=list(pool.map(create,('a'*40,'b'*40)))
        self.assertEqual(names[0],names[1])
        self.assertEqual(sum(bool(t['data'].get('ci_environment')) for t in self.store.tasks()),1)

    def test_refresh_base_requires_real_git_ancestry(self):
        from pipeline.ci import includes_base
        source=self.store.directory/'source';source.mkdir()
        def git(*args):return subprocess.check_output(['git',*args],cwd=source,text=True,stderr=subprocess.DEVNULL).strip()
        git('init','-q');git('config','user.name','fixture');git('config','user.email','fixture@example.com')
        git('-c','core.hooksPath=/dev/null','commit','--no-gpg-sign','--allow-empty','-qm','first');first=git('rev-parse','HEAD')
        git('-c','core.hooksPath=/dev/null','commit','--no-gpg-sign','--allow-empty','-qm','second');second=git('rev-parse','HEAD')
        self.assertTrue(includes_base(self.store,second,first))
        self.assertFalse(includes_base(self.store,first,second))
        self.assertFalse(includes_base(self.store,second,'missing-ref'))

    def test_observation_is_discarded_if_ci_recovers_or_head_changes_while_logs_download(self):
        for changed in ({'headRefOid':'f'*40},{'statusCheckRollup':[dict(self.checks[0],conclusion='SUCCESS')]}):
            with self.subTest(change=changed):
                self.change_on_confirmation=changed
                observer=self.observer()
                with patch.object(observer,'command',side_effect=self.command),self.assertRaises(Wait):
                    observe(observer,self.store.task('task-001'))
                self.store.retry(observer.claim['id'],observer.claim['token'],{},delay=0)
                self.assertFalse(self.store.task('task-001')['data'].get('repair'))

    def test_legacy_publication_uses_recorded_base_not_current_main(self):
        data=self.store.task('task-001')['data'];data.pop('published_verified')
        self.store.put_task('task-001','g','body',data)
        work=self.store.enqueue('publish','task-001','g',{'candidate':self.candidate,'base':self.base,'verified':self.receipt})
        self.store.event('pull_request',{'head':self.head},'task-001',work)
        self.assertEqual(publication(self.store,self.store.task('task-001')),self.receipt)

    def test_observation_cannot_race_a_branch_publisher(self):
        work=self.store.enqueue('publish','task-001','g',{'candidate':self.candidate,'verified':self.receipt})
        publisher=self.store.claim('publish','publisher',ident=work)
        self.assertIsNotNone(publisher)
        job=discover(self.store,'observe','new-main')[0]
        self.assertIsNone(self.store.claim('observe','observer',ident=job))

    def test_publisher_observes_failed_old_head_before_overwriting_it_with_a_new_integration(self):
        from pipeline.stages import publish
        data=self.store.task('task-001')['data']
        newer=self.receipt|{'head':'e'*40,'base':'new-main'}
        data['verified']=newer;self.store.put_task('task-001','g','body',data)
        work=self.store.enqueue('publish','task-001','g',{'candidate':self.candidate,'verified':newer})
        execution=Execution(self.store,self.store.claim('publish','publisher',ident=work))
        with patch.object(execution,'command',side_effect=self.command), \
             patch('pipeline.stages.checkout',side_effect=AssertionError('CI must be read before branch mutation')):
            result,updates,_=publish(execution,self.store.task('task-001'))
        self.assertEqual(result['status'],'candidate_failed')
        self.assertEqual(updates['candidate'],self.head)

    def test_observed_merge_restores_its_receipt_and_fences_unneeded_new_base_verification(self):
        work=self.store.enqueue('verify','task-001','g',{'candidate':self.candidate,'base':'new-main'})
        verifier=self.store.claim('verify','slow-verifier')
        self.pr.update(state='MERGED',mergeCommit={'oid':'f'*40})
        observer=self.observer()
        with patch.object(observer,'command',side_effect=self.command):
            result,updates,findings=observe(observer,self.store.task('task-001'))
        self.store.finish(observer.claim['id'],observer.claim['token'],result,updates,findings)
        self.assertEqual(self.store.task('task-001')['data']['verified'],self.receipt)
        self.assertNotIn('delivered',self.store.task('task-001')['data'])
        with self.assertRaises(StaleClaim):self.store.finish(work,verifier['token'],{})


if __name__=='__main__':unittest.main()
