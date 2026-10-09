"""Prove a shared Cargo cache cannot hide a broken workspace implementation."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


class CargoArtifactTest(unittest.TestCase):
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
            sentinel = root / 'target/debug/deps/unrelated-dependency.rlib'
            for name, expected in [('good', 0), ('mutant', 101), ('good', 0)]:
                cleaned = subprocess.run([sys.executable, str(helper)], cwd=root / name,
                                         env=env, capture_output=True, text=True, timeout=30)
                self.assertEqual(cleaned.returncode, 0, cleaned.stderr)
                result = subprocess.run(['cargo', 'test', '--offline', '--quiet'], cwd=root / name,
                                        env=env, capture_output=True, text=True, timeout=30)
                self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
                if name == 'good':
                    sentinel.write_text('dependency cache retained')
                self.assertTrue(sentinel.exists())


if __name__ == '__main__':
    unittest.main()
