#!/usr/bin/env python3
"""Launch one durable sandbox job and attach to its log across reconnects."""
import fcntl
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import time


STAGE = Path(__file__).resolve().parent


def alive(identity):
    try:
        value = json.loads(identity.read_text())
        stat = Path(f"/proc/{int(value['pid'])}/stat").read_text().rsplit(')', 1)[1].split()
        return stat[0] != 'Z' and value['start'] == stat[19] and value['boot'] == Path(
            '/proc/sys/kernel/random/boot_id').read_text().strip()
    except (OSError, ValueError, KeyError, IndexError):
        return False


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('mode', 'task', 'arg1', 'arg2', 'ident'):
        parser.add_argument(name)
    parser.add_argument('offset', type=int)
    parser.add_argument('--retry-failed', action='store_true')
    args = parser.parse_args()
    arguments = [args.mode, args.task, args.arg1, args.arg2, args.ident]
    offset = args.offset
    result = STAGE / 'remote.exit'
    identity = STAGE / 'remote.exit.process.json'
    log = STAGE / 'remote.log'
    intent = STAGE / 'remote-command.json'
    lock = (STAGE / 'remote-launch.lock').open('a')
    fcntl.flock(lock, fcntl.LOCK_EX)
    if intent.exists() and json.loads(intent.read_text()) != arguments:
        print('attachment arguments do not match the persistent job', file=sys.stderr)
        return 79
    if result.exists() and args.retry_failed and int(result.read_text().strip()) != 0:
        if alive(identity):
            print('completed job is still exiting; retry attachment', file=sys.stderr)
            return 77
        result.unlink()
        identity.unlink(missing_ok=True)
        intent.unlink(missing_ok=True)
    if not result.exists() and not identity.exists():
        if intent.exists():
            deadline = time.monotonic() + 10
            while not identity.exists() and not result.exists() and time.monotonic() < deadline:
                time.sleep(.05)
            if not identity.exists() and not result.exists():
                print('remote launch outcome is uncertain; refusing duplicate execution', file=sys.stderr)
                return 77
        else:
            intent.write_text(json.dumps(arguments))
            return launch_and_attach(arguments, offset, result, identity, log, lock)
    return attach(offset, result, identity, log, lock)


def launch_and_attach(arguments, offset, result, identity, log, lock):
    if offset < 0 or (not log.exists() and offset != 0) or (log.exists() and offset > log.stat().st_size):
        print('remote log cursor is outside the persistent log', file=sys.stderr)
        return 79
    try:
        with log.open('ab', buffering=0) as output:
            process = subprocess.Popen(['bash', str(STAGE / 'dev-process.sh'), str(result),
                                        'bash', str(STAGE / 'dev-remote.sh'), *arguments],
                                       stdin=subprocess.DEVNULL, stdout=output, stderr=subprocess.STDOUT,
                                       start_new_session=True, close_fds=True)
        deadline = time.monotonic() + 10
        while not identity.exists() and not result.exists() and process.poll() is None and time.monotonic() < deadline:
            time.sleep(.05)
        if not identity.exists() and not result.exists():
            print('remote launch has no durable process identity', file=sys.stderr)
            return 77
    except OSError as exc:
        print(f'remote job could not launch: {exc}', file=sys.stderr)
        return 77
    return attach(offset, result, identity, log, lock)


def attach(offset, result, identity, log, lock):
    if not result.exists() and not alive(identity):
        print('remote job disappeared without a durable outcome; inspect its checkpoint', file=sys.stderr)
        return 77
    fcntl.flock(lock, fcntl.LOCK_UN)
    lock.close()
    if offset < 0 or offset > log.stat().st_size:
        print('remote log cursor is outside the persistent log', file=sys.stderr)
        return 79
    with log.open('rb') as stream:
        stream.seek(offset)
        while True:
            start = stream.tell()
            line = stream.readline()
            done = result.exists()
            if line and (line.endswith(b'\n') or done):
                sys.stdout.buffer.write(line if line.endswith(b'\n') else line + b'\n')
                sys.stdout.buffer.write(f'GYRE_REMOTE_LOG_OFFSET {stream.tell()}\n'.encode())
                sys.stdout.buffer.flush()
            elif done:
                return int(result.read_text().strip())
            else:
                stream.seek(start)
                if not alive(identity):
                    # The wrapper atomically writes its result before exiting.
                    time.sleep(.1)
                    if not result.exists():
                        print('remote job disappeared without an outcome', file=sys.stderr)
                        return 77
                time.sleep(.2)


if __name__ == '__main__':
    raise SystemExit(main())
