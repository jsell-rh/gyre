#!/usr/bin/env python3
"""Invalidate workspace artifacts before reusing a target dir across worktrees."""
import json
import subprocess
import sys


def main():
    result = subprocess.run(['cargo', 'metadata', '--no-deps', '--format-version', '1'],
                            capture_output=True, text=True)
    if result.returncode:
        print(result.stderr or result.stdout, file=sys.stderr)
        return result.returncode
    metadata = json.loads(result.stdout)
    members = set(metadata['workspace_members'])
    packages = [p['id'] for p in metadata['packages'] if p['id'] in members]
    if not packages:
        raise RuntimeError('Cargo workspace has no packages to rebuild')
    command = ['cargo', 'clean']
    for package in packages:
        command += ['--package', package]
    return 0 if subprocess.run(command).returncode == 0 else 75


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError) as exc:
        print(f'Cargo artifact preparation unavailable: {exc}', file=sys.stderr)
        sys.exit(75)
