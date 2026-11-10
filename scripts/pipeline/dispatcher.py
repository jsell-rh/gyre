"""Build and launch the extracted dispatcher, scoped to this Gyre ledger."""
import fcntl
import hashlib
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]


def binary(state):
    source = ROOT / 'tools/fullsend-dispatcher'
    digest = hashlib.sha256()
    for path in sorted(source.iterdir()):
        if path.suffix == '.go' and not path.name.endswith('_test.go') or path.name in ('go.mod', 'go.sum'):
            digest.update(path.name.encode() + path.read_bytes())
    directory = state / 'tools'
    directory.mkdir(exist_ok=True)
    target = directory / ('gyre-dispatcher-' + digest.hexdigest()[:16])
    with (directory / 'dispatcher-build.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        if not target.exists():
            temporary = target.with_suffix('.building')
            subprocess.run(['go', 'build', '-trimpath', '-o', str(temporary), '.'], cwd=source, check=True)
            temporary.chmod(0o700)
            temporary.replace(target)
    return target


def serve(store, interval, task=None, plain=False):
    target = binary(store.directory)
    temporary = store.directory / 'dispatcher/tmp'
    temporary.mkdir(parents=True, exist_ok=True, mode=0o700)
    env = {**os.environ,
           'FULLSEND_DISPATCH_ADAPTER': str(ROOT / 'scripts/dispatch-adapter.py'),
           'FULLSEND_DISPATCH_PYTHON': sys.executable,
           'GYRE_PIPELINE_STATE': str(store.directory), 'TMPDIR': str(temporary)}
    # Inherited HyperShell launcher variables must never select its adapter.
    env.pop('FUNNEL_ADAPTER', None)
    args = [str(target), '--state', str(store.directory), '--interval', str(interval)]
    if task:
        args += ['--task', task]
    if plain:
        args += ['--plain']
    store.close()
    os.execve(target, args, env)
