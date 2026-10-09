#!/usr/bin/env python3
"""Recover task-owned stashes before checkpointing an interrupted agent round."""
import re
import subprocess
import sys


def git(*args):
    return subprocess.check_output(['git', *args], text=True).strip()


def restore(branch):
    if not re.fullmatch(r'devloop/task-\d+/attempt-\d+', branch):
        raise ValueError('invalid task branch')
    if git('branch', '--show-current') != branch:
        raise RuntimeError('agent left the assigned branch; refuse to checkpoint another checkout')
    owned = []
    for line in git('stash', 'list', '--format=%H%x1f%gs').splitlines():
        sha, subject = line.split('\x1f', 1)
        if subject.startswith((f'WIP on {branch}:', f'On {branch}:')):
            owned.append(sha)
    # Restore oldest first if more than one baseline probe hid task work.
    for sha in reversed(owned):
        subprocess.run(['git', 'stash', 'apply', '--quiet', sha], check=True)
        # The work is now visible to checkpoint/recovery, rather than hidden in
        # a stash the next model session doesn't know to inspect. Conflicts fail
        # closed above and retain the original stash for debugging/recovery.
        for line in git('stash', 'list', '--format=%gd%x1f%H').splitlines():
            ref, actual = line.split('\x1f', 1)
            if actual == sha:
                subprocess.run(['git', 'stash', 'drop', '--quiet', ref], check=True)
                break
        print(f'GYRE_RECOVERED_STASH {sha} branch={branch}', flush=True)


if __name__ == '__main__':
    restore(sys.argv[1])
