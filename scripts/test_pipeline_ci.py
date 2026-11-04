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
            if args[-1]=='headRefOid,statusCheckRollup,state' and self.change_on_confirmation:
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

    def test_pending_checks_do_not_request_implementation_or_repeat_verification(self):
        self.pr['statusCheckRollup']=self.checks[1:]
        observer=self.observer()
        with patch.object(observer,'command',side_effect=self.command),self.assertRaises(Wait):
            observe(observer,self.store.task('task-001'))
        data=self.store.task('task-001')['data']
        self.assertFalse(data.get('repair'))
        self.assertEqual(data['ci']['status'],'pending')
        self.assertEqual(discover(self.store,'verify','new-main'),[])

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
