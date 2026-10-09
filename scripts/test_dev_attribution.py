"""Exercise attribution with actual Git history and a rewritten branch."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).with_name('dev-attribution.py')


class AttributionTest(unittest.TestCase):
    def test_rebase_replaces_stale_ids_without_losing_shipped_commits(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            def git(*args):
                return subprocess.check_output(['git', *args], cwd=root, text=True, stderr=subprocess.PIPE).strip()
            def commit(message):
                git('add', '.')
                git('commit', '-qm', message)
                return git('rev-parse', 'HEAD')
            git('init', '-q', '-b', 'main')
            git('config', 'user.name', 'Test')
            git('config', 'user.email', 'test@example.com')
            git('config', 'core.hooksPath', '/dev/null')
            git('config', 'commit.gpgsign', 'false')
            (root / 'crates').mkdir()
            (root / 'specs/tasks').mkdir(parents=True)
            task = root / 'specs/tasks/task-001.md'
            task.write_text('---\nprogress: needs-revision\ncommits:\n  - deadbeef\ndepends_on: []\n---\nBody\n---\nMore body\n')
            (root / 'crates/shipped.rs').write_text('shipped\n')
            shipped = commit('feat(task-001): shipped implementation')
            (root / 'crates/other.rs').write_text('unrelated\n')
            unrelated = commit('feat(task-0010): another task')
            git('checkout', '-qb', 'feature')
            (root / 'crates/feature.rs').write_text('feature\n')
            old = commit('fix(task-001+002): repair implementation')
            git('checkout', '-q', 'main')
            (root / 'advance.txt').write_text('main moved\n')
            commit('process: advance main')
            git('update-ref', 'refs/remotes/origin/main', 'HEAD')
            git('checkout', '-q', 'feature')
            git('rebase', 'main')
            rewritten = git('rev-parse', 'HEAD')
            self.assertNotEqual(old, rewritten)
            (root / 'crates/unlabeled.rs').write_text('previously invisible repair\n')
            unlabeled = commit('fix: production repair without task label')
            (root / 'crates/process.rs').write_text('production code in a process-labeled commit\n')
            process_product = commit('process: checkpoint production repair')
            subprocess.run(['python3', str(SCRIPT), 'task-001'], cwd=root, check=True)
            text = task.read_text()
            line = next(line for line in text.splitlines() if line.startswith('commits:'))
            self.assertEqual(set(json.loads(line.partition(':')[2])), {shipped, rewritten, unlabeled, process_product})
            self.assertNotIn(old, text)
            self.assertNotIn(unrelated, text)
            self.assertIn('depends_on: []\n---\nBody\n---\nMore body\n', text)
            subprocess.run(['python3', str(SCRIPT), 'task-001'], cwd=root, check=True)
            self.assertEqual(task.read_text(), text)
            (root / 'unrelated-staged.txt').write_text('preserve independently staged work\n')
            git('add', 'unrelated-staged.txt')
            subprocess.run(['python3', str(SCRIPT), 'task-001', '--commit'], cwd=root, check=True)
            self.assertEqual(git('show', 'HEAD:specs/tasks/task-001.md'), text.strip())
            self.assertEqual(git('show', '--format=', '--name-only', 'HEAD'), 'specs/tasks/task-001.md')
            self.assertEqual(git('diff', '--cached', '--name-only'), 'unrelated-staged.txt')
            checkpoint = git('rev-parse', 'HEAD')
            subprocess.run(['python3', str(SCRIPT), 'task-001', '--commit'], cwd=root, check=True)
            self.assertEqual(git('rev-parse', 'HEAD'), checkpoint)

    def test_blobless_history_needs_no_network_for_rename_attribution(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            origin, clone = root / 'origin', root / 'clone'
            origin.mkdir()
            def git(cwd, *args):
                return subprocess.check_output(['git', *args], cwd=cwd, text=True, stderr=subprocess.PIPE).strip()
            git(origin, 'init', '-q', '-b', 'main')
            git(origin, 'config', 'user.name', 'Test')
            git(origin, 'config', 'user.email', 'test@example.com')
            git(origin, 'config', 'core.hooksPath', '/dev/null')
            git(origin, 'config', 'commit.gpgsign', 'false')
            git(origin, 'config', 'uploadpack.allowFilter', 'true')
            (origin / 'crates').mkdir()
            (origin / 'specs/tasks').mkdir(parents=True)
            task = origin / 'specs/tasks/task-001.md'
            task.write_text('---\nprogress: needs-revision\ncommits: []\n---\n')
            old = origin / 'crates/old.rs'
            old.write_text(''.join(f'line {i}\n' for i in range(100)))
            git(origin, 'add', '.')
            git(origin, 'commit', '-qm', 'feat(task-001): original production code')
            initial = git(origin, 'rev-parse', 'HEAD')
            old.rename(origin / 'crates/new.rs')
            (origin / 'crates/new.rs').write_text(''.join(f'line {i}\n' for i in range(99)) + 'changed final line\n')
            git(origin, 'add', '.')
            git(origin, 'commit', '-qm', 'fix(task-001): rename production code')
            renamed = git(origin, 'rev-parse', 'HEAD')
            subprocess.run(['git', 'clone', '--quiet', '--filter=blob:none', origin.as_uri(), str(clone)], check=True)
            git(clone, 'config', 'remote.origin.url', (root / 'unavailable').as_uri())
            # Rename detection tries to fetch the old, absent blob. Attribution
            # must use paths/trees rather than content similarity.
            old_probe = subprocess.run(['git', 'log', '--name-only'], cwd=clone, capture_output=True)
            self.assertNotEqual(old_probe.returncode, 0)
            subprocess.run(['python3', str(SCRIPT), 'task-001'], cwd=clone, check=True)
            text = (clone / 'specs/tasks/task-001.md').read_text()
            commits = json.loads(next(line for line in text.splitlines() if line.startswith('commits:')).partition(':')[2])
            self.assertEqual(set(commits), {initial, renamed})


if __name__ == '__main__':
    unittest.main()
