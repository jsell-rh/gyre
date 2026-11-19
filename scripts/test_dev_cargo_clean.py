"""Prove a shared Cargo cache cannot hide a broken workspace implementation."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


class CargoArtifactTest(unittest.TestCase):
    def test_invalid_manifest_preserves_code_failure_and_diagnostics(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / 'Cargo.toml').write_text('[package\ninvalid manifest\n')
            result = subprocess.run([sys.executable, str(Path(__file__).with_name('dev-cargo-clean.py'))],
                                    cwd=root, env={**os.environ, 'CARGO_TERM_COLOR': 'always'},
                                    capture_output=True, text=True, timeout=30)
            self.assertEqual(result.returncode, 101, result.stdout + result.stderr)
            self.assertIn('error:', result.stderr)
            self.assertNotIn('\x1b[', result.stderr)

    def test_rebuilds_current_checkout_and_retains_dependency_artifacts(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            # A real path dependency plays the shared-cache third party: the
            # helper must invalidate the checkout's own units while retaining
            # the dependency's compiled artifacts in the shared target dir.
            dep = root / 'dep'
            (dep / 'src').mkdir(parents=True)
            (dep / 'Cargo.toml').write_text(
                '[package]\nname="unrelated_dependency"\nversion="0.1.0"\nedition="2021"\n')
            (dep / 'src/lib.rs').write_text('pub fn retained() -> bool { true }\n')
            for name, value in [('good', 'true'), ('mutant', 'false')]:
                work = root / name
                (work / 'src').mkdir(parents=True)
                (work / 'Cargo.toml').write_text(
                    '[package]\nname="gyre_cache_probe"\nversion="0.1.0"\nedition="2021"\n\n'
                    '[dependencies]\nunrelated_dependency = { path = "../dep" }\n')
                (work / 'src/lib.rs').write_text(
                    'fn scoped() -> bool { ' + value + ' }\n'
                    '#[test]\nfn requires_scope() { assert!(scoped()); }\n')
            env = {**os.environ, 'CARGO_TARGET_DIR': str(root / 'target')}
            helper = Path(__file__).with_name('dev-cargo-clean.py')
            sentinel = None
            for name, expected in [('good', 0), ('mutant', 101), ('good', 0)]:
                cleaned = subprocess.run([sys.executable, str(helper)], cwd=root / name,
                                         env=env, capture_output=True, text=True, timeout=30)
                self.assertEqual(cleaned.returncode, 0, cleaned.stderr)
                result = subprocess.run(['cargo', 'test', '--offline', '--quiet', '--message-format=json'], cwd=root / name,
                                        env=env, capture_output=True, text=True, timeout=120)
                self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
                if sentinel is None:
                    artifacts = [json.loads(line) for line in result.stdout.splitlines() if line.startswith('{')]
                    # Cargo 1.100 moved intermediate artifacts to a build-unit
                    # layout (build/<pkg>/<meta>/out/), and `cargo clean
                    # --package` removes a cleaned package's build-unit
                    # directory wholesale, so a sentinel placed next to the
                    # checkout's own executable does not survive. Locate the
                    # dependency's artifacts through Cargo's JSON messages
                    # instead of assuming a fixed directory layout.
                    dependencies = [Path(filename) for item in artifacts
                                    if item.get('reason') == 'compiler-artifact'
                                    and item.get('target', {}).get('name') == 'unrelated_dependency'
                                    for filename in item.get('filenames', [])
                                    if filename.endswith('.rlib')]
                    self.assertEqual(len(dependencies), 1, result.stdout)
                    self.assertTrue(dependencies[0].is_relative_to(root / 'target'))
                    # The sentinel name shares the dependency's crate-name
                    # prefix so a helper that wrongly cleans the dependency's
                    # package (not just workspace members) cannot keep it.
                    sentinel = dependencies[0].parent / 'unrelated_dependency-sentinel.rlib'
                    sentinel.write_text('dependency cache retained')
                self.assertEqual(sentinel.read_text(), 'dependency cache retained')


if __name__ == '__main__':
    unittest.main()
