#!/usr/bin/env python3
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('ci', Path(__file__).with_name('dev-ci.py'))
ci = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ci)


class GitHubChecksTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.head, self.base = 'a' * 40, 'b' * 40
        self.pr = {'state': 'OPEN', 'headRefOid': self.head, 'mergeStateStatus': 'CLEAN', 'statusCheckRollup': []}
        self.calls = []
        self.baseline = []
        self.logs = {}

    def run_cli(self, *args, **kwargs):
        self.calls.append(args)
        if args[:3] == ('gh', 'pr', 'view'):
            value = json.dumps(self.pr)
        elif args[:2] == ('gh', 'api'):
            value = json.dumps({'workflow_runs': self.baseline})
        elif '--log-failed' in args:
            value = self.logs[args[3]]
        else:
            value = json.dumps({'workflowDatabaseId': 5, 'name': 'E2E Tests'})
        return subprocess.CompletedProcess(args, 0, value, '')

    def observe(self):
        return ci.observe(self.run_cli, 'https://github.com/example/repo/pull/1', self.head, self.base, self.temp.name)

    def check(self, conclusion='SUCCESS', status='COMPLETED'):
        return {'status': status, 'conclusion': conclusion, 'name': 'e2e', 'workflowName': 'E2E Tests',
                'detailsUrl': 'https://github.com/example/repo/actions/runs/10/job/100'}

    def setup_observation(self, *, changed=None, fixed_main=False, failed_step='Install Playwright browsers', stale=False, mixed=False):
        self.pr['statusCheckRollup']=[self.check('FAILURE')]
        self.logs={'10':'e2e\tInstall Playwright browsers\t2026-11-09T13:25:06Z Error: ERROR: Playwright does not support chromium on ubuntu26.04-x64\n'}
        if mixed:
            self.pr['statusCheckRollup'].append(self.check('FAILURE')|{'name':'Test','detailsUrl':'https://github.com/example/repo/actions/runs/20/job/200'})
            self.logs['20']='test result: FAILED. product assertion failed\n'
        self.baseline=[{'head_sha':self.base,'workflow_id':5,'status':'completed','conclusion':'success','id':9,'run_number':1}]
        main='f'*40
        def run(*args,**kwargs):
            if args[:2]!=('gh','api') or 'actions/runs?' in args[2]:return self.run_cli(*args,**kwargs)
            url=args[2]
            if url.endswith('/actions/runs/10'):
                value={'id':10,'head_sha':self.head,'status':'completed','conclusion':'failure','run_attempt':1,'path':'.github/workflows/e2e.yml'}
            elif url.endswith('/actions/jobs/100'):
                value={'id':100,'name':'e2e','head_sha':self.base if stale else self.head,'run_id':10,'status':'completed','conclusion':'failure','run_attempt':1,'steps':[{'name':failed_step,'conclusion':'failure'}]}
            elif '/git/ref/' in url:value={'object':{'sha':main}}
            else:
                path,ref=url.split('/contents/')[1].split('?ref=')
                if path.endswith('.npmrc'):return subprocess.CompletedProcess(args,1,json.dumps({'status':'404'}),'')
                different=(changed==path and ref==self.head) or (fixed_main and ref==main and path=='.github/workflows/e2e.yml')
                value={'type':'file','sha':('2' if different else '1')*40}
            return subprocess.CompletedProcess(args,0,json.dumps(value),'')
        return ci.observe(run,'https://github.com/example/repo/pull/1',self.head,self.base,self.temp.name)

    def test_setup_failure_ignores_historical_green_runner_and_has_stable_identity(self):
        first=self.setup_observation()
        self.assertEqual(first['status'],'environment_failed')
        self.head='c'*40;self.base='d'*40;self.pr['headRefOid']=self.head
        second=self.setup_observation()
        self.assertEqual(second['failure_signature'],first['failure_signature'])
        self.assertIn('ubuntu26.04-x64',second['environment_failure']['platforms'])

    def test_setup_failure_refreshes_against_changed_main_without_a_model_repair(self):
        result=self.setup_observation(fixed_main=True)
        self.assertEqual(result['status'],'environment_refresh')
        self.assertEqual(result['target_base'],'f'*40)

    def test_candidate_ci_changes_and_product_test_failures_are_not_shared(self):
        for path in ('.github/workflows/e2e.yml','web/package.json','web/package-lock.json'):
            with self.subTest(path=path):self.assertEqual(self.setup_observation(changed=path)['status'],'candidate_failed')
        self.assertEqual(self.setup_observation(failed_step='Run E2E tests')['status'],'candidate_failed')
        self.assertEqual(self.setup_observation(mixed=True)['status'],'candidate_failed')

    def test_stale_setup_job_cannot_create_a_shared_repair(self):
        with self.assertRaisesRegex(RuntimeError,'evidence changed'):self.setup_observation(stale=True)

    def test_no_checks_yet_or_pending_or_policy_blocks_publication(self):
        self.assertEqual(self.observe()['status'], 'pending')
        self.pr['statusCheckRollup'] = [self.check(None, 'IN_PROGRESS')]
        self.assertEqual(self.observe()['status'], 'pending')
        self.pr['statusCheckRollup'] = [self.check()]
        self.pr['mergeStateStatus'] = 'BLOCKED'
        self.assertEqual(self.observe()['status'], 'policy_pending')
        self.pr['mergeStateStatus'] = 'CLEAN'
        self.assertEqual(self.observe()['status'], 'passed')
        self.pr['headRefOid'] = self.base
        with self.assertRaisesRegex(RuntimeError, 'head changed'):
            self.observe()

    def test_exact_base_failure_requires_matching_test_defects(self):
        self.pr['statusCheckRollup'] = [self.check('FAILURE')]
        self.baseline = [{'head_sha': self.base, 'workflow_id': 5, 'status': 'completed', 'conclusion': 'failure', 'id': 9, 'run_number': 1}]
        self.logs = {'10': '2026-10-08 tests/e2e/app.spec.js:55:3 › expected real navigation ───\n',
                     '9': '2026-10-07 tests/e2e/app.spec.js:55:3 › expected real navigation ─────\n'}
        self.assertEqual(self.observe()['status'], 'baseline_failed')
        self.logs['10'] += 'tests/e2e/app.spec.js:90:3 › feature regression ───\n'
        self.assertEqual(self.observe()['status'], 'candidate_failed')
        self.assertIn('feature regression', (Path(self.temp.name) / 'github-checks.log').read_text())
        self.baseline[0]['head_sha'] = 'c' * 40
        self.assertEqual(self.observe()['status'], 'candidate_failed')

    def test_cancelled_runner_retries_without_implementation(self):
        self.pr['statusCheckRollup'] = [self.check('CANCELLED')]
        self.assertEqual(self.observe(), {'status': 'infrastructure', 'runs': [10]})
        self.assertEqual(len(self.calls), 1)

    def test_completed_failure_is_not_hidden_by_a_pending_sibling_check(self):
        self.pr['statusCheckRollup']=[self.check('FAILURE'),self.check(None,'IN_PROGRESS')]
        self.logs={'10':'tests/e2e/app.spec.js:4:1 › broken feature\n'}
        observed=self.observe()
        self.assertEqual(observed['status'],'candidate_failed')
        self.assertIn('broken feature',Path(observed['candidate_log']).read_text())

    def test_missing_or_unknown_completed_conclusion_is_not_a_code_failure(self):
        for conclusion in (None,'UNKNOWN','ACTION_REQUIRED'):
            self.pr['statusCheckRollup']=[self.check(conclusion)]
            self.assertEqual(self.observe()['status'],'pending')

    def test_confirmed_merge_conflicts_are_actionable_independently_of_ci_readiness(self):
        self.pr.update(mergeStateStatus='DIRTY',mergeable='CONFLICTING',baseRefOid='c'*40)
        for checks in ([],[self.check()],[self.check(None,'IN_PROGRESS')],[self.check('FAILURE')]):
            with self.subTest(checks=checks):
                self.pr['statusCheckRollup']=checks
                observed=self.observe()
                self.assertEqual(observed['status'],'merge_conflict')
                self.assertEqual(observed['head'],self.head)
                self.assertEqual(observed['base'],'c'*40)
                self.assertEqual(observed['verified_base'],self.base)
        self.pr.update(mergeable='UNKNOWN',statusCheckRollup=[])
        self.assertEqual(self.observe()['status'],'pending')

    def test_contradictory_status_requires_current_exact_head_workflow_and_job(self):
        check = self.check('SUCCESS','IN_PROGRESS') | {'completedAt':'2026-11-05T11:07:44Z'}
        self.pr['statusCheckRollup'] = [check]
        workflow = {'id':10,'head_sha':self.head,'status':'completed','conclusion':'success','run_attempt':1}
        job = {'id':100,'run_id':10,'head_sha':self.head,'name':'e2e','run_attempt':1,
               'status':'in_progress','conclusion':'success','completed_at':check['completedAt']}
        def run(*args,**kwargs):
            value = workflow if args[-1].endswith('/runs/10') else job
            return subprocess.CompletedProcess(args,0,json.dumps(value),'')
        fixed, evidence = ci.reconcile_checks(run,'https://github.com/example/repo/pull/1',self.head,self.pr)
        self.assertEqual(ci.check_sets(fixed),([],[],[]))
        self.assertEqual(evidence[0]['head'],self.head)
        self.assertEqual(check['status'],'IN_PROGRESS')  # preserve the raw observation
        for target,field,value in [(workflow,'head_sha',self.base),(workflow,'status','in_progress'),
                                   (workflow,'run_attempt',2),(job,'head_sha',self.base),
                                   (job,'completed_at',None),(job,'conclusion','failure'),
                                   (job,'run_id',11),(job,'name','different-check')]:
            with self.subTest(field=field,value=value):
                previous=target[field];target[field]=value
                unresolved,evidence=ci.reconcile_checks(run,'https://github.com/example/repo/pull/1',self.head,self.pr)
                self.assertEqual(len(ci.check_sets(unresolved)[0]),1)
                self.assertEqual(evidence,[])
                target[field]=previous
        check['conclusion']='FAILURE';job['conclusion']='failure'
        failed,_=ci.reconcile_checks(run,'https://github.com/example/repo/pull/1',self.head,self.pr)
        self.assertEqual(len(ci.check_sets(failed)[1]),1)

    def test_repair_evidence_separates_current_candidate_from_old_baseline(self):
        self.pr['statusCheckRollup'] = [self.check('FAILURE')]
        self.baseline = [{'head_sha': self.base, 'workflow_id': 5, 'status': 'completed',
                          'conclusion': 'failure', 'id': 9, 'run_number': 1}]
        self.logs = {'10': 'tests/e2e/explorer.spec.js:5:3 › current visual defect\n',
                     '9': 'obsolete navigation failure\n' * 4000}
        observation = self.observe()
        self.assertEqual(observation['status'], 'candidate_failed')
        current = Path(observation['candidate_log']).read_text()[-16000:]
        self.assertIn('current visual defect', current)
        self.assertNotIn('obsolete navigation failure', current)
        self.assertIn('obsolete navigation failure', Path(observation['baseline_log']).read_text())
        self.assertEqual(observation['runs'], [10])


if __name__ == '__main__':
    unittest.main()
