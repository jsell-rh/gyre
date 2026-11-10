"""Build admission must span commands and permit recursive build scripts."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


class BuildCommandTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        wrapper = Path(__file__).with_name('dev-build-command.sh').resolve()
        for name in ('cargo', 'npm', 'npx'):
            (self.root / name).symlink_to(wrapper)
        self.real = self.root / 'real-command'
        self.real.write_text('''#!/usr/bin/env python3
import os, pathlib, subprocess, sys, time
root = pathlib.Path(os.environ['BUILD_TEST_ROOT'])
mode = sys.argv[1]
with (root / 'calls').open('a') as log: log.write(mode + '\\n')
print(mode, flush=True)
if mode == 'hold':
    deadline = time.monotonic() + 10
    while not (root / 'release').exists() and time.monotonic() < deadline: time.sleep(.01)
elif mode == 'nested':
    sys.exit(subprocess.call([str(root / 'npm'), 'exit7']))
elif mode == 'exit7':
    sys.exit(7)
''')
        self.real.chmod(0o755)
        self.env = {**os.environ, 'BUILD_TEST_ROOT': str(self.root),
                    'GYRE_DEV_BUILD_LOCK': str(self.root / 'build.lock')}
        self.env.pop('GYRE_DEV_BUILD_LOCK_HELD', None)
        for name in ('CARGO', 'NPM', 'NPX'):
            self.env['GYRE_DEV_REAL_' + name] = str(self.real)

    def process(self, command, argument):
        child = subprocess.Popen([str(self.root / command), argument], env=self.env,
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        self.addCleanup(lambda: child.kill() if child.poll() is None else None)
        return child

    def test_single_job_budget_wins_over_explicit_agent_parallelism(self):
        self.real.write_text("#!/usr/bin/env python3\nimport os,sys,json; print(json.dumps({'args':sys.argv[1:],'jobs':os.environ['CARGO_BUILD_JOBS'],'threads':os.environ['RUST_TEST_THREADS']}))\n")
        import json
        for flags in (['-j','8'],['--jobs=8'],['-j8']):
            result=subprocess.run([str(self.root/'cargo'),'test',*flags,'--','-j8'],env=self.env,capture_output=True,text=True,check=True)
            value=json.loads(result.stdout)
            self.assertEqual(value['jobs'],'1')
            self.assertEqual(value['threads'],'1')
            self.assertEqual(value['args'],['test','--','-j8'])
        result=subprocess.run([str(self.root/'npm'),'test'],env=self.env,capture_output=True,text=True,check=True)
        self.assertEqual(json.loads(result.stdout)['args'],['test','--','--no-file-parallelism','--maxWorkers=1'])

    def test_other_build_waits_and_retains_actual_exit(self):
        first = self.process('cargo', 'hold')
        self.assertEqual(first.stdout.readline().strip(), 'hold')
        second = self.process('npx', 'exit7')
        with self.assertRaises(subprocess.TimeoutExpired):
            second.wait(timeout=.1)
        self.assertEqual((self.root / 'calls').read_text(), 'hold\n')
        (self.root / 'release').touch()
        _, error = first.communicate(timeout=5)
        self.assertEqual(first.returncode, 0, error)
        output, error = second.communicate(timeout=5)
        self.assertEqual(second.returncode, 7, error)
        self.assertEqual(output.strip(), 'exit7')

    def test_recursive_build_command_does_not_deadlock(self):
        child = self.process('cargo', 'nested')
        _, error = child.communicate(timeout=5)
        self.assertEqual(child.returncode, 7, error)
        self.assertEqual((self.root / 'calls').read_text(), 'nested\nexit7\n')

    def test_lean_compiler_settings_override_inherited_agent_environment(self):
        import json
        self.env.update(CARGO_INCREMENTAL='1',RUSTFLAGS='-C debuginfo=2',CARGO_PROFILE_TEST_DEBUG='2')
        self.real.write_text("#!/usr/bin/env python3\nimport os,json;print(json.dumps({k:os.environ[k] for k in ('CARGO_INCREMENTAL','RUSTFLAGS','CARGO_PROFILE_TEST_DEBUG','CARGO_PROFILE_TEST_INCREMENTAL')}))\n")
        result=subprocess.run([str(self.root/'cargo'),'test'],env=self.env,capture_output=True,text=True,check=True)
        value=json.loads(result.stdout)
        self.assertEqual(value['CARGO_INCREMENTAL'],'0')
        self.assertEqual(value['CARGO_PROFILE_TEST_INCREMENTAL'],'false')
        self.assertEqual(value['CARGO_PROFILE_TEST_DEBUG'],'0')
        self.assertIn('--threads=1',value['RUSTFLAGS'])
        self.assertNotIn('debug-assertions',value['RUSTFLAGS'])

    def test_review_rejects_workspace_suite_before_starting_a_build_but_permits_focused_tests(self):
        self.env['GYRE_PIPELINE_STAGE']='review'
        for args in (['test','--all'],['build','--workspace'],['clippy','--all-targets']):
            result=subprocess.run([str(self.root/'cargo'),*args],env=self.env,capture_output=True,text=True)
            self.assertEqual(result.returncode,64)
        self.assertFalse((self.root/'calls').exists())
        focused=subprocess.run([str(self.root/'cargo'),'test','-p','gyre-domain','--','scope_check'],
                               env=self.env,capture_output=True,text=True)
        self.assertEqual(focused.returncode,0,focused.stderr)
        self.assertEqual((self.root/'calls').read_text(),'test\n')


if __name__ == '__main__':
    unittest.main()
