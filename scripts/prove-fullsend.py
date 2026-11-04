#!/usr/bin/python3
"""Real gateway/Git transport proof with a scripted runtime, never inference."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import uuid

from pipeline.credentials import load
from pipeline.execution import Execution, ROOT
from pipeline.fullsend import binary, execute, forge_token, prepare
from pipeline.store import Store


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-state', type=Path, default=ROOT / '.gyre-pipeline')
    parser.add_argument('--gateway')
    parser.add_argument('--credentials-file', type=Path)
    parser.add_argument('--state', type=Path, help='new, isolated proof directory')
    args = parser.parse_args()
    source = Store(args.source_state)
    runner = binary(source)
    gateway = args.gateway or source.setting('gateway', 'gyre-gyre')
    repo_url = source.setting('repo_url', 'https://github.com/jsell-rh/gyre.git')
    image = source.setting('fullsend_image', json.loads((ROOT / '.fullsend/toolchain.json').read_text())['worker_image'])
    source.close()
    root = (args.state or args.source_state / ('fullsend-proof-' + uuid.uuid4().hex[:12])).resolve()
    root.mkdir(mode=0o700, parents=True, exist_ok=False)
    store = Store(root)
    credentials = args.credentials_file or args.source_state / 'gateway.env'
    if credentials.exists():
        if credentials.stat().st_mode & 0o077:
            raise ValueError('credential input must have mode 0600')
        shutil.copyfile(credentials, root / 'gateway.env')
        (root / 'gateway.env').chmod(0o600)
    load(store)
    for key, value in {'slots': 1, 'gateway': gateway, 'repo_url': repo_url, 'fullsend_image': image}.items():
        store.set_setting(key, value)
    base = subprocess.check_output(['git', 'ls-remote', repo_url, 'refs/heads/main'], text=True, timeout=60).split()[0]
    body = ('---\ntitle: Model-free fullsend transport fixture\nprogress: not-started\n'
            'depends_on: []\nspec_ref: fixture only\n---\n\n## Acceptance Criteria\n\n'
            'Prove transport and checkpoint handling. This is not a product implementation or model review.\n')
    store.put_task('task-9999', 'fixture', body, {})
    work = store.enqueue('implement', 'task-9999', 'fixture', {'base': base})
    execution = Execution(store, store.claim('implement', 'fixture-proof'))
    os.environ['GYRE_FULLSEND'] = str(runner)
    proof = {'scope': 'scripted infrastructure fixture; no inference or product delivery', 'state': str(root)}
    print('Proof state:', root, flush=True)
    try:
        with execution.heartbeat():
            execution.login()
            cfg, repo, shim = prepare(execution, store.task('task-9999'), proof['scope'])
            (cfg / 'config.yaml').write_text(json.dumps({'version': '1', 'runtime': 'dummy',
                'agents': [{'name': 'implement', 'source': 'harness/implement.yaml'}]}))
            harness = json.loads((cfg / 'harness/implement.yaml').read_text())
            harness.pop('base')
            harness.update(policy='policies/worker.yaml', providers=['github'], model='fixture',
                           validation_loop={'script': 'scripts/validate-assignment.py', 'max_iterations': 1})
            (cfg / 'harness/implement.yaml').write_text(json.dumps(harness))
            (cfg / 'behaviour').mkdir()
            (cfg / 'behaviour/current-scenario.yaml').write_text(json.dumps({'ops': [
                {'op': 'read_file', 'args': 'AGENTS.md'},
                {'op': 'write_fixture', 'args': 'checkout/.gyre-fullsend-fixture.txt, inline',
                 'content': 'Explicit model-free fullsend transport fixture.\n'},
                {'op': 'write_fixture', 'args': 'checkout/specs/tasks/task-9999.md, inline',
                 'content': body.replace('progress: not-started', 'progress: ready-for-review')},
            ]}))
            env = {**os.environ, 'GH_TOKEN': forge_token(), 'PATH': str(shim) + os.pathsep + os.environ['PATH'],
                   'GYRE_FULLSEND_CONTEXT': str(execution.directory / 'fullsend-context.json'),
                   'GYRE_PIPELINE_ROOT': str(ROOT), 'OPENSHELL_GATEWAY': gateway, 'GYRE_OPEN_GATEWAY': gateway}
            env.pop('FULLSEND_INFERENCE_GATEWAY_API_KEY', None)
            outcome = execute(execution, cfg, repo, shim, env, runtime='dummy')
            proof['outcome'] = outcome
            if not outcome.get('valid') or not outcome.get('published'):
                raise RuntimeError('scripted runtime did not produce a valid published checkpoint')
            with tempfile.TemporaryDirectory() as directory:
                def git(*args):
                    return subprocess.check_output(['git', '-C', directory, *args], text=True, stderr=subprocess.PIPE, timeout=120).strip()
                git('init', '-q')
                git('fetch', '-q', repo_url, execution.branch)
                if git('rev-parse', 'FETCH_HEAD') != outcome['head'] or git('rev-parse', 'FETCH_HEAD^{tree}') != outcome['tree']:
                    raise RuntimeError('remote checkpoint identity differs from the receipt')
                if git('show', 'FETCH_HEAD:.gyre-fullsend-fixture.txt') != 'Explicit model-free fullsend transport fixture.':
                    raise RuntimeError('remote checkpoint is missing the fixture source')
                proof['remote_source_verified'] = True
                ref = 'refs/heads/' + execution.branch
                git('push', '--force-with-lease=' + ref + ':' + outcome['head'], repo_url, ':' + ref)
                proof['fixture_branch_deleted'] = True
            store.finish(work, execution.claim['token'], {'fixture_only': True, 'head': outcome['head']})
    except BaseException as exc:
        proof['error'] = type(exc).__name__ + ': ' + str(exc)
        raise
    finally:
        proof['resources'] = [dict(row) for row in store.db.execute('SELECT name,kind,state FROM resources')]
        (root / 'proof.json').write_text(json.dumps(proof, indent=2))
        print('Receipt:', root / 'proof.json', flush=True)
        print('Attempt log:', execution.log, flush=True)
        store.close()


if __name__ == '__main__':
    main()
