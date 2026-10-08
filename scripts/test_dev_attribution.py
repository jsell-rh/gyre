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
            git('checkout', '-qb', 'feature')
            (root / 'crates/feature.rs').write_text('feature\n')
            old = commit('fix(task-001+002): repair implementation')
            (root / 'crates/other.rs').write_text('unrelated\n')
            unrelated = commit('feat(task-0010): another task')
            git('checkout', '-q', 'main')
            (root / 'advance.txt').write_text('main moved\n')
            commit('process: advance main')
            git('checkout', '-q', 'feature')
            git('rebase', 'main')
            rewritten = git('rev-parse', 'HEAD^')
            self.assertNotEqual(old, rewritten)
            subprocess.run(['python3', str(SCRIPT), 'task-001'], cwd=root, check=True)
            text = task.read_text()
            line = next(line for line in text.splitlines() if line.startswith('commits:'))
            self.assertEqual(set(json.loads(line.partition(':')[2])), {shipped, rewritten})
            self.assertNotIn(old, text)
            self.assertNotIn(unrelated, text)
            self.assertIn('depends_on: []\n---\nBody\n---\nMore body\n', text)
            subprocess.run(['python3', str(SCRIPT), 'task-001'], cwd=root, check=True)
            self.assertEqual(task.read_text(), text)


if __name__ == '__main__':
    unittest.main()
