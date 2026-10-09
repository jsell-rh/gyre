import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


class RecoveryReceiptTest(unittest.TestCase):
    def test_receipt_binds_patch_to_real_index_and_published_head(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            repo = root / 'repo'
            repo.mkdir()
            def git(*args):
                return subprocess.check_output(['git', *args], cwd=repo, text=True).strip()
            git('init', '-q')
            git('config', 'user.name', 'test')
            git('config', 'user.email', 'test@example.com')
            (repo / 'feature.txt').write_text('before\n')
            git('add', '.')
            git('commit', '-qm', 'base')
            base = git('rev-parse', 'HEAD')
            git('update-ref', 'refs/remotes/origin/main', base)
            git('checkout', '-qb', 'pipeline/task-001/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-1')
            (repo / 'feature.txt').write_text('after\n')
            published = root / 'published'
            published.write_text(base + '\n')
            cli = root / 'openshell'
            cli.write_text("""#!/usr/bin/env python3
import os, subprocess, sys
command = sys.argv[-1].replace('/tmp/stage/push-expected', os.environ['RECOVERY_PUBLISHED'])
raise SystemExit(subprocess.call(['bash', '-c', command], cwd=os.environ['RECOVERY_REPO']))
""")
            cli.chmod(0o755)
            patch = root / 'recovery.patch'
            result = subprocess.run(['python3', str(Path(__file__).with_name('dev-recover.py')),
                                     'sandbox', str(patch)], capture_output=True, text=True,
                                    env={**os.environ, 'OPENSHELL': str(cli), 'RECOVERY_REPO': str(repo),
                                         'RECOVERY_PUBLISHED': str(published)}, timeout=15)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            receipt = json.loads(patch.with_suffix('.json').read_text())
            self.assertEqual(receipt['base'], base)
            self.assertEqual(receipt['head'], base)
            self.assertEqual(receipt['tree'], git('write-tree'))
            self.assertEqual(receipt['branch'], 'pipeline/task-001/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-1')
            self.assertTrue(receipt['published_known'])
            self.assertEqual(receipt['published_head'], base)
            self.assertEqual(receipt['patch_sha256'], hashlib.sha256(patch.read_bytes()).hexdigest())
            self.assertEqual(receipt['stash_count'], 0)
            self.assertEqual(patch.with_suffix('.json').stat().st_mode & 0o777, 0o600)
            self.assertIn(b'+after', patch.read_bytes())


if __name__ == '__main__':
    unittest.main()
