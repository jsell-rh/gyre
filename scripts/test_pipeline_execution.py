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
from pipeline.stages import cleanup

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


if __name__ == '__main__':
    unittest.main()
