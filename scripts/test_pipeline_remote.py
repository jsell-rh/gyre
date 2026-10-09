"""Real Git checks for safe bookkeeping after detached candidate inspection."""
import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('remote', Path(__file__).with_name('pipeline-remote.py'))
remote = importlib.util.module_from_spec(spec)
spec.loader.exec_module(remote)


class DetachedCandidateTest(unittest.TestCase):
    def test_checkout_retries_materialized_seed_in_same_worker(self):
        attempts = []
        def execute(*args):
            attempts.append(args)
            if len(attempts) == 2:
                raise subprocess.CalledProcessError(128, args)
            return subprocess.CompletedProcess(args, 0)
        with patch.object(remote, 'run', side_effect=execute), patch.object(remote.time, 'sleep') as sleep:
            remote.checkout_seed('private-branch', 'seed')
        self.assertEqual(attempts[2], ('git', 'fetch', '--quiet', '--refetch', '--no-filter', 'origin', 'main', 'seed'))
        self.assertEqual(attempts[-1], ('git', 'checkout', '-b', 'private-branch', 'seed'))
        sleep.assert_called_once_with(5)
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.previous = Path.cwd()
        os.chdir(self.root)
        self.addCleanup(os.chdir, self.previous)
        self.git('init', '-q', '-b', 'assigned')
        self.git('config', 'user.name', 'Test')
        self.git('config', 'user.email', 'test@example.com')
        self.git('config', 'commit.gpgsign', 'false')
        self.git('config', 'core.hooksPath', '/dev/null')
        (self.root / 'source').write_text('real code\n')
        self.git('add', '.')
        self.git('commit', '-qm', 'candidate')
        self.head = self.git('rev-parse', 'HEAD')
        self.git('checkout', '-q', '--detach')

    def git(self, *args):
        return subprocess.check_output(['git', *args], text=True, stderr=subprocess.PIPE).strip()

    def test_exact_candidate_reattaches_without_hiding_edits(self):
        (self.root / 'source').write_text('unreviewed edit\n')
        remote.reattach_candidate('assigned', self.root)
        self.assertEqual(self.git('branch', '--show-current'), 'assigned')
        self.assertEqual(self.git('rev-parse', 'HEAD'), self.head)
        self.assertEqual((self.root / 'source').read_text(), 'unreviewed edit\n')
        self.assertTrue(self.git('diff', '--name-only'))

    def test_different_source_stays_detached_and_fails_branch_guard(self):
        (self.root / 'source').write_text('different candidate\n')
        self.git('add', '.')
        self.git('commit', '-qm', 'other source')
        remote.reattach_candidate('assigned', self.root)
        self.assertEqual(self.git('branch', '--show-current'), '')
        self.assertNotEqual(self.git('rev-parse', 'HEAD'), self.head)

    def test_active_rebase_is_never_reattached(self):
        (self.root / '.git/rebase-merge').mkdir()
        remote.reattach_candidate('assigned', self.root)
        self.assertEqual(self.git('branch', '--show-current'), '')


if __name__ == '__main__':
    unittest.main()
