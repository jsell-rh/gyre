#!/usr/bin/env python3
"""Real exact-tree gateway gates; isolated state and no inference or fake delivery."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import uuid

from pipeline.credentials import load
from pipeline.execution import ROOT, Execution
from pipeline.stages import verify
from pipeline.store import Store


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-state', type=Path, default=ROOT / '.gyre-pipeline')
    parser.add_argument('--candidate', default='HEAD')
    args = parser.parse_args()
    source = Store(args.source_state)
    settings = {key: source.setting(key, default) for key, default in (
        ('gateway', 'gyre-gyre'), ('repo_url', 'https://github.com/jsell-rh/gyre.git'),
        ('fullsend_image', json.loads((ROOT / '.fullsend/toolchain.json').read_text())['worker_image']),
        ('verification_provider', 'github'))}
    source.close()
    root = args.source_state / ('remote-verification-proof-' + uuid.uuid4().hex[:12])
    root.mkdir(mode=0o700)
    credentials = args.source_state / 'gateway.env'
    if credentials.exists():
        if credentials.stat().st_mode & 0o077:
            raise ValueError('gateway credentials must have mode 0600')
        shutil.copyfile(credentials, root / 'gateway.env')
        (root / 'gateway.env').chmod(0o600)
    s = Store(root)
    for key, value in settings.items():
        s.set_setting(key, value)
    s.set_setting('slots', 1)
    load(s)
    def git(*args):
        return subprocess.check_output(['git', *args], cwd=ROOT, text=True, timeout=120).strip()
    candidate = git('rev-parse', args.candidate)
    base = git('ls-remote', settings['repo_url'], 'refs/heads/main').split()[0]
    # Reuse an already delivered task solely to exercise its completion field.
    # Proof scope is explicit; neither a model review nor a new delivery is claimed.
    body = git('show', candidate + ':specs/tasks/task-239.md')
    s.put_task('task-239', 'proof', body, {'dependencies': []})
    work = s.enqueue('verify', 'task-239', 'proof', {'base': base, 'candidate': candidate})
    e = Execution(s, s.claim('verify', 'remote-verification-proof'))
    print('Proof state:', root, flush=True)
    result = {'scope': 'model-free remote infrastructure proof; no product delivery', 'candidate': candidate, 'base': base}
    try:
        with e.heartbeat():
            receipt, updates, findings = verify(e, s.task('task-239'))
            result.update(receipt=receipt, findings=findings)
            if not receipt.get('passed'):
                raise RuntimeError('real remote verification failed; inspect retained output.log')
            branch = 'refs/heads/' + receipt['branch']
            git('push', '--force-with-lease=' + branch + ':' + receipt['head'], 'origin', ':' + branch)
            s.finish(work, e.claim['token'], result)
            print('PASS: complete gateway receipt for exact tree', receipt['tree'], flush=True)
    finally:
        result['resources'] = [dict(r) for r in s.db.execute('SELECT name,kind,state FROM resources')]
        (root / 'proof.json').write_text(json.dumps(result, indent=2))
        s.close()


if __name__ == '__main__':
    main()
