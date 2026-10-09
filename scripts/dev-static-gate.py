#!/usr/bin/env python3
"""Run a cloud gate; reproduce a failure on its main base in the same sandbox."""
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tempfile


def main():
    base = subprocess.check_output(['git', 'rev-parse', sys.argv[1]], text=True).strip()
    command = sys.argv[2:]
    timeout = int(os.environ.get('GYRE_DEV_GATE_TIMEOUT', '1800')) + 60
    try:
        result = subprocess.run(command, timeout=timeout)
    except (OSError, subprocess.TimeoutExpired) as exc:
        print(f'gate execution unavailable: {exc}', file=sys.stderr)
        return 77
    if result.returncode == 0:
        return 0
    if result.returncode in (124, 137, -9):
        print('gate execution timed out or was killed; awaiting infrastructure recovery', file=sys.stderr)
        return 77
    directory = Path(tempfile.mkdtemp(prefix='gyre-gate-baseline-'))
    try:
        added = subprocess.run(['git', 'worktree', 'add', '--quiet', '--detach', str(directory), base], capture_output=True, text=True)
        if added.returncode:
            print(added.stderr, file=sys.stderr)
            return 77
        try:
            baseline = subprocess.run(command, cwd=directory, capture_output=True, text=True, timeout=timeout)
        except (OSError, subprocess.TimeoutExpired) as exc:
            print(f'baseline gate execution unavailable: {exc}', file=sys.stderr)
            return 77
        if baseline.returncode == 0:
            return 1  # Regression or newly strengthened gate: repair candidate.
        output = (baseline.stdout + baseline.stderr)[-65536:]
        if baseline.returncode in (124, 137, -9) or re.search(r'failed to download|failed to get .* dependency|Could not resolve (?:host|proxy)|Temporary failure in name resolution', output):
            print('baseline gate infrastructure unavailable: ' + output, file=sys.stderr)
            return 77
        if 'command not found' in output or 'ModuleNotFoundError' in output:
            print('baseline gate lacks a required tool or dependency: ' + output, file=sys.stderr)
            return 79
        signature = hashlib.sha256(json.dumps({'command': command, 'python': platform.python_version(),
                                              'image': os.environ.get('GYRE_DEV_IMAGE', 'pinned-worker')}, sort_keys=True).encode()).hexdigest()
        # The local driver captures stdout before the heavy sandbox is purged.
        print('GYRE_BASELINE_FAILURE_JSON ' + json.dumps({'base': base, 'environment': signature,
                                                         'probe': command, 'log': output}), flush=True)
        return 81
    finally:
        subprocess.run(['git', 'worktree', 'remove', '--force', str(directory)], capture_output=True)
        if directory.exists():
            shutil.rmtree(directory, ignore_errors=True)


if __name__ == '__main__':
    raise SystemExit(main())
