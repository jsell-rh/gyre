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
            for name in ("models.yml", "config.yml"):
                (root / name).write_text("test: true\n")
            fake = root / "openshell"
            fake.write_text("""#!/usr/bin/env python3
import base64, os, pathlib, sys
args = sys.argv[1:]
record = pathlib.Path(os.environ['GYRE_FAKE_RECORD'])
if args[:2] == ['gateway', 'login']:
    sys.exit(0)
if 'create' in args:
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
    sys.stdin.buffer.read()
    record.open('a').write('stage\\n')
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


if __name__ == "__main__":
    unittest.main()
