#!/usr/bin/env python3
"""Reject source edits by a reviewer, including work hidden in owned stashes."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess


def git(*args):
    return subprocess.check_output(['git', *args])


def files(task):
    paths = set(git('ls-files', '-z', '--cached', '--others', '--exclude-standard').split(b'\0'))
    result = {}
    for raw in sorted(paths - {b''}):
        name = os.fsdecode(raw)
        path = Path(name)
        if (name == f'specs/tasks/{task}.md' or name.startswith('web/dist/')
                or (name.startswith('specs/reviews/') and
                    re.fullmatch(r'(?:audit-)?' + re.escape(task) + r'(?:[.-].*)?', path.name))):
            continue
        if path.is_symlink():
            data = ('symlink:' + os.readlink(path)).encode()
        elif path.exists():
            data = path.read_bytes() + str(path.stat().st_mode & 0o777).encode()
        else:
            data = b'missing'
        result[name] = hashlib.sha256(data).hexdigest()
    return result


def restore(branch):
    spec = importlib.util.spec_from_file_location('checkpoint', Path(__file__).with_name('dev-checkpoint.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    module.restore(branch)


def snapshot(task, state):
    if not re.fullmatch(r'task-\d+', task):
        raise ValueError('invalid task name')
    branch = git('branch', '--show-current').decode().strip()
    restore(branch)
    state.write_text(json.dumps({'task': task, 'branch': branch,
                                 'files': files(task),
                                 'task_text': Path(f'specs/tasks/{task}.md').read_text()}))


def check(state):
    before = json.loads(state.read_text())
    # A review can look clean while source edits remain in a stash. Recover
    # them before deciding whether the code under review stayed unchanged.
    problem = ''
    try:
        restore(before['branch'])
    except (ValueError, RuntimeError, subprocess.CalledProcessError) as exc:
        problem = f'Cannot safely restore review work: {exc}\n'
    after = files(before['task'])
    changed = sorted(p for p in set(before['files']) | set(after)
                     if before['files'].get(p) != after.get(p))
    if not changed and not problem:
        return 0
    finding = ('\n## Review\n\n### Review changed source code\n\n' + problem
               + '\n'.join('- ' + p for p in changed)
               + '\n\nPreserved these edits for implementation. Review cannot approve its own '
                 'source or verifier edits. Repair them within task scope and request a fresh independent review.\n')
    task_path = Path(f"specs/tasks/{before['task']}.md")
    text = task_path.read_text() if task_path.exists() else before['task_text']
    parts = text.split('---', 2)
    parts[1], count = re.subn(r'(?m)^progress:[^\n]*$', 'progress: needs-revision', parts[1])
    if count != 1:
        raise ValueError('cannot reopen task with invalid progress field')
    task_path.parent.mkdir(parents=True, exist_ok=True)
    task_path.write_text('---'.join(parts) + finding)
    repair = state.parent / 'repair.md'
    repair.write_text(finding + (repair.read_text() if repair.exists() else ''))
    print(finding, flush=True)
    return 1


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['snapshot', 'check'])
    parser.add_argument('state', type=Path)
    parser.add_argument('--task')
    args = parser.parse_args()
    if args.action == 'snapshot':
        snapshot(args.task, args.state)
    else:
        raise SystemExit(check(args.state))
