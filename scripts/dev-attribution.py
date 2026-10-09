#!/usr/bin/env python3
"""Refresh task commit attribution after rebases and checkpoints."""
import json
from pathlib import Path
import re
import subprocess
import sys


def refresh(task):
    if not re.fullmatch(r'task-\d+', task):
        raise ValueError('invalid task name')
    history = subprocess.check_output(
        ['git', 'log', '--no-merges', '--no-renames', '--format=%x1e%H%x1f%s', '--name-only'], text=True)
    hashes = []
    for record in history.split('\x1e')[1:]:
        header, *paths = record.splitlines()
        sha, subject = header.split('\x1f', 1)
        labels = set()
        for first, siblings in re.findall(r'task-(\d+)((?:\+\d+)*)', subject):
            labels.update('task-' + number for number in [first] + siblings.split('+')[1:])
        if task in labels and not subject.startswith(('process:', 'review:')) and any(
                path.startswith(('crates/', 'web/src', 'web/tests')) for path in paths):
            hashes.append(sha)
    path = Path('specs/tasks') / (task + '.md')
    parts = path.read_text().split('---', 2)
    if len(parts) != 3 or parts[0].strip():
        raise ValueError('missing task frontmatter')
    lines = parts[1].splitlines()
    start = next((i for i, line in enumerate(lines) if line.startswith('commits:')), len(lines))
    end = start + 1
    while end < len(lines) and not re.match(r'^[a-z_][\w-]*:', lines[end]):
        end += 1
    lines[start:end] = ['commits: ' + json.dumps(hashes)]
    parts[1] = '\n'.join(lines) + '\n'
    path.write_text('---'.join(parts))


if __name__ == '__main__':
    refresh(sys.argv[1])
