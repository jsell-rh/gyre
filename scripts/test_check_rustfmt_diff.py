#!/usr/bin/env python3
"""The formatting gate checks new lines while tolerating old formatting debt."""

from pathlib import Path
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("check-rustfmt-diff.py")


def run(cwd, *args):
    return subprocess.run(args, cwd=cwd, text=True, capture_output=True)


class RustfmtDiffTest(unittest.TestCase):
    def test_only_new_violations_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            assert run(root, "git", "init", "-q").returncode == 0
            run(root, "git", "config", "user.name", "Test")
            run(root, "git", "config", "user.email", "test@example.com")
            source = root / "sample.rs"
            source.write_text("fn old( ) {}\n")
            run(root, "git", "add", "sample.rs")
            run(root, "git", "commit", "-qm", "base")
            base = run(root, "git", "rev-parse", "HEAD").stdout.strip()

            source.write_text("fn old( ) {}\nfn fresh() {}\n")
            run(root, "git", "add", "sample.rs")
            run(root, "git", "commit", "-qm", "formatted addition")
            clean = run(root, "python3", str(SCRIPT), base)
            self.assertEqual(clean.returncode, 0, clean.stdout + clean.stderr)

            source.write_text("fn old( ) {}\nfn fresh( ) {}\n")
            run(root, "git", "add", "sample.rs")
            run(root, "git", "commit", "-qm", "unformatted addition")
            bad = run(root, "python3", str(SCRIPT), "HEAD^1")
            self.assertEqual(bad.returncode, 1, bad.stdout + bad.stderr)
            self.assertIn("sample.rs", bad.stdout)


if __name__ == "__main__":
    unittest.main()
