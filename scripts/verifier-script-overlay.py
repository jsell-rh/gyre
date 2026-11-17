#!/usr/bin/env python3
"""Restore upstream checks without leaving files deleted by the candidate behind.

Upstream verifier LOGIC (the check scripts) is restored from the base so a
candidate cannot weaken the gates that judge it, but the frozen repo-state
data files (scripts/*-exemptions.txt) follow the merge instead: base data
describes the base tree, and judging the merged tree with it fails whenever a
candidate legitimately removes an exempted route or finding together with its
exemption entry. This cannot weaken the upstream phase: dev-check.sh forbids
adding any exemption entry relative to the base, so merged data is entrywise a
subset of base data, and a removed entry only re-exposes the underlying
finding to the upstream check that consumed it.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess


def git(*args, input=None):
    return subprocess.check_output(['git', *args], input=input)


def tree(ref):
    entries = {}
    for entry in git('ls-tree', '-rz', ref, '--', 'scripts').split(b'\0'):
        if entry:
            metadata, name = entry.split(b'\t', 1)
            mode, kind, oid = metadata.split()
            if kind != b'blob':
                raise RuntimeError('non-file script entry cannot be overlaid')
            entries[os.fsdecode(name)] = {'mode':mode.decode(), 'oid':oid.decode()}
    return entries


def begin(manifest):
    candidate = git('rev-parse', 'HEAD').decode().strip()
    base = git('rev-parse', 'HEAD^1').decode().strip()
    upstream, current = tree(base), tree(candidate)
    introduced = {name:entry for name,entry in upstream.items() if name not in current}
    for name in introduced:
        if os.path.lexists(name):
            raise RuntimeError(f'refusing to overwrite an existing path: {name}')
    manifest.write_text(json.dumps({'candidate':candidate, 'introduced':introduced}))
    git('restore', '--source='+base, '--worktree', '--', 'scripts')
    # Frozen repo-state data follows the merge (see module docstring): only
    # files present in both trees, so candidate-deleted files stay restored
    # from the base (conservative) and candidate-added files are untouched.
    state = sorted(name for name in current
                   if name.endswith('-exemptions.txt') and name in upstream)
    if state:
        git('restore', '--source='+candidate, '--worktree', '--', *state)


def finish(manifest):
    if not manifest.exists():
        return
    value = json.loads(manifest.read_text())
    if git('rev-parse', 'HEAD').decode().strip() != value['candidate']:
        raise RuntimeError('script overlay changed HEAD')
    errors = []
    root = Path.cwd().resolve()
    for name,entry in value['introduced'].items():
        path = Path(name)
        if not path.parent.resolve().is_relative_to(root):
            errors.append(name)
            continue
        if not os.path.lexists(path):
            continue
        correct_type = path.is_symlink() if entry['mode'] == '120000' else path.is_file() and not path.is_symlink()
        if not correct_type:
            errors.append(name)
            continue
        content = os.fsencode(os.readlink(path)) if path.is_symlink() else path.read_bytes()
        oid = git('hash-object', '--stdin', input=content).decode().strip()
        if oid != entry['oid']:
            errors.append(name)  # Never erase an unexpected source mutation.
            continue
        path.unlink()
    git('restore', '--source='+value['candidate'], '--worktree', '--', 'scripts')
    if errors:
        raise RuntimeError('upstream script overlay was modified: '+', '.join(errors))
    manifest.unlink()


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('begin','finish'))
    parser.add_argument('manifest', type=Path)
    args = parser.parse_args()
    try:
        (begin if args.operation == 'begin' else finish)(args.manifest)
    except Exception as exc:
        print('Verifier script overlay failed: '+str(exc), flush=True)
        raise SystemExit(77)
