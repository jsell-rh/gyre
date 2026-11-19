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
                # Explicit package selection: the probe workspace has exactly
                # one member, so this compiles the same units as a bare
                # `cargo test` while staying a focused probe that constrained
                # dev environments (single-package build budgets) can run.
                result = subprocess.run(['cargo', 'test', '-p', 'gyre_cache_probe', '--offline', '--quiet', '--message-format=json'], cwd=root / name,
                                        env=env, capture_output=True, text=True, timeout=120)
                self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
                if sentinel is None:
                    artifacts = [json.loads(line) for line in result.stdout.splitlines() if line.startswith('{')]
                    executables = [Path(item['executable']) for item in artifacts
                                   if item.get('reason') == 'compiler-artifact' and item.get('executable')]
                    self.assertEqual(len(executables), 1, result.stdout)
                    # Target triples and Cargo's intermediate build layout can
                    # vary across toolchains; use Cargo's actual artifact path.
                    self.assertTrue(executables[0].is_relative_to(root / 'target'))
                    # The sentinel must represent an artifact of an UNRELATED
                    # package in the shared target dir: something `cargo clean
                    # --package` of the workspace member must never remove.
                    # Cargo 1.100 stabilized a new build-dir layout that puts a
                    # package's own artifacts (including test executables)
                    # under <profile>/build/<pkg>/<meta>/out/ and `cargo clean
                    # --package` removes that whole tree, so the member's own
                    # executable directory cannot host the sentinel. Both
                    # layouts always create a `build` directory inside the
                    # profile directory, so locate the profile directory that
                    # owns the executable and place the sentinel in a sibling
                    # package's build unit.
                    profile = None
                    for candidate in executables[0].parents:
                        if not candidate.is_relative_to(root / 'target'):
                            break
                        if (candidate / 'build').is_dir():
                            profile = candidate
                            break
                    self.assertIsNotNone(profile, result.stdout)
                    sentinel = profile / 'build' / 'unrelated-dependency' / 'out' / 'unrelated-dependency.rlib'
                    sentinel.parent.mkdir(parents=True, exist_ok=True)
                    sentinel.write_text('dependency cache retained')
                self.assertEqual(sentinel.read_text(), 'dependency cache retained')


if __name__ == '__main__':
    unittest.main()
