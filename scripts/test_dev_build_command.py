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


if __name__ == '__main__':
    unittest.main()
