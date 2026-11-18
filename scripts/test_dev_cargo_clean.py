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
            for name, value in [('good', 'true'), ('mutant', 'false')]:
                work = root / name
                (work / 'src').mkdir(parents=True)
                (work / 'Cargo.toml').write_text(
                    '[package]\nname="gyre_cache_probe"\nversion="0.1.0"\nedition="2021"\n')
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
                                        env=env, capture_output=True, text=True, timeout=30)
                self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
                if sentinel is None:
                    artifacts = [json.loads(line) for line in result.stdout.splitlines() if line.startswith('{')]
                    executables = [Path(item['executable']) for item in artifacts
                                   if item.get('reason') == 'compiler-artifact' and item.get('executable')]
                    self.assertEqual(len(executables), 1, result.stdout)
                    # Target triples and Cargo's intermediate build layout can
                    # vary across toolchains; use Cargo's actual artifact path.
                    self.assertTrue(executables[0].is_relative_to(root / 'target'))
                    sentinel = executables[0].parent / 'unrelated-dependency.rlib'
                    sentinel.write_text('dependency cache retained')
                self.assertEqual(sentinel.read_text(), 'dependency cache retained')


if __name__ == '__main__':
    unittest.main()
