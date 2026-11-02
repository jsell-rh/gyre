"""Transport and claim contracts; fixtures never qualify model quality."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
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
    def test_prepared_checkout_is_self_contained_and_harness_has_no_key(self):
        cfg, repo, shim = self.prepare()
        self.assertFalse((repo / '.git/objects/info/alternates').exists())
        self.assertEqual(self.call('git', 'rev-parse', 'HEAD', cwd=repo).strip(), self.base)
        self.assertEqual(self.call('git', 'branch', '--show-current', cwd=repo).strip(), self.execution.branch)
        harness = json.loads((cfg / 'harness/implement.yaml').read_text())
        self.assertTrue(all(not p['dest'].endswith('auth.json') for p in harness['host_files']))
        self.assertNotIn('ENMAAS_API_KEY', json.dumps(harness))
        self.assertNotIn('FULLSEND_INFERENCE_GATEWAY_API_KEY', json.dumps(harness))
        self.assertEqual(harness['sandbox_timeout_seconds'], 600)
        self.assertTrue((shim / 'openshell').is_file())

    def test_actual_checkpoint_publishes_fixture_source_to_bare_remote(self):
        _, repo, _ = self.prepare()
        (repo / 'code.py').write_text('def add(a, b):\n    return a + b\n')
        (repo / 'specs/tasks/task-9999.md').write_text(self.body.replace('not-started','ready-for-review'))
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
if a[:1]==['-g']: a=a[2:]
v['calls'].append(a)
if a[:2]==['sandbox','create']:
 n=a[a.index('--name')+1];v['pods'].append({'name':n,'phase':'Ready','labels':{}})
elif a[:2]==['sandbox','delete'] and not os.environ.get('HOLD_DELETE'):
 v['pods']=[x for x in v['pods'] if x['name']!=a[2]]
p.write_text(json.dumps(v))
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
        result=self.invoke('sandbox','create','--name',self.name,'--keep','--','sleep','infinity')
        self.assertEqual(result.returncode,0,result.stderr)
        state=json.loads((self.root/'backend.json').read_text())
        self.assertNotIn('--keep',state['calls'][0])
        self.assertIn('gyre.dev/work='+self.work,state['calls'][0])
        self.assertEqual(self.store.db.execute('select state from resources where name=?',(self.name,)).fetchone()[0],'present')
        refused=self.invoke('sandbox','create','--name','fs-imp-'+'b'*12,'--','sleep','infinity')
        self.assertEqual(refused.returncode,75)
        self.assertEqual(len(json.loads((self.root/'backend.json').read_text())['pods']),1)

    def test_checkpoint_hook_consumes_matching_scope_and_refuses_other_gateway(self):
        result = self.invoke('--gateway', 'gyre-gyre', 'provider', 'list')
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = json.loads((self.root / 'backend.json').read_text())['calls']
        self.assertEqual(calls, [['provider', 'list']])
        refused = self.invoke('--gateway', 'foreign', 'provider', 'list')
        self.assertEqual(refused.returncode, 75)
        self.assertEqual(json.loads((self.root / 'backend.json').read_text())['calls'], calls)

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
