"""Exercise crash windows and cleanup against the real ledger and handlers."""
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
from pipeline.store import Store
from pipeline.execution import Execution
from pipeline.stages import cleanup, implement, prompt

spec = importlib.util.spec_from_file_location('pipeline_cli', Path(__file__).with_name('dev-pipeline.py'))
cli = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cli)


class CrashTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.store = Store(self.temp.name)
        self.addCleanup(self.store.close)
        self.store.put_task('task-001', 'g', 'body', {'dependencies': []})
        self.work = self.store.enqueue('implement', 'task-001', 'g', {})
        self.claim = self.store.claim('implement', 'actor')

    def test_inventory_cannot_release_inflight_creation(self):
        self.store.reserve('inflight', self.work, self.claim['token'], 'sandbox', 1)
        with patch.object(cli.gateway if hasattr(cli, 'gateway') else __import__('pipeline.execution', fromlist=['gateway']).gateway, 'login'), patch('pipeline.execution.gateway.inventory', return_value=[]):
            cli.discover_cleanup(self.store)
        self.assertEqual(self.store.db.execute('SELECT state FROM resources').fetchone()[0], 'intent')
        self.assertFalse(self.store.reserve('duplicate', self.work, self.claim['token'], 'sandbox', 1))

    def test_crash_after_publication_adopts_receipt_without_gateway(self):
        prior = self.store.directory / 'attempts' / self.work / '0'
        (prior / 'bundle').mkdir(parents=True)
        (prior / 'bundle/job.json').write_text(json.dumps({'stage': 'implement', 'task': 'task-001', 'branch': 'private'}))
        (prior / 'outcome.json').write_text(json.dumps({'published': True, 'head': 'source', 'valid': True}))
        execution = Execution(self.store, self.claim)
        with patch.object(execution, 'login', side_effect=AssertionError('must not allocate')):
            result = execution.cloud_step(self.store.task('task-001'), '')
        self.assertEqual(result['head'], 'source')
        self.assertEqual(json.loads((execution.directory / 'outcome.json').read_text())['head'], 'source')
        self.assertEqual(self.store.db.execute('SELECT count(*) FROM resources').fetchone()[0], 0)

    def test_failed_capture_still_purges_expensive_compute(self):
        self.store.reserve('pod', self.work, self.claim['token'], 'sandbox', 1)
        self.store.finish(self.work, self.claim['token'], {})
        ident = self.store.enqueue('cleanup', 'task-001', 'g', {'resource': 'pod'})
        claim = self.store.claim('cleanup', 'cleaner')
        execution = Execution(self.store, claim)
        item = {'name': 'pod', 'labels': {'gyre.dev/pipeline': self.store.setting('owner')}}
        with patch.object(execution, 'login'), patch('pipeline.stages.gateway.inventory', side_effect=[[item], [], []]), patch.object(execution, 'remote', side_effect=subprocess.TimeoutExpired('capture', 30)), patch.object(execution, 'os', return_value=subprocess.CompletedProcess([], 0, '', '')) as delete:
            result, _, _ = cleanup(execution, self.store.task('task-001'))
        self.assertEqual(result['deleted'], 'pod')
        delete.assert_called_once_with('sandbox', 'delete', 'pod', timeout=180, check=False)
        self.assertEqual(self.store.db.execute("SELECT state FROM resources WHERE name='pod'").fetchone()[0], 'absent')

    def merge_cleanup(self, observation):
        self.store.reserve('merge-old', self.work, self.claim['token'], 'merge', 1,
                           {'url': 'https://github.com/jsell-rh/gyre/pull/633'})
        self.store.db.execute("UPDATE work SET state='obsolete',token=token+1 WHERE id=?", (self.work,))
        self.store.enqueue('cleanup', 'task-001', 'g', {'resource': 'merge-old'})
        claim = self.store.claim('cleanup', 'cleaner')
        execution = Execution(self.store, claim)
        result = {'data': {'repository': {'pullRequest': observation}}}
        with patch.object(execution, 'command', return_value=subprocess.CompletedProcess([], 0, json.dumps(result), '')):
            return cleanup(execution, self.store.task('task-001'))

    def test_rejected_merge_does_not_permanently_block_other_deliveries(self):
        self.merge_cleanup({'state': 'OPEN', 'headRefOid': 'head', 'mergeCommit': None,
                            'autoMergeRequest': None, 'mergeQueueEntry': None})
        self.store.put_task('task-002', 'g', 'body', {})
        self.store.enqueue('publish', 'task-002', 'g', {})
        claim = self.store.claim('publish', 'next-publisher')
        self.assertTrue(self.store.reserve('merge-new', claim['id'], claim['token'], 'merge', 1))

    def test_queued_merge_retains_its_global_permit(self):
        from pipeline.execution import Wait
        with self.assertRaises(Wait):
            self.merge_cleanup({'state': 'OPEN', 'headRefOid': 'head', 'mergeCommit': None,
                                'autoMergeRequest': None, 'mergeQueueEntry': {'id': 'entry'}})
        self.assertEqual(self.store.db.execute("SELECT state FROM resources WHERE name='merge-old'").fetchone()[0], 'intent')

    def test_unknown_merge_observation_retains_its_global_permit(self):
        from pipeline.execution import Retry
        with self.assertRaises(Retry):
            self.merge_cleanup({'state': 'OPEN', 'headRefOid': 'head'})
        self.assertEqual(self.store.db.execute("SELECT state FROM resources WHERE name='merge-old'").fetchone()[0], 'intent')

    def test_implementer_cannot_weaken_its_assigned_contract(self):
        body = '---\ntitle: Thing\nspec_ref: behavior.md\ndepends_on: []\nprogress: not-started\n---\n\n## Required behavior\nReject foreign tenants.\n'
        self.store.put_task('task-001', 'g', body, {'dependencies': []})
        execution = Execution(self.store, self.claim)
        execution.claim['input'] = {'base': 'base'}
        execution.log.write_text('agent completed')
        weakened = body.replace('not-started', 'ready-for-review').replace('Reject foreign tenants.', 'Allow every tenant.')
        receipt = {'task_body': weakened, 'valid': True, 'head': 'candidate', 'base': 'base', 'branch': 'private', 'agent_exit': 0}
        with patch('pipeline.stages.prompt', return_value='assignment'), patch.object(execution, 'cloud_step', return_value=receipt):
            result, updates, _ = implement(execution, self.store.task('task-001'))
        self.assertEqual(result['task_body'], body)
        self.assertEqual(updates['repair']['category'], 'contract')
        self.assertIsNone(updates['review'])

    def test_reviewer_receives_original_ci_failure_after_repair_completes(self):
        body = '---\ntitle: Thing\nspec_ref: behavior.md\ndepends_on: []\nprogress: not-started\n---\n\n## Required behavior\nReject foreign tenants.\n'
        finding = {'category': 'ci', 'source': 'previous-head', 'detail': 'CI_FOREIGN_TENANT_FAILURE',
                   'candidate_log': str(self.store.directory / 'ci.log')}
        self.store.put_task('task-001', 'g', body,
                            {'dependencies': [], 'candidate_base': 'old-main', 'repair': finding})
        execution = Execution(self.store, self.claim)
        execution.claim['input'] = {'base': 'new-main'}
        receipt = {'task_body': body.replace('not-started', 'ready-for-review'), 'valid': True,
                   'head': 'fixed', 'base': 'new-main', 'branch': 'private', 'agent_exit': 0}
        with patch('pipeline.stages.prompt', return_value='assignment'), patch.object(execution, 'cloud_step', return_value=receipt):
            result, updates, findings = implement(execution, self.store.task('task-001'))
        self.store.finish(self.work, self.claim['token'], result, updates, findings)
        self.store.enqueue('review', 'task-001', 'g', {'candidate': 'fixed', 'base': 'new-main'})
        reviewer = Execution(self.store, self.store.claim('review', 'independent-reviewer'))
        self.assertIsNone(self.store.task('task-001')['data']['repair'])
        with patch.object(reviewer, 'command', return_value=subprocess.CompletedProcess([], 0, body, '')):
            assignment = prompt(reviewer, self.store.task('task-001'))
        self.assertIn('CI_FOREIGN_TENANT_FAILURE', assignment)
        self.assertIn('previous-head', assignment)

    def test_invalid_review_outcome_is_not_replayed_forever(self):
        prior = self.store.directory / 'attempts' / self.work / '0'
        (prior / 'bundle').mkdir(parents=True)
        (prior / 'bundle/job.json').write_text(json.dumps({'stage': 'review', 'task': 'task-001', 'branch': 'old'}))
        (prior / 'outcome.json').write_text(json.dumps({'published': True, 'head': 'source', 'valid': False}))
        claim = {**self.claim, 'stage': 'review'}
        execution = Execution(self.store, claim)
        with patch.object(execution, 'login', side_effect=RuntimeError('new model assignment required')):
            with self.assertRaisesRegex(RuntimeError, 'new model assignment required'):
                execution.cloud_step(self.store.task('task-001'), '')

    def test_cleanup_never_promotes_a_reviewers_checkpoint_to_implementation(self):
        self.store.reserve('finished-review', self.work, self.claim['token'], 'sandbox', 1)
        self.store.resource_state('finished-review', 'absent')
        self.store.db.execute("UPDATE work SET stage='review' WHERE id=?", (self.work,))
        self.store.retry(self.work, self.claim['token'], {'message': 'review host handoff interrupted'}, delay=0)
        prior = self.store.directory / 'attempts' / self.work / str(self.claim['token'])
        prior.mkdir(parents=True)
        (prior / 'recovery.json').write_text(json.dumps({'base': 'review-source'}))
        ident = self.store.enqueue('cleanup', 'task-001', 'g', {'resource': 'finished-review'}, priority=10000)
        claim = self.store.claim('cleanup', 'cleaner')
        execution = Execution(self.store, claim)
        with patch.object(execution, 'login'), patch('pipeline.stages.gateway.inventory', return_value=[]), patch('pipeline.stages.checkout', side_effect=AssertionError('must not promote review source')):
            _, updates, _ = cleanup(execution, self.store.task('task-001'))
        self.assertEqual(updates, {})
        self.assertIsNone(self.store.task('task-001')['data'].get('candidate'))

    def test_cleanup_captures_but_never_promotes_explicitly_fenced_source(self):
        self.store.reserve('fenced-pod', self.work, self.claim['token'], 'sandbox', 1)
        self.store.resource_state('fenced-pod', 'absent')
        self.store.db.execute("UPDATE work SET state='obsolete',token=token+1 WHERE id=?", (self.work,))
        prior = self.store.directory / 'attempts' / self.work / str(self.claim['token'])
        prior.mkdir(parents=True)
        (prior / 'recovery.json').write_text(json.dumps({'base': 'obsolete-source'}))
        self.store.enqueue('cleanup', 'task-001', 'g', {'resource': 'fenced-pod'}, priority=10000)
        claim = self.store.claim('cleanup', 'cleaner')
        execution = Execution(self.store, claim)
        with patch.object(execution, 'login'), patch('pipeline.stages.gateway.inventory', return_value=[]), patch('pipeline.stages.checkout', side_effect=AssertionError('must not promote fenced source')):
            _, updates, _ = cleanup(execution, self.store.task('task-001'))
        self.assertEqual(updates, {})

    def test_metadata_only_resubmission_does_not_resolve_a_real_review_finding(self):
        root = self.store.directory / 'source-proof'
        root.mkdir()
        def git(*args):
            return subprocess.check_output(['git', *args], cwd=root, text=True, stderr=subprocess.PIPE).strip()
        git('init', '-q', '-b', 'main')
        git('config', 'user.name', 'Test')
        git('config', 'user.email', 'test@example.com')
        git('config', 'commit.gpgsign', 'false')
        git('config', 'core.hooksPath', '/dev/null')
        body = '---\ntitle: Scope enforcement\nspec_ref: behavior.md\ndepends_on: []\nprogress: needs-revision\n---\n\n## Required behavior\nReject foreign tenants.\n'
        task_path = root / 'specs/tasks/task-001.md'
        task_path.parent.mkdir(parents=True)
        task_path.write_text(body)
        (root / 'production.rs').write_text('pub fn allowed() -> bool { true }\n')
        git('add', '.'); git('commit', '-qm', 'unrepaired candidate')
        base = git('rev-parse', 'HEAD')
        revised = body.replace('needs-revision', 'ready-for-review') + '\n## Shipped\n\n- Claimed to fix scope.\n'
        task_path.write_text(revised)
        git('add', '.'); git('commit', '-qm', 'notes only')
        head = git('rev-parse', 'HEAD')
        origin = self.store.directory / 'source-origin.git'
        subprocess.run(['git', 'clone', '--quiet', '--bare', str(root), str(origin)], check=True)
        git('remote', 'add', 'origin', str(origin))
        repair = {'category': 'review', 'source': base, 'id': 'finding', 'detail': 'Scope enforcement is absent.'}
        self.store.put_task('task-001', 'g', body, {'dependencies': [], 'candidate': base, 'candidate_base': base, 'repair': repair})
        execution = Execution(self.store, self.claim)
        execution.claim['input'] = {'base': base, 'candidate': base, 'repair': repair}
        receipt = {'task_body': revised, 'valid': True, 'head': head, 'base': base, 'branch': 'private', 'agent_exit': 0}
        with patch('pipeline.stages.prompt', return_value='assignment'), patch.object(execution, 'cloud_step', return_value=receipt), patch('pipeline.stages.checkout', return_value=root):
            from pipeline.execution import Retry
            with self.assertRaises(Retry) as raised:
                implement(execution, self.store.task('task-001'))
        self.assertTrue(raised.exception.fresh_model)
        self.assertEqual(self.store.task('task-001')['data']['repair'], repair)
        self.assertEqual(self.store.task('task-001')['data']['candidate'], base)


if __name__ == '__main__':
    unittest.main()
