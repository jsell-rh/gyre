#!/usr/bin/env python3
"""Attach to one persistent stage process without launching duplicates."""
import fcntl
import importlib.util
from pathlib import Path
import subprocess
import sys
import time

stage = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('attach', stage / 'pipeline-transport.py')
attach = importlib.util.module_from_spec(spec)
spec.loader.exec_module(attach)


def main():
    offset = int(sys.argv[1])
    result, identity, log = (stage / name for name in
                             ('step.exit', 'step.exit.process.json', 'step.log'))
    lock = (stage / 'step.lock').open('a')
    fcntl.flock(lock, fcntl.LOCK_EX)
    intent = stage / 'step.intent'
    if not result.exists() and not identity.exists():
        if intent.exists():
            # A launcher can die between Popen and identity publication.
            # Preserve the ambiguous intent; cleanup captures its checkpoint.
            return 77
        intent.touch()
        with log.open('ab', buffering=0) as output:
            child = subprocess.Popen(['bash', str(stage / 'dev-process.sh'), str(result),
                                      'python3', str(stage / 'pipeline-remote.py')],
                                     stdout=output, stderr=subprocess.STDOUT,
                                     stdin=subprocess.DEVNULL, start_new_session=True)
        deadline = time.monotonic() + 10
        while not identity.exists() and not result.exists() and child.poll() is None:
            if time.monotonic() > deadline:
                return 77
            time.sleep(.05)
    return attach.attach(offset, result, identity, log, lock)


if __name__ == '__main__':
    raise SystemExit(main())
