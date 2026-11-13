"""Retry Fullsend's upload staging only, before the SDK connects to the gateway."""
import ctypes
import os
import re
import shlex
import signal
import subprocess
import sys
import time


def staging(args):
    if args[:2] != ['sandbox', 'exec'] or '--' not in args:
        return False
    command = args[args.index('--') + 1:]
    if len(command) != 3 or command[:2] != ['sh', '-c']:
        return False
    try:
        words = shlex.split(command[2])
    except ValueError:
        return False
    stage = r'/tmp/fs-upload-[a-zA-Z]{20}'
    if len(words) == 5 and words[:4] == ['mkdir', '-m', '700', '--']:
        return bool(re.fullmatch(stage, words[4]))
    if (len(words) == 14 and words[:2] == ['test', '-f']
            and words[3:7] == ['&&', 'mkdir', '-p', '--']
            and words[8:12] == ['&&', 'mv', '-fT', '--']
            and words[2] == words[12] and words[7].startswith('/') and words[13].startswith('/')):
        return bool(re.fullmatch(stage + r'/[^/]+', words[2]))
    return False


def _parent_death(parent):
    # Fullsend kills its CLI subprocess on deadline, including with SIGKILL.
    # Ensure a retry wrapper cannot orphan a CLI/SSH process holding its pipes.
    if ctypes.CDLL(None, use_errno=True).prctl(1, signal.SIGKILL) != 0:
        raise OSError(ctypes.get_errno(), 'cannot bind bootstrap child lifetime')
    if os.getppid() != parent:
        os.kill(os.getpid(), signal.SIGKILL)


def run(command, env, execution, timeout=120):
    deadline = time.monotonic() + timeout
    process = None
    def stopped(signum, _frame):
        raise SystemExit(128 + signum)
    handlers = {sig: signal.signal(sig, stopped) for sig in (signal.SIGTERM, signal.SIGINT)}
    try:
        for attempt in range(3):
            execution.current()
            parent = os.getpid()
            process = subprocess.Popen(command, env=env, stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, preexec_fn=lambda: _parent_death(parent))
            try:
                out, err = process.communicate(timeout=max(.01, deadline - time.monotonic()))
            except subprocess.TimeoutExpired:
                process.kill()
                out, err = process.communicate()
                sys.stdout.buffer.write(out); sys.stderr.buffer.write(err)
                return 124
            # SdkError::Connect is raised while opening the channel, before
            # any remote exec. Do not retry generic
            # transport errors, remote exits, authentication, or partial output.
            diagnostic = re.sub(rb'\x1b\[[0-9;]*m', b'', err).decode('utf-8', errors='replace').strip()
            preconnect = bool(re.fullmatch(r'Error:\s*[×x]?\s*connect error: transport error', diagnostic))
            if process.returncode != 1 or out or not preconnect or attempt == 2:
                sys.stdout.buffer.write(out); sys.stderr.buffer.write(err)
                if process.returncode and preconnect:
                    execution.store.event('bootstrap_connect_exhausted', {'attempts':attempt+1}, execution.claim['task'], execution.claim['id'])
                return process.returncode
            execution.store.event('bootstrap_connect_retry', {'attempt':attempt+1}, execution.claim['task'], execution.claim['id'])
            time.sleep(min(2 ** attempt, max(0, deadline - time.monotonic())))
        raise AssertionError('unreachable bootstrap retry')
    finally:
        if process is not None:
            if process.poll() is None: process.kill()
            process.communicate()
        for sig, handler in handlers.items(): signal.signal(sig, handler)
