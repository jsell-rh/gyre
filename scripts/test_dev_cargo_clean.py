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
            # A real path dependency models what the shared cache must keep:
            # artifacts of packages that are not workspace members of the
            # checkout running the helper. Its reported artifacts are tracked
            # directly instead of a sentinel file, because Cargo's layout for
            # a member's own artifacts (deps/ vs build/<pkg>/<hash>/out/) is
            # toolchain-dependent and those paths are legitimately cleaned.
            dependency = root / 'dep'
            (dependency / 'src').mkdir(parents=True)
            (dependency / 'Cargo.toml').write_text(
                '[package]\nname="gyre_cache_dep"\nversion="0.1.0"\nedition="2021"\n')
            (dependency / 'src/lib.rs').write_text(
                'pub fn retained() -> bool { true }\n')
            for name, value in [('good', 'true'), ('mutant', 'false')]:
                work = root / name
                (work / 'src').mkdir(parents=True)
                (work / 'Cargo.toml').write_text(
                    '[package]\nname="gyre_cache_probe"\nversion="0.1.0"\nedition="2021"\n'
                    '[dependencies]\ngyre_cache_dep = { path = "../dep" }\n')
                (work / 'src/lib.rs').write_text(
                    'fn scoped() -> bool { ' + value + ' }\n'
                    '#[test]\nfn requires_scope() { assert!(scoped()); }\n')
            env = {**os.environ, 'CARGO_TARGET_DIR': str(root / 'target')}
            helper = Path(__file__).with_name('dev-cargo-clean.py')
            dependency_artifacts = None
            member_rlib = None
            for name, expected in [('good', 0), ('mutant', 101), ('good', 0)]:
                cleaned = subprocess.run([sys.executable, str(helper)], cwd=root / name,
                                         env=env, capture_output=True, text=True, timeout=30)
                self.assertEqual(cleaned.returncode, 0, cleaned.stderr)
                if member_rlib is not None:
                    # The helper must invalidate the member's own cached
                    # artifacts; a stale rlib is exactly the masking hazard.
                    self.assertFalse(member_rlib.exists(),
                                     f'{member_rlib} survived the workspace clean')
                result = subprocess.run(['cargo', 'test', '-p', 'gyre_cache_probe', '--offline',
                                         '--quiet', '--message-format=json'], cwd=root / name,
                                        env=env, capture_output=True, text=True, timeout=120)
                self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
                artifacts = [json.loads(line) for line in result.stdout.splitlines() if line.startswith('{')]
                reports = {Path(filename): item for item in artifacts
                           if item.get('reason') == 'compiler-artifact'
                           and item.get('target', {}).get('name') == 'gyre_cache_dep'
                           for filename in item.get('filenames', [])}
                self.assertTrue(reports, result.stdout)
                if dependency_artifacts is None:
                    dependency_artifacts = reports
                else:
                    for item in reports.values():
                        self.assertTrue(item.get('fresh'),
                                        'dependency artifacts were dropped from the shared cache and rebuilt')
                for artifact in dependency_artifacts:
                    self.assertTrue(artifact.exists(), f'{artifact} missing from the shared cache')
                if member_rlib is None:
                    member_rlib = next(
                        Path(filename) for item in artifacts
                        if item.get('reason') == 'compiler-artifact'
                        and item.get('target', {}).get('name') == 'gyre_cache_probe'
                        for filename in item.get('filenames', [])
                        if filename.endswith('.rlib'))


if __name__ == '__main__':
    unittest.main()
