"""Prove a detached remote job survives a lost attachment without restarting."""
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time
import unittest


SCRIPTS = Path(__file__).resolve().parent


class AttachmentTest(unittest.TestCase):
    def test_disconnect_resumes_one_job_and_its_exact_exit_status(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ('dev-attach.py', 'dev-process.sh'):
                shutil.copy2(SCRIPTS / name, root / name)
            (root / 'dev-remote.sh').write_text(
                'cd "$(dirname "$0")"\necho run >> runs\necho started\n'
                'for i in $(seq 1 200); do [ ! -f release ] || break; sleep .05; done\necho finished\nexit 7\n')
            command = ['python3', str(root / 'dev-attach.py'), 'worker', 'task-001', 'branch', 'seed', 'id']
            capture = root / 'attachment.log'
            with capture.open('wb') as output:
                first = subprocess.Popen(command + ['0'], stdout=output, stderr=subprocess.PIPE)
                self.addCleanup(lambda: first.poll() is None and first.kill())
                deadline = time.monotonic() + 10
                while b'GYRE_REMOTE_LOG_OFFSET ' not in capture.read_bytes() and time.monotonic() < deadline:
                    time.sleep(.05)
                received = capture.read_bytes()
                self.assertIn(b'started\n', received)
                offset = re.findall(rb'GYRE_REMOTE_LOG_OFFSET (\d+)', received)[-1].decode()
                first.terminate()
                first.communicate(timeout=5)
            second = subprocess.Popen(command + [offset], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            (root / 'release').touch()
            output, error = second.communicate(timeout=10)
            self.assertEqual(second.returncode, 7, error.decode())
            self.assertNotIn(b'started\n', output)
            self.assertIn(b'finished\n', output)
            self.assertEqual((root / 'runs').read_text(), 'run\n')
            offset = re.findall(rb'GYRE_REMOTE_LOG_OFFSET (\d+)', output)[-1].decode()
            replay = subprocess.run(command + [offset], capture_output=True, timeout=5)
            self.assertEqual((replay.returncode, replay.stdout), (7, b''))
            self.assertEqual((root / 'runs').read_text(), 'run\n')

    def test_local_cursor_is_saved_without_polluting_agent_output(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'offset'
            result = subprocess.run(['python3', str(SCRIPTS / 'dev-log-cursor.py'), str(path)],
                                    input=b'first\nGYRE_REMOTE_LOG_OFFSET 6\nsecond\nGYRE_REMOTE_LOG_OFFSET 13\n',
                                    capture_output=True, check=True)
            self.assertEqual(result.stdout, b'first\nsecond\n')
            self.assertEqual(path.read_text(), '13\n')

    def test_completed_failure_requires_explicit_retry_and_reuses_log(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ('dev-attach.py', 'dev-process.sh'):
                shutil.copy2(SCRIPTS / name, root / name)
            (root / 'dev-remote.sh').write_text(
                'cd "$(dirname "$0")"\necho run >> runs\n'
                '[ "$(wc -l < runs)" -gt 1 ] || { echo registry-failed; exit 77; }\necho succeeded\n')
            command = ['python3', str(root / 'dev-attach.py'), 'worker', 'task-001', 'branch', 'seed', 'id', '0']
            first = subprocess.run(command, capture_output=True, timeout=5)
            self.assertEqual(first.returncode, 77)
            cached = subprocess.run(command, capture_output=True, timeout=5)
            self.assertEqual(cached.returncode, 77)
            self.assertEqual((root / 'runs').read_text(), 'run\n')
            retried = subprocess.run(command + ['--retry-failed'], capture_output=True, timeout=5)
            self.assertEqual(retried.returncode, 0, retried.stderr.decode())
            self.assertIn(b'succeeded', retried.stdout)
            self.assertIn('registry-failed', (root / 'remote.log').read_text())
            self.assertEqual((root / 'runs').read_text(), 'run\nrun\n')


if __name__ == '__main__':
    unittest.main()
