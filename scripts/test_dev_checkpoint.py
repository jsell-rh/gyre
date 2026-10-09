import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('checkpoint', Path(__file__).with_name('dev-checkpoint.py'))
checkpoint = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checkpoint)


class InterruptedBaselineTest(unittest.TestCase):
    def test_hidden_production_work_is_restored_without_touching_foreign_stash(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            def git(*args):
                return subprocess.check_output(['git', *args], cwd=root, text=True, stderr=subprocess.PIPE).strip()
            git('init', '-q', '-b', 'main')
            git('config', 'user.name', 'Test')
            git('config', 'user.email', 'test@example.com')
            git('config', 'core.hooksPath', '/dev/null')
            git('config', 'commit.gpgsign', 'false')
            (root / 'production.rs').write_text('old implementation\n')
            git('add', '.'); git('commit', '-qm', 'initial')
            (root / 'production.rs').write_text('foreign changes\n')
            git('stash', 'push', '-qm', 'foreign')
            foreign = git('rev-parse', 'stash')
            branch = 'devloop/task-001/attempt-1'
            git('checkout', '-qb', branch)
            (root / 'production.rs').write_text('real production repair\n')
            git('stash', 'push', '-qm', 'baseline probe')
            (root / 'new-test.txt').write_text('new regression test\n')
            subprocess.run(['python3', str(Path(checkpoint.__file__).resolve()), branch], cwd=root, check=True)
            self.assertEqual((root / 'production.rs').read_text(), 'real production repair\n')
            self.assertEqual((root / 'new-test.txt').read_text(), 'new regression test\n')
            self.assertEqual(git('rev-parse', 'stash'), foreign)
            # A second checkpoint must not duplicate or reapply recovered work.
            subprocess.run(['python3', str(Path(checkpoint.__file__).resolve()), branch], cwd=root, check=True)
            git('add', '.'); git('commit', '-qm', 'checkpoint')
            self.assertEqual(git('show', 'HEAD:production.rs'), 'real production repair')

    def test_wrong_branch_is_not_silently_published_as_task_work(self):
        with tempfile.TemporaryDirectory() as directory:
            result = subprocess.run(['git', 'init', '-q', '-b', 'main', directory], check=True)
            result = subprocess.run(['python3', str(Path(checkpoint.__file__).resolve()), 'devloop/task-001/attempt-1'], cwd=directory, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('assigned branch', result.stderr)
