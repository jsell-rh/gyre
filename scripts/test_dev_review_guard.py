"""Exercise review isolation against committed, untracked, and stashed edits."""
from pathlib import Path
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).with_name('dev-review-guard.py')


class ReviewGuardTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        root = Path(self.temp.name)
        self.work = root / 'work'
        self.work.mkdir()
        self.state = root / 'review.json'
        self.git('init', '-q', '-b', 'pipeline/task-001/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-1')
        self.git('config', 'user.name', 'Test')
        self.git('config', 'user.email', 'test@example.com')
        self.git('config', 'commit.gpgsign', 'false')
        self.git('config', 'core.hooksPath', '/dev/null')
        self.task = self.work / 'specs/tasks/task-001.md'
        self.task.parent.mkdir(parents=True)
        self.task.write_text('---\nprogress: ready-for-review\n---\nRequired behavior\n')
        self.source = self.work / 'verifier.sh'
        self.source.write_text('exit 0\n')
        self.git('add', '.')
        self.git('commit', '-qm', 'baseline')
        self.guard('snapshot', '--task', 'task-001')

    def git(self, *args):
        return subprocess.check_output(['git', *args], cwd=self.work, text=True, stderr=subprocess.PIPE).strip()

    def guard(self, action, *args):
        return subprocess.run(['python3', str(SCRIPT), action, str(self.state), *args],
                              cwd=self.work, capture_output=True, text=True, timeout=15)

    def test_task_verdict_and_own_review_notes_are_allowed(self):
        self.task.write_text(self.task.read_text().replace('ready-for-review', 'complete'))
        review = self.work / 'specs/reviews/task-001.md'
        review.parent.mkdir(parents=True)
        review.write_text('concrete findings\n')
        self.assertEqual(self.guard('check').returncode, 0)
        self.assertIn('progress: complete', self.task.read_text())

    def test_committed_verifier_edits_and_untracked_source_reopen_task(self):
        self.source.write_text('exit 1\n')
        self.git('add', '.')
        self.git('commit', '-qm', 'reviewer changed verifier')
        (self.work / 'new.rs').write_text('new source\n')
        self.task.write_text(self.task.read_text().replace('ready-for-review', 'complete'))
        result = self.guard('check')
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn('progress: needs-revision', self.task.read_text())
        self.assertIn('verifier.sh', result.stdout)
        self.assertIn('new.rs', result.stdout)
        self.assertEqual(self.source.read_text(), 'exit 1\n', 'implementation work was lost')

    def test_stashed_edits_cannot_hide_from_the_guard(self):
        self.source.write_text('exit 1\n')
        self.git('stash', 'push', '-m', 'review probe')
        self.assertEqual(self.source.read_text(), 'exit 0\n')
        result = self.guard('check')
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn('progress: needs-revision', self.task.read_text())
        self.assertEqual(self.source.read_text(), 'exit 1\n')
        self.assertEqual(self.git('stash', 'list'), '')

    def test_prior_implementation_stash_is_visible_before_review(self):
        self.source.write_text('exit 1\n')
        self.git('stash', 'push', '-m', 'prior implementation work')
        result = self.guard('snapshot', '--task', 'task-001')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.source.read_text(), 'exit 1\n')
        self.assertEqual(self.guard('check').returncode, 0)


if __name__ == '__main__':
    unittest.main()
