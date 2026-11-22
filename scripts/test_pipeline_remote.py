"""Real Git checks for safe bookkeeping after detached candidate inspection."""
import importlib.util
import os
from pathlib import Path
import shutil
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

    def formatter(self):
        formatter = shutil.which('rustfmt')
        self.assertIsNotNone(formatter, 'Rust fixture checks require rustfmt')
        return formatter

    def normalize(self, base, formatter):
        return remote.normalize_new_rust(base, formatter,
                                         os.environ.get('RUSTUP_HOME', str(Path.home() / '.rustup')))

    def test_formatter_uses_installed_toolchain_when_native_exec_drops_image_environment(self):
        source = self.root / 'new.rs';source.write_text('fn created( ){ }\n')
        formatter = self.formatter()
        toolchain = os.environ.get('RUSTUP_HOME', str(Path.home() / '.rustup'))
        with patch.dict(os.environ, {'PATH': '/usr/bin:/bin', 'HOME': str(self.root),
                                     'RUSTUP_HOME': str(self.root / 'unconfigured')}, clear=True):
            result = remote.normalize_new_rust(self.head, formatter, toolchain)
        self.assertEqual(result, {'files': ['new.rs'], 'errors': []})
        self.assertEqual(source.read_text(), 'fn created() {}\n')

    def test_new_rust_is_formatted_across_a_later_metadata_only_commit(self):
        source = self.root / 'new.rs'
        source.write_text('fn created( ){println!("value");}\n')
        self.git('add', '.');self.git('commit', '-qm', 'source')
        (self.root / 'notes').write_text('bookkeeping')
        self.git('add', '.');self.git('commit', '-qm', 'metadata')
        head = self.git('rev-parse', 'HEAD')
        result = self.normalize(self.head, self.formatter())
        self.assertEqual(result, {'files': ['new.rs'], 'errors': []})
        self.assertEqual(source.read_text(), 'fn created() {\n    println!("value");\n}\n')
        self.assertEqual(self.git('rev-parse', 'HEAD'), head)  # finalizer owns commit/attribution
        self.assertEqual(self.normalize(self.head, self.formatter()), {'files': [], 'errors': []})

    def test_untracked_unicode_path_formats_without_changing_existing_debt(self):
        old = self.root / 'old.rs';old.write_text('fn legacy( ){ }\n')
        self.git('add', '.');self.git('commit', '-qm', 'baseline')
        base = self.git('rev-parse', 'HEAD')
        old.write_text('fn legacy( ){ } // existing-file edit\n')
        before = old.read_bytes()
        new = self.root / 'new file-λ.rs';new.write_text('fn added( ){ }\n')
        self.assertEqual(self.normalize(base, self.formatter()),
                         {'files': [new.name], 'errors': []})
        self.assertEqual(old.read_bytes(), before)
        self.assertEqual(new.read_text(), 'fn added() {}\n')

    def test_new_rust_cannot_follow_symlinks_or_modify_shared_files(self):
        with tempfile.TemporaryDirectory() as directory:
            outside = Path(directory) / 'outside.rs';outside.write_text('fn valuable( ){ }\n')
            original = outside.read_bytes()
            (self.root / 'link.rs').symlink_to(outside)
            os.link(outside, self.root / 'hard.rs')
            result = self.normalize(self.head, self.formatter())
            self.assertEqual(result['files'], [])
            self.assertEqual({item['path'] for item in result['errors']}, {'link.rs', 'hard.rs'})
            self.assertEqual(outside.read_bytes(), original)

    def test_formatter_failure_keeps_source_available_for_checkpoint_recovery(self):
        source = self.root / 'new.rs';source.write_text('fn preserved( ){ }\n')
        original = source.read_bytes()
        result = self.normalize(self.head, shutil.which('false'))
        self.assertEqual(result['files'], [])
        self.assertEqual(result['errors'], [{'path': 'new.rs', 'error': 'CalledProcessError'}])
        self.assertEqual(source.read_bytes(), original)
        self.assertEqual(self.git('rev-parse', 'HEAD'), self.head)


if __name__ == '__main__':
    unittest.main()
