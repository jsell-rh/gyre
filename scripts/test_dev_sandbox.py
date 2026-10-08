#!/usr/bin/env python3
"""Exercise transport reconnection without provisioning a second sandbox."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


class SandboxTransportTest(unittest.TestCase):
    def test_failed_push_saves_patch_before_deleting_sandbox(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "attempts" / "0123456789abcdef").mkdir(parents=True)
            (root / "attempts" / "0123456789abcdef" / "repair.md").write_text("repair the rejected candidate\n")
            for name in ("models.yml", "config.yml"):
                (root / name).write_text("test: true\n")
            fake = root / "openshell"
            fake.write_text("""#!/usr/bin/env python3
import base64, io, os, pathlib, sys, tarfile
args = sys.argv[1:]
record = pathlib.Path(os.environ['GYRE_FAKE_RECORD'])
if args[:2] == ['gateway', 'login']:
    sys.exit(0)
if 'create' in args:
    assert 'gyre-enmaas' in args and 'gyre-pricetag' not in args
    record.open('a').write('create\\n')
elif 'get' in args:
    print('Phase: Ready')
elif 'exec' in args and '/tmp/stage/dev-remote.sh' in args:
    print('GYRE_BOOTSTRAP_COMPLETE task=task-001')
    record.open('a').write('remote\\n')
    sys.exit(76)
elif 'exec' in args and 'GYRE_RECOVERY_BEGIN' in args[-1]:
    record.open('a').write('recover\\n')
    print('GYRE_RECOVERY_BEGIN')
    print(base64.b64encode(b'diff --git a/file b/file\\n').decode())
    print('GYRE_RECOVERY_END')
elif 'exec' in args:
    with tarfile.open(fileobj=io.BytesIO(sys.stdin.buffer.read())) as bundle:
        models = bundle.extractfile('./models.yml').read().decode()
        config = bundle.extractfile('./config.yml').read().decode()
        repair = bundle.extractfile('./repair.md').read().decode()
    assert 'https://api.enmaas.devshift.net/v1' in models
    assert 'api: openai-completions' in models
    assert 'supportsDeveloperRole: false' in models
    assert 'defaultProvider: enmaas-glm-5-3' in config
    assert repair == 'repair the rejected candidate\\n'
    record.open('a').write('stage\\n')
elif 'delete' in args:
    record.open('a').write('delete\\n')
""")
            fake.chmod(0o755)
            env = {**os.environ, "OPENSHELL": str(fake), "OPENSHELL_OIDC_CLIENT_SECRET": "test",
                   "GYRE_FAKE_RECORD": str(root / "calls"), "GYRE_DEV_STATE": str(root)}
            env.pop("MODELS_YML", None)
            env.pop("CONFIG_YML", None)
            result = subprocess.run(
                ["bash", str(Path(__file__).with_name("dev-sandbox.sh")), "worker", "task-001",
                 "devloop/task-001/attempt-1", "origin/main", "0123456789abcdef"],
                env=env, capture_output=True, text=True, timeout=30,
            )
            self.assertEqual(result.returncode, 76, result.stdout + result.stderr)
            self.assertEqual((root / "attempts" / "0123456789abcdef" / "recovery.patch").read_bytes(),
                             b"diff --git a/file b/file\n")
            self.assertEqual((root / "calls").read_text().splitlines(),
                             ["create", "stage", "remote", "recover", "delete"])

    def test_failed_worker_patch_is_saved_locally(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fake = root / "openshell"
            fake.write_text("""#!/usr/bin/env python3
import base64, sys
print('gateway notice')
print('GYRE_RECOVERY_BEGIN')
print(base64.b64encode(b'diff --git a/file b/file\\n').decode())
print('GYRE_RECOVERY_END')
""")
            fake.chmod(0o755)
            output = root / "recovery.patch"
            result = subprocess.run(
                ["python3", str(Path(__file__).with_name("dev-recover.py")),
                 "gyre-001-w-deadbeef", str(output)],
                env={**os.environ, "OPENSHELL": str(fake)},
                capture_output=True, text=True, timeout=10,
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertEqual(output.read_bytes(), b"diff --git a/file b/file\n")

    def test_transport_reconnects_inside_one_sandbox(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "attempts" / "0123456789abcdef").mkdir(parents=True)
            for name in ("models.yml", "config.yml"):
                (root / name).write_text("test: true\n")
            fake = root / "openshell"
            fake.write_text("""#!/usr/bin/env python3
import os, pathlib, sys
args = sys.argv[1:]
record = pathlib.Path(os.environ['GYRE_FAKE_RECORD'])
if args[:2] == ['gateway', 'login']:
    sys.exit(0)
if 'create' in args:
    record.open('a').write('create\\n')
elif 'get' in args:
    print('Phase: Ready')
elif 'exec' in args and '/tmp/stage/dev-remote.sh' in args:
    if 'CARGO_TARGET_DIR=/tmp/gyre-target' not in args or 'RUSTFLAGS=-C link-arg=-fuse-ld=lld' not in args:
        sys.exit(42)
    count = sum(line == 'remote' for line in record.read_text().splitlines())
    record.open('a').write('remote\\n')
    if count == 0:
        if os.environ.get('GYRE_FAKE_REGISTRY'):
            print('GYRE_BOOTSTRAP_COMPLETE task=task-001')
            print('Failed to connect to static.crates.io:443')
            sys.exit(1)
        sys.exit(74)
    print('GYRE_BOOTSTRAP_COMPLETE task=task-001')
elif 'exec' in args:
    sys.stdin.buffer.read()
    count = sum(line == 'stage' for line in record.read_text().splitlines())
    record.open('a').write('stage\\n')
    if count == 0:
        sys.exit(74)
elif 'delete' in args:
    record.open('a').write('delete\\n')
""")
            fake.chmod(0o755)
            env = {**os.environ, "OPENSHELL": str(fake), "OPENSHELL_OIDC_CLIENT_SECRET": "test",
                   "GYRE_FAKE_RECORD": str(root / "calls"), "GYRE_DEV_STATE": str(root),
                   "MODELS_YML": str(root / "models.yml"), "CONFIG_YML": str(root / "config.yml")}
            result = subprocess.run(
                ["bash", str(Path(__file__).with_name("dev-sandbox.sh")), "worker", "task-001",
                 "devloop/task-001/attempt-1", "origin/main", "0123456789abcdef"],
                env=env, capture_output=True, text=True, timeout=30,
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertEqual((root / "calls").read_text().splitlines(),
                             ["create", "stage", "stage", "remote", "remote", "delete"])
            (root / "calls").write_text("")
            result = subprocess.run(
                ["bash", str(Path(__file__).with_name("dev-sandbox.sh")), "worker", "task-001",
                 "devloop/task-001/attempt-1", "origin/main", "0123456789abcdef"],
                env={**env, "GYRE_FAKE_REGISTRY": "1"}, capture_output=True, text=True, timeout=30,
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertEqual((root / "calls").read_text().splitlines(),
                             ["create", "stage", "stage", "remote", "remote", "delete"])


class SandboxCreateCleanupTest(unittest.TestCase):
    def test_pending_object_survives_create_timeout_then_runs_without_reallocation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fake = root / 'openshell'; record = root / 'calls'
            fake.write_text('''#!/usr/bin/env python3
import os, pathlib, sys
args = sys.argv[1:]
record = pathlib.Path(os.environ['RECORD'])
if args[:2] == ['gateway', 'login']: sys.exit(0)
if 'create' in args:
    record.open('a').write('create\\n')
    print('ProvisioningTimedOut: waiting for capacity'); sys.exit(1)
if 'get' in args:
    count = record.read_text().splitlines().count('get')
    record.open('a').write('get\\n')
    print('Phase: Pending' if count < 2 else 'Phase: Ready'); sys.exit(0)
if 'exec' in args:
    if '/tmp/stage/dev-remote.sh' in args:
        record.open('a').write('remote\\n')
    else:
        sys.stdin.buffer.read(); record.open('a').write('stage\\n')
    sys.exit(0)
if 'delete' in args:
    record.open('a').write('delete\\n'); sys.exit(0)
sys.exit(1)
''')
            fake.chmod(0o755)
            env = {**os.environ, 'OPENSHELL': str(fake), 'OPENSHELL_OIDC_CLIENT_SECRET': 'test',
                   'GYRE_DEV_STATE': str(root), 'GYRE_DEV_READY_POLL': '0', 'RECORD': str(record)}
            result = subprocess.run(['bash', str(Path(__file__).with_name('dev-sandbox.sh')), 'worker', 'task-1000',
                                     'devloop/task-1000/attempt-1', 'origin/main', '0123456789abcdef'],
                                    env=env, capture_output=True, text=True, timeout=15)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            calls = record.read_text().splitlines()
            self.assertEqual(calls.count('create'), 1)
            self.assertEqual(calls[-3:], ['stage', 'remote', 'delete'])
            self.assertIn('gyr-zrs-w-01234567', result.stderr)

    def test_capacity_timeout_is_retryable_and_deletes_partial_sandbox(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fake = root / "openshell"
            record = root / "deleted.txt"
            fake.write_text("""#!/bin/sh
case " $* " in
  *" gateway login "*) exit 0 ;;
  *" sandbox create "*) echo 'ProvisioningTimedOut: waiting for capacity'; exit 1 ;;
  *" sandbox delete "*) echo "$*" >> "$DELETE_RECORD"; exit 0 ;;
esac
exit 1
""")
            fake.chmod(0o755)
            result = subprocess.run(
                ["bash", str(Path(__file__).with_name("dev-sandbox.sh")), "worker", "task-001",
                 "devloop/task-001/attempt-1", "origin/main", "0123456789abcdef"],
                env={**os.environ, "OPENSHELL": str(fake), "DELETE_RECORD": str(record),
                     "OPENSHELL_OIDC_CLIENT_SECRET": "test", "GYRE_DEV_STATE": str(root)},
                capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 78, result.stdout + result.stderr)
            self.assertIn("sandbox delete gyre-001-w-01234567", record.read_text())

    def test_partial_create_is_deleted(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fake = root / "openshell"
            record = root / "deleted.txt"
            fake.write_text("""#!/bin/sh
case " $* " in
  *" gateway login "*) exit 0 ;;
  *" sandbox create "*) echo "$CREATE_ERROR"; exit 1 ;;
  *" sandbox delete "*) echo "$*" >> "$DELETE_RECORD"; exit 0 ;;
esac
exit 1
""")
            fake.chmod(0o755)
            env = {**os.environ, "OPENSHELL": str(fake), "DELETE_RECORD": str(record),
                   "OPENSHELL_OIDC_CLIENT_SECRET": "test", "GYRE_DEV_STATE": str(root)}
            for message, code in (("Created sandbox: gyre-001-w-01234567", 75),
                                  ("provider 'gyre-github-rw' not found", 79)):
                with self.subTest(message=message):
                    result = subprocess.run(
                        ["bash", str(Path(__file__).with_name("dev-sandbox.sh")), "worker", "task-001",
                         "devloop/task-001/attempt-1", "origin/main", "0123456789abcdef"],
                        env={**env, "CREATE_ERROR": message}, capture_output=True, text=True, timeout=10)
                    self.assertEqual(result.returncode, code, result.stdout + result.stderr)
            self.assertIn("sandbox delete gyre-001-w-01234567", record.read_text())


if __name__ == "__main__":
    unittest.main()
