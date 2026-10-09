import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from pipeline.recovery import restore, unchanged_bootstrap


class RecoveryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.remote = self.root / 'origin.git'
        self.repo = self.root / 'worker'
        self.artifact = self.root / 'artifact'
        self.artifact.mkdir()
        self.call('git', 'init', '--quiet', '--bare', str(self.remote))
        self.call('git', 'clone', '--quiet', str(self.remote), str(self.repo))
        self.git('config', 'user.name', 'Test')
        self.git('config', 'user.email', 'test@example.invalid')
        (self.repo / 'source.txt').write_text('base\n')
        self.git('add', '.')
        self.git('commit', '-qm', 'base')
        self.git('push', '-q', 'origin', 'HEAD:main')
        self.base = self.git('rev-parse', 'HEAD')
        self.branch = 'pipeline/task-001/' + 'a' * 32 + '-1'
        self.git('checkout', '-qb', self.branch)
        (self.repo / 'source.txt').write_text('real unfinished production change\n')
        self.git('add', '.')
        tree = self.git('write-tree')
        patch = subprocess.check_output(['git', 'diff', '--binary', '--cached', self.base], cwd=self.repo)
        (self.artifact / 'recovery.patch').write_bytes(patch)
        self.receipt = {'version': 1, 'base': self.base, 'head': self.base,
                        'tree': tree, 'branch': self.branch, 'published_known': True,
                        'published_head': '', 'stash_count': 0,
                        'patch_sha256': hashlib.sha256(patch).hexdigest()}
        self.save()

    def call(self, *args, **kw):
        return subprocess.check_output(args, stderr=subprocess.DEVNULL, **kw).decode().strip()

    def git(self, *args):
        return self.call('git', *args, cwd=self.repo)

    def save(self):
        (self.artifact / 'recovery.json').write_text(json.dumps(self.receipt))

    def test_recovery_restores_actual_source_and_replay_adopts_same_head(self):
        first = restore(self.artifact, self.repo)
        self.assertEqual(self.git('rev-parse', first['head'] + '^{tree}'), self.receipt['tree'])
        self.assertEqual(self.git('show', first['head'] + ':source.txt'), 'real unfinished production change')
        self.assertEqual(self.git('ls-remote', 'origin', self.branch).split()[0], first['head'])
        self.assertEqual(restore(self.artifact, self.repo)['head'], first['head'])

    def test_corrupted_patch_cannot_publish(self):
        with (self.artifact / 'recovery.patch').open('ab') as output:
            output.write(b'corruption')
        with self.assertRaises(ValueError):
            restore(self.artifact, self.repo)
        self.assertEqual(self.git('ls-remote', 'origin', self.branch), '')

    def test_empty_bootstrap_requires_matching_base_and_tree(self):
        tree = self.git('rev-parse', self.base + '^{tree}')
        self.receipt.update(branch='main', head=self.base, base=self.base, tree=tree,
                            published_known=False, patch_sha256=hashlib.sha256(b'').hexdigest())
        (self.artifact / 'recovery.patch').write_bytes(b'')
        self.save()
        self.assertTrue(unchanged_bootstrap(self.artifact, self.base, tree))
        self.assertFalse(unchanged_bootstrap(self.artifact, 'a' * 40, tree))
        self.assertFalse(unchanged_bootstrap(self.artifact, self.base, 'b' * 40))
        (self.artifact / 'recovery.patch').write_bytes(b'actual edit')
        self.assertFalse(unchanged_bootstrap(self.artifact, self.base, tree))

    def test_tree_mismatch_cannot_publish(self):
        self.receipt['tree'] = self.base
        self.save()
        with self.assertRaises(ValueError):
            restore(self.artifact, self.repo)
        self.assertEqual(self.git('ls-remote', 'origin', self.branch), '')

    def test_unknown_publish_lease_cannot_publish(self):
        self.receipt['published_known'] = False
        self.save()
        with self.assertRaises(ValueError):
            restore(self.artifact, self.repo)

    def test_external_push_is_not_overwritten(self):
        self.git('push', '-q', 'origin', 'HEAD:refs/heads/' + self.branch)
        with self.assertRaises(ValueError):
            restore(self.artifact, self.repo)
        self.assertEqual(self.git('ls-remote', 'origin', self.branch).split()[0], self.base)


if __name__ == '__main__':
    unittest.main()
