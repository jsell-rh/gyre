#!/usr/bin/env python3
"""Invalidate workspace artifacts before reusing a target dir across worktrees."""
import json
import subprocess


def main():
    result = subprocess.run(['cargo', 'metadata', '--no-deps', '--format-version', '1'],
                            check=True, capture_output=True, text=True)
    metadata = json.loads(result.stdout)
    members = set(metadata['workspace_members'])
    packages = [p['id'] for p in metadata['packages'] if p['id'] in members]
    if not packages:
        raise RuntimeError('Cargo workspace has no packages to rebuild')
    command = ['cargo', 'clean']
    for package in packages:
        command += ['--package', package]
    subprocess.run(command, check=True)


if __name__ == '__main__':
    main()
