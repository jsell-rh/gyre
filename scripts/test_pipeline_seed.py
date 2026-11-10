"""Exercise operator reseeding through the real CLI and durable work store."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from pipeline.store import Store

ROOT = Path(__file__).resolve().parents[1]


class SeedTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.state = Path(self.temp.name)
        self.seed()

    def seed(self, *flags, check=True):
        return subprocess.run([sys.executable, str(ROOT / 'scripts/dev-pipeline.py'), '--state', str(self.state),
                               'seed', str(ROOT / 'specs/tasks/task-192.md'), *flags],
                              capture_output=True, text=True, check=check, timeout=30)

    def test_normal_reseed_preserves_metadata_proof_and_fresh_archives_checkpoint(self):
        s = Store(self.state)
        task = s.task('task-192')
        old = task['data'] | {'triaged': task['generation'], 'candidate': 'old-source'}
        s.put_task(task['name'], task['generation'], task['body'], old)
        self.seed()
        self.assertEqual(s.task('task-192')['data']['triaged'], task['generation'])
        s.put_task(task['name'], task['generation'], task['body'], old)
        self.seed('--fresh')
        current = s.task('task-192')
        self.assertEqual(current['generation'], task['generation'])
        self.assertIsNone(current['data'].get('candidate'))
        self.assertIsNone(current['data'].get('triaged'))
        first_id = current['data']['repair']['id']
        self.assertEqual(current['data']['repair']['category'], 'fresh_start')
        archive, = (self.state / 'task-resets').glob('*.json')
        self.assertEqual(json.loads(archive.read_text())['data']['candidate'], 'old-source')
        self.assertEqual(archive.stat().st_mode & 0o777, 0o600)
        self.seed('--fresh')
        self.assertNotEqual(s.task('task-192')['data']['repair']['id'], first_id)
        s.close()

    def test_fresh_start_refuses_active_or_delivered_work(self):
        s = Store(self.state)
        t = s.task('task-192')
        s.enqueue('triage', t['name'], t['generation'], {})
        claim = s.claim('triage', 'active-worker')
        result = self.seed('--fresh', check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('drain active work', result.stderr)
        s.finish(claim['id'], claim['token'], {}, {'delivered': {'sha': 'actual'}})
        result = self.seed('--fresh', check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('delivered task', result.stderr)
        s.close()


if __name__ == '__main__':
    unittest.main()
