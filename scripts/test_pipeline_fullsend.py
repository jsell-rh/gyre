"""Transport and claim contracts; fixtures never qualify model quality."""
import importlib.util
import json
import os
import shutil
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import tarfile
import unittest
from unittest.mock import patch

from pipeline.execution import Execution, Retry
from pipeline.fullsend import prepare, binary
from pipeline.store import Store


class FullsendFixture(unittest.TestCase):
    def setUp(self):
        cli_env = patch.dict(os.environ, {'OPENSHELL': sys.executable})
        cli_env.start()
        self.addCleanup(cli_env.stop)
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / 'origin.git'
        self.call('git', 'init', '-q', '--bare', str(self.repo))
        source = self.root / 'source'
        self.call('git', 'clone', '-q', str(self.repo), str(source))
        self.call('git', 'checkout', '-qb', 'main', cwd=source)
        self.call('git', 'config', 'user.name', 'Fixture', cwd=source)
        self.call('git', 'config', 'user.email', 'fixture@example.invalid', cwd=source)
        (source / 'specs/tasks').mkdir(parents=True)
        self.body = '---\ntitle: Arithmetic fixture\nprogress: not-started\ndepends_on: []\nspec_ref: fixture\n---\n\n## Acceptance Criteria\n\nAdd returns the sum.\n'
        (source / 'specs/tasks/task-9999.md').write_text(self.body)
        (source / 'code.py').write_text('def add(a, b):\n    return a - b\n')
        self.call('git', 'add', '.', cwd=source)
        self.call('git', 'commit', '-qm', 'fixture baseline', cwd=source)
        self.base = self.call('git', 'rev-parse', 'HEAD', cwd=source).strip()
        self.call('git', 'push', '-q', 'origin', 'main', cwd=source)
        self.call('git', 'symbolic-ref', 'HEAD', 'refs/heads/main', cwd=self.repo)
        self.store = Store(self.root / 'state')
        self.addCleanup(self.store.close)
        self.store.set_setting('repo_url', str(self.repo))
        self.store.set_setting('fullsend_image', 'registry.invalid/fixture@sha256:' + 'a' * 64)
        self.store.set_setting('slots', 1)
        self.store.put_task('task-9999', 'g', self.body, {})
        self.work = self.store.enqueue('implement', 'task-9999', 'g', {'base': self.base})
        self.claim = self.store.claim('implement', 'fixture-owner')
        self.execution = Execution(self.store, self.claim)

    def call(self, *args, **kwargs):
        return subprocess.check_output(args, stderr=subprocess.PIPE, text=True, **kwargs)

    def prepare(self):
        return prepare(self.execution, self.store.task('task-9999'), 'Fixture assignment only.')


class FullsendContracts(FullsendFixture):
    def test_prompt_excludes_baseline_failures_from_a_different_base(self):
        from pipeline.stages import prompt
        self.store.finish(self.work, self.claim['token'], {}, findings=[
            {'category': 'baseline', 'base': 'old-main', 'detail': 'OBSOLETE_BASELINE_SENTINEL'},
            {'category': 'baseline', 'base': self.base, 'detail': 'CURRENT_BASELINE_SENTINEL'},
        ])
        with patch.object(self.execution, 'command', return_value=subprocess.CompletedProcess([], 0, stdout='task')):
            text = prompt(self.execution, self.store.task('task-9999'))
        self.assertNotIn('OBSOLETE_BASELINE_SENTINEL', text)
        self.assertIn('CURRENT_BASELINE_SENTINEL', text)

    def test_prepared_checkout_is_self_contained_and_harness_has_no_key(self):
        cfg, repo, shim = self.prepare()
        self.assertFalse((repo / '.git/objects/info/alternates').exists())
        self.assertEqual(self.call('git', 'rev-parse', 'HEAD', cwd=repo).strip(), self.base)
        self.assertEqual(self.call('git', 'branch', '--show-current', cwd=repo).strip(), self.execution.branch)
        harness = json.loads((cfg / 'harness/implement.yaml').read_text())
        self.assertTrue(all(not p['dest'].endswith('auth.json') for p in harness['host_files']))
        self.assertNotIn('ENMAAS_API_KEY', json.dumps(harness))
        self.assertNotIn('FULLSEND_INFERENCE_GATEWAY_API_KEY', json.dumps(harness))
        self.assertEqual(harness['sandbox_timeout_seconds'], 14400)
        self.assertEqual(harness['timeout_minutes'], 240)
        self.assertTrue((shim / 'openshell').is_file())

    def test_assignment_clone_excludes_old_attempts_and_publishes_from_copied_checkout(self):
        source=self.root/'source'
        self.call('git','checkout','-qb','pipeline/unrelated',cwd=source)
        (source/'unrelated.bin').write_bytes(b'unrelated historical attempt\n'*10000)
        self.call('git','add','unrelated.bin',cwd=source)
        self.call('git','commit','-qm','unrelated attempt',cwd=source)
        unrelated=self.call('git','rev-parse','HEAD:unrelated.bin',cwd=source).strip()
        self.call('git','push','-q','origin','pipeline/unrelated',cwd=source)
        self.call('git','checkout','-qb','pipeline/candidate',self.base,cwd=source)
        (source/'code.py').write_text('def add(a, b):\n    return a + b\n')
        self.call('git','commit','-qam','assigned candidate',cwd=source)
        candidate=self.call('git','rev-parse','HEAD',cwd=source).strip()
        self.call('git','push','-q','origin','pipeline/candidate',cwd=source)
        self.call('git','config','uploadpack.allowFilter','true',cwd=self.repo)
        # file:// exercises real pack negotiation rather than Git's local
        # hardlink optimization, which ignores partial-clone filters.
        self.store.set_setting('repo_url',self.repo.as_uri())
        self.execution.claim['input']['candidate']=candidate
        _,repo,_=self.prepare()
        self.assertEqual(self.call('git','rev-parse','HEAD',cwd=repo).strip(),candidate)
        self.assertEqual(self.call('git','config','remote.origin.partialclonefilter',cwd=repo).strip(),'blob:none')
        refs=self.call('git','for-each-ref','--format=%(refname)','refs/remotes',cwd=repo)
        self.assertNotIn('pipeline/unrelated',refs)
        self.assertNotIn('pipeline/candidate',refs)
        self.assertFalse((repo/'.git/shallow').exists())
        objects=self.call('git','cat-file','--batch-all-objects','--batch-check=%(objectname)',cwd=repo)
        self.assertNotIn(unrelated,objects)
        self.assertFalse((repo/'.git/objects/info/alternates').exists())
        copied=self.root/'copied-to-sandbox';shutil.copytree(repo,copied)
        shutil.rmtree(repo)
        self.call('git','merge-base','--is-ancestor',self.base,candidate,cwd=copied)
        self.assertIn('a + b',(copied/'code.py').read_text())
        self.call('git','push','-q','origin','HEAD:refs/heads/pipeline/publication-proof',cwd=copied)
        self.assertEqual(self.call('git','rev-parse','refs/heads/pipeline/publication-proof',cwd=self.repo).strip(),candidate)

    def test_assessment_is_registered_with_fullsend_and_has_a_read_only_guard(self):
        self.store.finish(self.work,self.claim['token'],{})
        work=self.store.enqueue('assess','task-9999','g',{'base':self.base,'candidate':self.base,'requirements':'fp'})
        self.claim=self.store.claim('assess','assessor');self.execution=Execution(self.store,self.claim)
        cfg,repo,_=self.prepare()
        config=(cfg/'config.yaml').read_text()
        self.assertIn('name: assess\n    source: harness/assess.yaml',config)
        harness=json.loads((cfg/'harness/assess.yaml').read_text())
        self.assertEqual(harness['role'],'review')
        self.assertEqual(harness['agent'],'agents/assess.md')
        self.assertTrue((cfg/'agents/assess.md').is_file())
        self.assertTrue((self.execution.directory/'bundle/review-before.json').is_file())

    def interrupted_review(self, source_edit=False, wrong_candidate=False):
        import io
        self.store.finish(self.work,self.claim['token'],{})
        work=self.store.enqueue('review','task-9999','g',{'base':self.base,'candidate':self.base})
        old=Execution(self.store,self.store.claim('review','old-reviewer'))
        _,repo,_=prepare(old,self.store.task('task-9999'),'review')
        note=repo/'specs/reviews/task-9999.md';note.parent.mkdir(parents=True)
        note.write_text('Checked actual add behavior; still need scoped tests.\n')
        if source_edit:(repo/'code.py').write_text('unsafe reviewer production edit\n')
        self.call('git','add','.',cwd=repo);self.call('git','commit','-qm','review evidence',cwd=repo)
        self.call('git','push','-q','origin','HEAD',cwd=repo)
        head=self.call('git','rev-parse','HEAD',cwd=repo).strip()
        (old.directory/'outcome.json').write_text(json.dumps({'head':head,'published':True,'valid':False,
                    'source_valid':not source_edit,'agent_exit':1}))
        with tarfile.open(old.directory/'evidence.tar.gz','w:gz') as archive:
            for name,value in [('review-evidence/probe.txt',b'real probe evidence'),
                               ('verdict.json',b'{"approved":true}'),
                               ('review-evidence/../../escape',b'must not escape')]:
                info=tarfile.TarInfo(name);info.size=len(value);archive.addfile(info,io.BytesIO(value))
        if wrong_candidate:
            p=old.directory/'bundle/job.json';d=json.loads(p.read_text());d['input']['candidate']='f'*40;p.write_text(json.dumps(d))
        self.store.retry(work,old.claim['token'],{'message':'interrupted'},delay=0)
        return Execution(self.store,self.store.claim('review','replacement'))

    def test_interrupted_review_resumes_evidence_without_changing_candidate_or_reusing_verdict(self):
        replacement=self.interrupted_review()
        cfg,repo,_=prepare(replacement,self.store.task('task-9999'),'resume review')
        bundle=replacement.directory/'bundle'
        manifest=json.loads((bundle/'review-resume.json').read_text())
        self.assertFalse(manifest['approved'])
        self.assertEqual(manifest['candidate'],self.base)
        self.assertEqual(self.call('git','rev-parse','HEAD',cwd=repo).strip(),self.base)
        self.assertFalse((repo/'specs/reviews/task-9999.md').exists())
        self.assertTrue((bundle/'review-resume/specs/reviews/task-9999.md').exists())
        self.assertEqual((bundle/'review-resume/review-evidence/probe.txt').read_text(),'real probe evidence')
        self.assertFalse((bundle/'review-resume/verdict.json').exists())
        self.assertFalse((bundle/'escape').exists())
        self.assertEqual(json.loads((cfg/'harness/review.yaml').read_text())['timeout_minutes'],240)

    def test_review_resume_rejects_modified_source(self):
        replacement=self.interrupted_review(source_edit=True)
        prepare(replacement,self.store.task('task-9999'),'review')
        self.assertFalse((replacement.directory/'bundle/review-resume.json').exists())

    def test_review_resume_rejects_different_candidate(self):
        replacement=self.interrupted_review(wrong_candidate=True)
        prepare(replacement,self.store.task('task-9999'),'review')
        self.assertFalse((replacement.directory/'bundle/review-resume.json').exists())

    def test_saved_task_with_null_optional_artifacts_can_resume(self):
        task = self.store.task('task-9999')
        task['data'] = {'retained_artifacts': None,
                        'repair': {'stash_artifacts': None, 'artifact_directories': None}}
        cfg, repo, _ = prepare(self.execution, task, 'Resume the saved assignment.')
        self.assertTrue((repo / '.git').is_dir())
        self.assertTrue((cfg / 'harness/implement.yaml').is_file())

    def test_verification_evidence_preserves_early_error_despite_noisy_log_tail(self):
        log = self.store.directory / 'verification.log'
        log.write_bytes(b'ERROR: unsafe UTF-8 byte slice in production code\n' + b'build warning\n' * 10000 + b'frontend build finished\n')
        task = self.store.task('task-9999')
        task['data'] = {'repair': {'category': 'verification', 'artifact': str(log)}}
        cfg, _, _ = prepare(self.execution, task, 'Repair the verifier finding.')
        staged = self.execution.directory / 'bundle/findings.log'
        data = staged.read_bytes()
        self.assertLessEqual(len(data), 65536)
        self.assertTrue(data.startswith(b'ERROR: unsafe UTF-8'))
        self.assertTrue(data.endswith(b'frontend build finished\n'))
        harness = json.loads((cfg / 'harness/implement.yaml').read_text())
        self.assertIn('/tmp/stage/findings.log', [item['dest'] for item in harness['host_files']])

    def test_actual_checkpoint_publishes_fixture_source_to_bare_remote(self):
        self.body += '\n## Acceptance Criteria\n\n- [ ] Arithmetic performs real addition.\n'
        self.store.put_task('task-9999','g',self.body,{})
        _, repo, _ = self.prepare()
        (repo / 'code.py').write_text('def add(a, b):\n    return a + b\n')
        (repo / 'specs/tasks/task-9999.md').write_text(self.body.replace('not-started','ready-for-review').replace('- [ ]','- [x]'))
        stage = self.execution.directory / 'bundle'
        job = json.loads((stage / 'job.json').read_text());job['checkout'] = str(repo)
        (stage / 'job.json').write_text(json.dumps(job))
        spec = importlib.util.spec_from_file_location('worker', Path(__file__).with_name('pipeline-remote.py'))
        module = importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
        previous = Path.cwd()
        self.addCleanup(os.chdir, previous)
        with patch.object(module, 'STAGE', stage), patch.object(sys, 'argv', ['worker','finalize','1']):
            self.assertEqual(module.main(), 0)
        receipt = json.loads((stage / 'outcome.json').read_text())
        actual = self.call('git', 'rev-parse', 'refs/heads/' + self.execution.branch, cwd=self.repo).strip()
        self.assertEqual(actual, receipt['head'])
        published_task=self.call('git','show',actual+':specs/tasks/task-9999.md',cwd=self.repo)
        self.assertIn('- [ ] Arithmetic performs real addition.',published_task)
        self.assertNotIn('- [x]',published_task)
        self.assertEqual(receipt['tree'], self.call('git', 'rev-parse', actual + '^{tree}', cwd=self.repo).strip())
        self.assertIn('a + b', self.call('git','show',actual+':code.py',cwd=self.repo))
        self.assertTrue(receipt['published'] and receipt['valid'])

    def test_evidence_capture_retains_probes_without_credentials_or_symlink_targets(self):
        from pipeline.fullsend import capture_evidence
        stage = self.root / 'remote-stage'
        (stage / 'review-evidence').mkdir(parents=True)
        (stage / 'review-evidence/probe.txt').write_text('actual probe output')
        (stage / 'auth.json').write_text('credential must not be captured')
        (stage / 'review-evidence/linked-secret').symlink_to(stage / 'auth.json')
        def remote(*args, **kwargs):
            code = args[-1].replace("Path('/tmp/stage')", 'Path(' + repr(str(stage)) + ')')
            return subprocess.run([sys.executable, '-c', code], capture_output=True, text=True, check=True)
        with patch.object(self.execution, 'os', side_effect=remote):
            capture_evidence(self.execution, 'fixture')
        with tarfile.open(self.execution.directory / 'evidence.tar.gz') as archive:
            self.assertEqual(archive.getnames(), ['review-evidence/probe.txt'])
            self.assertEqual(archive.extractfile('review-evidence/probe.txt').read(), b'actual probe output')

    def test_restoring_a_contract_does_not_require_an_unrelated_production_edit(self):
        from pipeline.stages import implement
        task = self.store.task('task-9999')
        task['data'] = {'candidate': self.base, 'candidate_base': self.base,
                        'repair': {'category': 'contract', 'source': self.base}}
        receipt = {'head': self.base, 'base': self.base, 'branch': 'fixture',
                   'valid': True, 'agent_exit': 0,
                   'task_body': self.body.replace('not-started', 'ready-for-review')}
        with patch.object(self.execution, 'cloud_step', return_value=receipt), patch('pipeline.stages.checkout', side_effect=AssertionError('contract restoration must not demand source churn')):
            _, updates, _ = implement(self.execution, task)
        self.assertIsNone(updates['repair'])
        self.assertEqual(updates['candidate'], self.base)
        self.assertEqual(updates['review_context']['category'], 'contract')

    def test_contract_repair_keeps_source_and_reports_the_protected_text_difference(self):
        from pipeline.stages import implement
        _, repo, _ = self.prepare()
        (repo / 'code.py').write_text('def add(a, b):\n    return a + b\n')
        self.call('git', 'add', '.', cwd=repo)
        self.call('git', '-c', 'core.hooksPath=/dev/null', 'commit', '-qm', 'fixture source repair', cwd=repo)
        head = self.call('git', 'rev-parse', 'HEAD', cwd=repo).strip()
        self.execution.log.write_text('transport noise must not replace contract evidence')
        changed = self.body.replace('progress: not-started', 'progress: ready-for-review') + '\n## Round 2 diagnostics\n\nObserved transport output.\n'
        receipt = {'head': head, 'base': self.base, 'branch': self.execution.branch,
                   'valid': True, 'agent_exit': 0, 'task_body': changed}
        with patch.object(self.execution, 'cloud_step', return_value=receipt), patch('pipeline.stages.checkout', return_value=repo):
            result, updates, _ = implement(self.execution, self.store.task('task-9999'))
        self.assertEqual(result['head'], head)
        self.assertEqual(result['task_body'], self.body)
        self.assertEqual(updates['repair']['category'], 'contract')
        self.assertIn('+## Round 2 diagnostics', updates['repair']['log_tail'])
        self.assertNotIn('transport noise', updates['repair']['log_tail'])
        self.assertIn('exactly ## Shipped', updates['repair']['detail'])

    def test_modified_runner_is_rejected_before_execution(self):
        path = self.root / 'modified-fullsend';path.write_text('untrusted executable')
        with patch.dict(os.environ, {'GYRE_FULLSEND': str(path)}):
            with self.assertRaisesRegex(Retry, 'missing or changed'):
                binary(self.store)

    def test_pause_does_not_change_capacity_setting(self):
        self.store.set_setting('slots', 82)
        self.store.set_setting('admission_paused', True)
        with patch.object(self.execution, 'login', side_effect=AssertionError('no gateway calls while paused')):
            with self.assertRaisesRegex(Retry, 'paused'):
                self.execution.cloud_step(self.store.task('task-9999'), '')
        self.assertEqual(self.store.setting('slots'), 82)
        self.assertEqual(self.store.db.execute('SELECT count(*) FROM resources').fetchone()[0], 0)


class OpenShellBoundary(FullsendFixture):
    def setUp(self):
        super().setUp()
        _, _, self.shim = self.prepare()
        self.backend = self.root / 'real-openshell'
        self.backend.write_text('''#!/usr/bin/python3
import json,os,sys
from pathlib import Path
p=Path(os.environ['BACKEND_STATE']);v=json.loads(p.read_text()) if p.exists() else {'calls':[], 'pods':[]}
a=sys.argv[1:]
if a[:1]==['-g']:
 v.setdefault('gateways',[]).append(a[1]);a=a[2:]
v['calls'].append(a)
if a[:2]==['sandbox','create']:
 n=a[a.index('--name')+1];labels=dict(a[i+1].split('=',1) for i,x in enumerate(a) if x=='--label');v['pods'].append({'name':n,'phase':'Ready','labels':labels})
elif a[:2]==['sandbox','delete'] and not os.environ.get('HOLD_DELETE'):
 v['pods']=[x for x in v['pods'] if x['name']!=a[2]]
p.write_text(json.dumps(v))
if a[:2]==['sandbox','exec']:
 import time
 p.with_suffix('.pid').write_text(str(os.getpid()))
 if os.environ.get('STAGING_TEST'):
  mode=os.environ['STAGING_TEST'];n=sum(c[:2]==['sandbox','exec'] for c in v['calls'])
  if mode=='hold':time.sleep(60)
  if mode=='always' or (mode=='once' and n==1):
   print('Error:   × connect error: transport error',file=sys.stderr);sys.exit(1)
  if mode=='remote':
   print('remote command failed: transport error',file=sys.stderr);sys.exit(1)
  v['executions']=v.get('executions',0)+1;p.write_text(json.dumps(v))
  print('staged fixture');sys.exit(0)
 time.sleep(60)
if os.environ.get('WARN_ONCE') and not any(x[:2]==['gateway','login'] for x in v['calls']):
 print('Warning: OIDC token refresh failed; continuing with cached token',file=sys.stderr)
if a[:2]==['provider','list'] and os.environ.get('EXPIRE_ONCE') and not any(x[:2]==['gateway','login'] for x in v['calls']):
 print('cached OIDC token has expired',file=sys.stderr);sys.exit(1)
if a[:2]==['sandbox','list']: print(json.dumps({'sandboxes':v['pods'],'next_page_token':''}))
elif a[:2]==['sandbox','get']: print(json.dumps(next(x for x in v['pods'] if x['name']==a[2])))
else: print('fixture control operation')
''')
        self.backend.chmod(0o700)
        context = self.execution.directory / 'fullsend-context.json'
        value=json.loads(context.read_text());value['real_openshell']=str(self.backend)
        context.write_text(json.dumps(value))
        self.env = {**os.environ,'GYRE_FULLSEND_CONTEXT':str(context),'BACKEND_STATE':str(self.root/'backend.json'),
                    'OPENSHELL_GATEWAY_INSECURE':'true','OPENSHELL_NO_BROWSER':'1'}
        self.name='fs-imp-'+'a'*12

    def invoke(self,*args,extra=None):
        return subprocess.run([str(self.shim/'openshell'),*args],env={**self.env,**(extra or {})},capture_output=True,text=True,timeout=30)

    def test_creation_is_reserved_and_owned_before_real_cli_and_capacity_is_enforced(self):
        self.store.set_setting('compute_memory:task-9999:implement',8)
        result=self.invoke('sandbox','create','--name',self.name,'--memory','8Gi','--keep','--','sleep','infinity')
        self.assertEqual(result.returncode,0,result.stderr)
        state=json.loads((self.root/'backend.json').read_text())
        self.assertNotIn('--keep',state['calls'][0])
        self.assertEqual(state['calls'][0][state['calls'][0].index('--memory')+1],'4Gi')
        self.assertIn('gyre.dev/work='+self.work,state['calls'][0])
        self.assertEqual(self.store.db.execute('select state from resources where name=?',(self.name,)).fetchone()[0],'present')
        refused=self.invoke('sandbox','create','--name','fs-imp-'+'b'*12,'--','sleep','infinity')
        self.assertEqual(refused.returncode,75)
        self.assertEqual(len(json.loads((self.root/'backend.json').read_text())['pods']),1)

    def test_readiness_retry_adopts_the_same_pending_allocation(self):
        self.assertEqual(self.invoke('sandbox','create','--name',self.name,'--','sleep','infinity').returncode,0)
        path=self.root/'backend.json';state=json.loads(path.read_text());state['pods'][0]['phase']='Provisioning';path.write_text(json.dumps(state))
        self.assertEqual(self.invoke('sandbox','delete',self.name).returncode,75)
        retry=self.invoke('sandbox','create','--name',self.name,'--','sleep','infinity')
        self.assertEqual(retry.returncode,0,retry.stderr)
        state=json.loads(path.read_text())
        self.assertEqual(sum(c[:2]==['sandbox','create'] for c in state['calls']),1)
        self.assertEqual(len(state['pods']),1)
        self.assertEqual(self.store.compute_used(),1)

    def test_streaming_deadline_targets_the_real_cli_without_an_orphan_parent(self):
        process = subprocess.Popen([str(self.shim / 'openshell'), 'sandbox', 'exec',
                                    '--name', self.name, '--', 'sleep', '60'],
                                   env=self.env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        try:
            marker = self.root / 'backend.pid'
            deadline = time.monotonic() + 5
            while not marker.exists() and time.monotonic() < deadline:
                time.sleep(.02)
            self.assertTrue(marker.exists(), 'real streaming command did not start')
            actual_pid = int(marker.read_text())
            self.assertEqual(actual_pid, process.pid, 'deadline would leave an orphan holding the pipe')
            process.terminate()
            process.wait(timeout=5)
            self.assertFalse(Path('/proc/' + str(actual_pid)).exists())
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
            process.stderr.close()

    def staging_args(self):
        path='/tmp/fs-upload-'+'a'*20+'/fixture'
        return ['sandbox','exec','--name',self.name,'--no-tty','--timeout','120','--','sh','-c',
                f"test -f '{path}' && mkdir -p -- '/tmp/stage/bin' && mv -fT -- '{path}' '/tmp/stage/bin/fixture'"]

    def test_bootstrap_connect_retry_keeps_the_same_sandbox_and_executes_once(self):
        result=self.invoke(*self.staging_args(),extra={'STAGING_TEST':'once'})
        self.assertEqual(result.returncode,0,result.stderr)
        state=json.loads((self.root/'backend.json').read_text())
        calls=[c for c in state['calls'] if c[:2]==['sandbox','exec']]
        self.assertEqual(len(calls),2)
        self.assertEqual(calls[0],calls[1])
        self.assertEqual(state['executions'],1)
        self.assertFalse(any(c[:2] in (['sandbox','create'],['sandbox','delete']) for c in state['calls']))
        self.assertEqual(self.store.db.execute("SELECT count(*) FROM events WHERE kind='bootstrap_connect_retry'").fetchone()[0],1)

    def test_bootstrap_retries_are_bounded_and_do_not_replay_remote_failures(self):
        for mode,count in [('always',3),('remote',1)]:
            with self.subTest(mode=mode):
                path=self.root/'backend.json'
                if path.exists():path.unlink()
                result=self.invoke(*self.staging_args(),extra={'STAGING_TEST':mode})
                self.assertEqual(result.returncode,1,result.stderr)
                state=json.loads(path.read_text())
                self.assertEqual(sum(c[:2]==['sandbox','exec'] for c in state['calls']),count)
                self.assertNotIn('executions',state)

    def test_preconnect_diagnostic_does_not_enable_arbitrary_command_replay(self):
        args=self.staging_args();args[-1]='echo arbitrary-command'
        result=self.invoke(*args,extra={'STAGING_TEST':'once'})
        self.assertEqual(result.returncode,1)
        state=json.loads((self.root/'backend.json').read_text())
        self.assertEqual(sum(c[:2]==['sandbox','exec'] for c in state['calls']),1)

    def test_bootstrap_wrapper_death_does_not_orphan_the_cli(self):
        process=subprocess.Popen([str(self.shim/'openshell'),*self.staging_args()],
            env={**self.env,'STAGING_TEST':'hold'},stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        child=None
        try:
            marker=self.root/'backend.pid';deadline=time.monotonic()+5
            while not marker.exists() and time.monotonic()<deadline:time.sleep(.02)
            self.assertTrue(marker.exists())
            child=int(marker.read_text());self.assertNotEqual(child,process.pid)
            process.kill();process.wait(timeout=5)
            deadline=time.monotonic()+5
            while time.monotonic()<deadline:
                stat=Path(f'/proc/{child}/stat')
                if not stat.exists() or stat.read_text().rsplit(')',1)[1].split()[0]=='Z':break
                time.sleep(.02)
            else:self.fail('bootstrap CLI survived its killed wrapper')
        finally:
            if process.poll() is None:process.kill();process.wait(timeout=5)
            if child and Path(f'/proc/{child}').exists():
                try:os.kill(child,9)
                except ProcessLookupError:pass

    def test_checkpoint_hook_consumes_matching_scope_and_refuses_other_gateway(self):
        result = self.invoke('--gateway', 'gyre-gyre', 'provider', 'list')
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = json.loads((self.root / 'backend.json').read_text())['calls']
        self.assertEqual(calls, [['provider', 'list']])
        refused = self.invoke('--gateway', 'foreign', 'provider', 'list')
        self.assertEqual(refused.returncode, 75)
        self.assertEqual(json.loads((self.root / 'backend.json').read_text())['calls'], calls)

    def test_inflight_hooks_follow_only_the_authenticated_instance_alias(self):
        alias='gyre-d-fixture'
        self.store.set_setting('dispatcher_gateway_registration',{'source':'gyre-gyre','alias':alias})
        self.store.set_setting('gateway',alias)
        for selector in ('gyre-gyre',alias):
            result=self.invoke('--gateway',selector,'provider','list')
            self.assertEqual(result.returncode,0,result.stderr)
        state=json.loads((self.root/'backend.json').read_text())
        self.assertEqual(state['gateways'],[alias,alias])
        refused=self.invoke('--gateway','hypershell-other','provider','list')
        self.assertEqual(refused.returncode,75)
        self.assertEqual(json.loads((self.root/'backend.json').read_text())['gateways'],[alias,alias])
        streamed=self.invoke('sandbox','upload',self.name,'fixture','/tmp/fixture')
        self.assertEqual(streamed.returncode,0,streamed.stderr)
        self.assertEqual(set(json.loads((self.root/'backend.json').read_text())['gateways']),{alias})

    def test_streamed_bootstrap_refreshes_auth_before_replacing_the_process(self):
        result=self.invoke('sandbox','upload',self.name,'fixture','/tmp/fixture')
        self.assertEqual(result.returncode,0,result.stderr)
        calls=json.loads((self.root/'backend.json').read_text())['calls']
        login=next(i for i,c in enumerate(calls) if c[:2]==['gateway','login'])
        upload=next(i for i,c in enumerate(calls) if c[:2]==['sandbox','upload'])
        self.assertLess(login,upload)
        self.assertEqual(sum(c[:2]==['sandbox','upload'] for c in calls),1)

    def test_cancellation_allows_cleanup_transport_but_forbids_new_compute(self):
        self.store.set_setting(f"cancel:{self.work}:{self.claim['token']}",True)
        result=self.invoke('provider','list')
        self.assertEqual(result.returncode,0,result.stderr)
        refused=self.invoke('sandbox','create','--name',self.name,'--','sleep','infinity')
        self.assertEqual(refused.returncode,75)
        self.assertEqual(json.loads((self.root/'backend.json').read_text())['pods'],[])

    def test_old_worker_auth_refresh_does_not_touch_shared_registration(self):
        alias='gyre-d-fixture'
        self.store.set_setting('dispatcher_gateway_registration',{'source':'gyre-gyre','alias':alias})
        self.store.set_setting('gateway',alias)
        result=self.invoke('provider','list',extra={'EXPIRE_ONCE':'1','GYRE_OPEN_GATEWAY':'gyre-gyre'})
        self.assertEqual(result.returncode,0,result.stderr)
        state=json.loads((self.root/'backend.json').read_text())
        self.assertTrue(any(c[:2]==['gateway','login'] for c in state['calls']))
        self.assertEqual(set(state['gateways']),{alias})

    def test_expired_gateway_auth_is_refreshed_and_operation_retried(self):
        result=self.invoke('provider','list','--names',extra={'EXPIRE_ONCE':'1'})
        self.assertEqual(result.returncode,0,result.stderr)
        calls=json.loads((self.root/'backend.json').read_text())['calls']
        self.assertEqual(sum(x[:2]==['provider','list'] for x in calls),2)
        self.assertTrue(any(x[:2]==['gateway','login'] for x in calls))

    def test_successful_mutation_with_cached_token_warning_is_not_replayed(self):
        result = self.invoke('provider', 'create', '--name', 'inference-gateway-fixture',
                             '--type', 'fixture', extra={'WARN_ONCE': '1'})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn('OIDC token refresh failed', result.stderr)
        calls = json.loads((self.root / 'backend.json').read_text())['calls']
        self.assertEqual(sum(x[:2] == ['provider', 'create'] for x in calls), 1)
        self.assertTrue(any(x[:2] == ['gateway', 'login'] for x in calls))

    def test_deletion_does_not_release_capacity_while_backend_retains_compute(self):
        self.assertEqual(self.invoke('sandbox','create','--name',self.name,'--','sleep','infinity').returncode,0)
        (self.execution.directory/'outcome.json').write_text(json.dumps({'published':True}))
        result=self.invoke('sandbox','delete',self.name,extra={'HOLD_DELETE':'1'})
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertEqual(self.store.db.execute('select state from resources where name=?',(self.name,)).fetchone()[0],'deleting')
        self.assertEqual(len(json.loads((self.root/'backend.json').read_text())['pods']),1)

    def test_failed_review_deletes_without_attempting_source_capture(self):
        self.assertEqual(self.invoke('sandbox','create','--name',self.name,'--','sleep','infinity').returncode,0)
        self.store.db.execute("UPDATE work SET stage='review' WHERE id=?",(self.work,))
        result=self.invoke('sandbox','delete',self.name)
        self.assertEqual(result.returncode,0,result.stderr)
        calls=json.loads((self.root/'backend.json').read_text())['calls']
        self.assertFalse(any(x[:2]==['sandbox','exec'] for x in calls))
        self.assertFalse(json.loads((self.root/'backend.json').read_text())['pods'])

    def test_fenced_worker_cannot_invoke_gateway(self):
        self.store.finish(self.work,self.claim['token'],{})
        result=self.invoke('sandbox','create','--name',self.name,'--','sleep','infinity')
        self.assertEqual(result.returncode,75)
        self.assertFalse((self.root/'backend.json').exists())

    def test_run_scoped_provider_intent_is_recorded_and_foreign_deletion_rejected(self):
        name='inference-gateway-fixture'
        result=self.invoke('provider','create','--name',name,'--type','fixture')
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertEqual(self.store.db.execute('select kind,state from resources where name=?',(name,)).fetchone()[:],('provider','present'))
        refused=self.invoke('provider','delete','inference-gateway-foreign')
        self.assertEqual(refused.returncode,75)
        calls=json.loads((self.root/'backend.json').read_text())['calls']
        self.assertFalse(any(x[:3]==['provider','delete','inference-gateway-foreign'] for x in calls))


if __name__ == '__main__':
    unittest.main()
