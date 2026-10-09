"""Restore an attested checkpoint and publish with an explicit Git lease."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile


def restore(directory, repository, expected_branch=None):
    directory, repository = Path(directory), Path(repository)
    patch = directory / 'recovery.patch'
    receipt = json.loads((directory / 'recovery.json').read_text())
    if (receipt.get('version') != 1 or not receipt.get('published_known') or
            receipt.get('stash_count') != 0 or
            hashlib.sha256(patch.read_bytes()).hexdigest() != receipt.get('patch_sha256')):
        raise ValueError('checkpoint lacks a complete, matching publication receipt')
    for key in ('base', 'head', 'tree'):
        if not re.fullmatch(r'[a-f0-9]{40}', receipt.get(key, '')):
            raise ValueError('invalid checkpoint source identity')
    branch = receipt.get('branch', '')
    if not re.fullmatch(r'pipeline/task-\d+/[a-f0-9]{32}-\d+', branch):
        raise ValueError('checkpoint does not belong to a pipeline claim')
    if expected_branch is not None and branch != expected_branch:
        raise ValueError('checkpoint belongs to another task or claim')
    published = receipt.get('published_head') or ''
    if published and not re.fullmatch(r'[a-f0-9]{40}', published):
        raise ValueError('invalid publication lease')

    def git(*args, env=None, data=None):
        return subprocess.check_output(['git', *args], cwd=repository,
                                       env=env, input=data).decode().strip()

    git('fetch', '--quiet', 'origin', receipt['base'])
    saved = directory / 'restored.json'
    if saved.exists():
        outcome = json.loads(saved.read_text())
        if outcome['patch_sha256'] != receipt['patch_sha256'] or outcome['branch'] != branch:
            raise ValueError('recovery outcome belongs to another checkpoint')
        head = outcome['head']
    else:
        with tempfile.TemporaryDirectory(prefix='gyre-recovery-') as temp:
            env = {**os.environ, 'GIT_INDEX_FILE': str(Path(temp) / 'index'),
                   'GIT_AUTHOR_NAME': 'gyre-pipeline', 'GIT_AUTHOR_EMAIL': 'jsell-rh@users.noreply.github.com',
                   'GIT_COMMITTER_NAME': 'gyre-pipeline', 'GIT_COMMITTER_EMAIL': 'jsell-rh@users.noreply.github.com'}
            git('read-tree', receipt['base'], env=env)
            if patch.stat().st_size:
                git('apply', '--cached', '--binary', str(patch.resolve()), env=env)
            tree = git('write-tree', env=env)
            if tree != receipt['tree']:
                raise ValueError('restored source tree does not match capture receipt')
            # A non-merge source commit keeps recovered edits visible to
            # attribution and review. Preserve published history separately.
            head = git('commit-tree', tree, '-p', receipt['base'], env=env,
                       data=b'checkpoint: recover interrupted pipeline source\n')
            if published:
                git('fetch', '--quiet', 'origin', published)
                head = git('commit-tree', tree, '-p', head, '-p', published, env=env,
                           data=b'checkpoint: retain published pipeline history\n')
            outcome = {'head': head, 'tree': tree, 'branch': branch,
                       'patch_sha256': receipt['patch_sha256']}
            temporary = saved.with_suffix('.tmp')
            temporary.write_text(json.dumps(outcome))
            temporary.chmod(0o600)
            temporary.replace(saved)
    actual = git('ls-remote', '--heads', 'origin', branch).split()
    actual = actual[0] if actual else ''
    if actual != head:
        if actual != published:
            raise ValueError('remote checkpoint changed; refusing to overwrite it')
        git('push', f'--force-with-lease=refs/heads/{branch}:{published}',
            'origin', f'{head}:refs/heads/{branch}')
    return outcome
