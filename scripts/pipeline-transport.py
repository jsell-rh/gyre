"""Attach by byte cursor to a process identified by PID, boot and start time."""
import fcntl
import json
from pathlib import Path
import sys
import time

def alive(identity):
    try:
        value = json.loads(identity.read_text())
        stat = Path(f"/proc/{int(value['pid'])}/stat").read_text().rsplit(')', 1)[1].split()
        return stat[0] != 'Z' and value['start'] == stat[19] and value['boot'] == Path(
            '/proc/sys/kernel/random/boot_id').read_text().strip()
    except (OSError, ValueError, KeyError, IndexError):
        return False



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

