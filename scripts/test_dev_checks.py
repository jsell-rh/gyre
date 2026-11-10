"""Execute integration gates against Git fixtures that try to weaken verification."""
from pathlib import Path
import os
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("dev-check.sh")


class CheckIntegrityTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="gyre-gate-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.work = self.root / "work"
        self.stage = self.root / "stage"
        self.work.mkdir()
        self.stage.mkdir()
        self.git("init", "-q")
        self.git("config", "user.name", "Test")
        self.git("config", "user.email", "test@example.com")
        self.git("config", "commit.gpgsign", "false")
        self.git("config", "core.hooksPath", "/dev/null")
        scripts = self.work / "scripts"
        scripts.mkdir()
        # Preserve the gate runner; isolate compiler and product checks. The
        # test concerns which committed verifier runs, rather than Rust itself.
        import re
        checks = set(re.findall(r"check-[a-z-]+", SCRIPT.read_text()))
        for check in checks:
            (scripts / f"{check}.sh").write_text("exit 0\n")
        (scripts / "check-arch.sh").write_text("grep -qx safe production.txt || { echo 'unsafe production rejected' >&2; exit 1; }\n")
        (scripts / "example-exemptions.txt").write_text("existing-violation\n")
        (self.work / "production.txt").write_text("safe\n")
        (self.work / "web/dist").mkdir(parents=True)
        (self.work / "web/dist/index.html").write_text('build fixture\n')
        tools = self.root / 'tools'
        tools.mkdir()
        npm = tools / 'npm'
        npm.write_text('#!/bin/sh\nexit 0\n')
        npm.chmod(0o755)
        self.env = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ['PATH'])
        self.commit("base")
        for name in ("check-rustfmt-diff.py", "check-clippy-diff.py"):
            (self.stage / name).write_text("raise SystemExit(0)\n")
        (self.stage / 'dev-static-gate.py').write_text(Path(__file__).with_name('dev-static-gate.py').read_text())
        (self.stage / 'verifier-script-overlay.py').write_text(Path(__file__).with_name('verifier-script-overlay.py').read_text())
        # Only filesystem locations are redirected; the actual gate runner is
        # executed, including Git restoration and the exemption comparison.
        self.runner = self.stage / "dev-check.sh"
        self.runner.write_text(SCRIPT.read_text().replace("/tmp/gyre", str(self.work))
                               .replace("/tmp/stage", str(self.stage)))

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.work, text=True, stderr=subprocess.PIPE).strip()

    def commit(self, message):
        self.git("add", ".")
        self.git("commit", "-qm", message)

    def test_candidate_cannot_replace_failing_upstream_gate_with_pass(self):
        (self.work / "production.txt").write_text("unsafe\n")
        check = self.work / "scripts/check-arch.sh"
        check.write_text("exit 0\n")
        self.commit("weaken gate")
        result = subprocess.run(["bash", str(self.runner)], capture_output=True, text=True, env=self.env)
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(check.read_text(), "exit 0\n", "candidate tree was not restored after failure")
        self.assertEqual(self.git("status", "--porcelain"), "")

    def test_candidate_deleted_upstream_script_does_not_contaminate_checkout(self):
        old=self.work/'scripts/python-callgraph.py'
        old.write_text('print("old extractor")\n')
        self.commit('upstream extractor')
        old.unlink()
        self.commit('candidate removes obsolete extractor')
        result=subprocess.run(['bash',str(self.runner)],capture_output=True,text=True,env=self.env)
        self.assertEqual(result.returncode,0,result.stdout+result.stderr)
        self.assertFalse(old.exists())
        self.assertEqual(self.git('status','--porcelain'),'')

    def test_overlay_does_not_erase_unexpected_mutation_of_a_restored_file(self):
        old=self.work/'scripts/python-callgraph.py'
        old.write_text('print("old extractor")\n')
        self.commit('upstream extractor')
        old.unlink()
        self.commit('candidate removes extractor')
        helper=self.stage/'verifier-script-overlay.py';manifest=self.stage/'overlay.json'
        import sys
        subprocess.run([sys.executable,str(helper),'begin',str(manifest)],cwd=self.work,check=True)
        old.write_text('unexpected generated source\n')
        result=subprocess.run([sys.executable,str(helper),'finish',str(manifest)],cwd=self.work,capture_output=True,text=True)
        self.assertEqual(result.returncode,77)
        self.assertIn('modified',result.stdout)
        self.assertEqual(old.read_text(),'unexpected generated source\n')

    def test_whitespace_failure_requests_implementation_repair(self):
        (self.work / "review.md").write_text("review with trailing whitespace  \n")
        self.commit("candidate with malformed review")
        result = subprocess.run(["bash", str(self.runner)], capture_output=True, text=True, env=self.env)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("trailing whitespace", result.stdout)

    def test_reports_multiple_candidate_defects_in_one_check(self):
        (self.work / 'production.txt').write_text('unsafe\n')
        (self.work / 'review.md').write_text('trailing whitespace  \n')
        self.commit('two candidate defects')
        result = subprocess.run(['bash', str(self.runner)], capture_output=True, text=True, env=self.env)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn('trailing whitespace', result.stdout)
        self.assertEqual(self.git('status', '--porcelain'), '')
        self.assertIn('unsafe production rejected', result.stderr)

    def test_replacing_an_exemption_at_the_same_count_is_rejected(self):
        (self.work / "scripts/example-exemptions.txt").write_text("new-violation\n")
        self.commit("replace exemption")
        result = subprocess.run(["bash", str(self.runner)], capture_output=True, text=True, env=self.env)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("new verification exemptions forbidden", result.stderr)
        self.assertIn("new-violation", result.stderr)

    def test_existing_main_gate_failure_is_a_scoped_baseline_defect(self):
        (self.work / 'production.txt').write_text('unsafe\n')
        self.commit('existing baseline defect')
        (self.work / 'unrelated.txt').write_text('feature\n')
        self.commit('unrelated feature')
        result = subprocess.run(['bash', str(self.runner)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 81, result.stdout + result.stderr)
        self.assertIn('GYRE_BASELINE_FAILURE_JSON', result.stdout)
        self.assertEqual(self.git('status', '--porcelain'), '')

    def test_candidate_download_failure_is_infrastructure_without_baseline_probe(self):
        probe = self.root / 'network-probe.py'
        probe.write_text('import sys\nfrom pathlib import Path\n'
                         f'p=Path({str(self.root / "invocations")!r})\n'
                         'p.write_text(p.read_text()+"called\\n" if p.exists() else "called\\n")\n'
                         'print("error: Could not connect to server", file=sys.stderr)\nraise SystemExit(1)\n')
        import sys
        result = subprocess.run([sys.executable, str(self.stage / 'dev-static-gate.py'), self.git('rev-parse', 'HEAD'),
                                 sys.executable, str(probe)], cwd=self.work, capture_output=True, text=True)
        self.assertEqual(result.returncode, 77, result.stdout + result.stderr)
        self.assertEqual((self.root / 'invocations').read_text(), 'called\n')
        self.assertNotIn('GYRE_BASELINE_FAILURE_JSON', result.stdout)


if __name__ == "__main__":
    unittest.main()
