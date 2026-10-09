#!/usr/bin/env python3
"""Independent discover/claim/run development stages and optional supervisor."""
import argparse
import fcntl
import json
import os
from pathlib import Path
import signal
import socket
import sqlite3
import subprocess
import sys
import time
import uuid

from pipeline.catalog import discover, generation_for, metadata, sync_checkout
from pipeline.execution import Execution, ROOT, Retry
from pipeline.eligibility import eligible
from pipeline.stages import HANDLERS
from pipeline.store import STAGES, Store, StaleClaim


def source(store):
    path = store.directory / 'source'
    with (store.directory / 'source.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        if not (path / '.git').exists():
            subprocess.run(['git', 'clone', '--quiet', '--shared', str(ROOT), str(path)], check=True, timeout=120)
            subprocess.run(['git', 'remote', 'set-url', 'origin', store.setting('repo_url', 'https://github.com/jsell-rh/gyre.git')], cwd=path, check=True)
        subprocess.run(['git', 'config', 'remote.origin.promisor', 'true'], cwd=path, check=True)
        subprocess.run(['git', 'config', 'remote.origin.partialclonefilter', 'blob:none'], cwd=path, check=True)
        subprocess.run(['git', 'fetch', '--quiet', '--filter=blob:none', 'origin', 'main'], cwd=path, check=True, timeout=120)
        subprocess.run(['git', 'checkout', '--quiet', '--detach', 'origin/main'], cwd=path, check=True)
        revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=path, text=True).strip()
        sync_checkout(store, path, revision)
        return revision


def discover_cleanup(store):
    from pipeline.execution import gateway
    os.environ.update(OPENSHELL_NO_BROWSER='1', OPENSHELL_GATEWAY_INSECURE='true', OPENSHELL_WORKSPACE='default')
    gateway.login(store.directory)
    inventory = gateway.inventory()
    observed = {item['name'] for item in inventory}
    with store.transaction():
        for row in store.db.execute("SELECT name,work,state FROM resources WHERE kind='sandbox' AND state<>'absent'").fetchall():
            # An unobserved creation intent still reserves compute. A request
            # may be in flight or have lost its response; cleanup must delete
            # that namespace before another reservation can replace it.
            if row['name'] not in observed and row['work'] is None:
                store.db.execute("UPDATE resources SET state='absent',updated=? WHERE name=?", (time.time(), row['name']))
        store.event('gateway_inventory', {'sandboxes': len(inventory)})
    selected = []
    rows = store.db.execute('''SELECT r.*,w.state AS work_state,w.expires FROM resources r
                              LEFT JOIN work w ON w.id=r.work WHERE r.state<>'absent' ''').fetchall()
    for row in rows:
        if row['work'] is None:
            continue
        if row['work_state'] == 'claimed' and (row['expires'] or 0) > time.time() and row['state'] != 'deleting':
            continue
        task = store.task(store.db.execute('SELECT task FROM work WHERE id=?', (row['work'],)).fetchone()['task'])
        selected.append(store.enqueue('cleanup', task['name'], task['generation'],
                                      {'resource': row['name'], 'resource_token': row['token']}, priority=10000))
    return selected


def discover_stage(store, stage, base):
    return discover_cleanup(store) if stage == 'cleanup' else discover(store, stage, base)


def run_one(store, stage, task=None):
    limit = store.setting('limits', {}).get(stage, 1 if stage == 'verify' else max(1, store.setting('slots', 8)))
    if stage in ('implement', 'review', 'triage') and store.setting('slots', 8) == 0:
        return False
    claim = store.claim(stage, f'{socket.gethostname()}:{os.getpid()}:{uuid.uuid4().hex[:8]}',
                        concurrency=limit, task=task, eligible=lambda work: eligible(store, work))
    if not claim:
        return False
    execution = Execution(store, claim)
    try:
        with execution.heartbeat():
            item = store.task(claim['task'])
            if item['data'].get('task_override'):
                item['body'] = item['data']['task_override']
            result, updates, findings = HANDLERS[stage](execution, item)
            execution.current()
            if not eligible(store, claim):
                raise StaleClaim('stage preconditions changed during execution')
            body, generation = None, None
            if stage in ('verify', 'publish') and 'dependencies' in updates:
                # A prerequisite is desired task metadata, not an ephemeral
                # field an implementation body can overwrite on completion.
                import re
                declaration = 'depends_on: [' + ', '.join(updates['dependencies']) + ']'
                body, count = re.subn(r'(?m)^depends_on:[^\n]*(?:\n[ \t]+-[^\n]*)*', declaration, item['body'], count=1)
                if count != 1:
                    raise ValueError('cannot persist prerequisite without a unique dependency field')
                generation = generation_for(store, body)
                updates['triaged'] = None
            if stage in ('triage', 'implement') and result.get('task_body'):
                body = result['task_body']
                generation = generation_for(store, body)
                updates.update(metadata(body))
                updates['task_override'] = None
                updates['triaged'] = generation if stage == 'triage' else (
                    item['data'].get('triaged') if generation == item['generation'] else None)
            receipt = {key: value for key, value in result.items() if key != 'task_body'}
            store.finish(claim['id'], claim['token'], receipt, updates, findings, body, generation)
            phase = 'Merged' if updates.get('delivered') else 'NeedsRevision' if updates.get('repair') else 'Completed'
            (execution.directory / 'phase.json').write_text(json.dumps({'phase': phase, 'at': time.time()}))
    except StaleClaim:
        # Another owner or a changed contract fenced this worker. Resources
        # remain reserved until cleanup observes their absence.
        return True
    except Exception as exc:
        try:
            receipt = execution.directory / 'recovery.json'
            if stage == 'implement' and receipt.exists():
                from pipeline.recovery import restore
                repository = store.directory / 'recovery.git'
                with (store.directory / 'recovery.lock').open('a') as lock:
                    fcntl.flock(lock, fcntl.LOCK_EX)
                    if not repository.exists():
                        subprocess.run(['git', 'clone', '--quiet', '--bare', '--shared', str(ROOT), str(repository)], check=True)
                        subprocess.run(['git', 'remote', 'set-url', 'origin', store.setting('repo_url', 'https://github.com/jsell-rh/gyre.git')], cwd=repository, check=True)
                    outcome = restore(execution.directory, repository,
                                      getattr(execution, 'branch', f"pipeline/{claim['task']}/{claim['id']}-{claim['token']}"))
                metadata_receipt = json.loads(receipt.read_text())
                body = subprocess.check_output(['git', 'show', outcome['head'] + ':specs/tasks/' + claim['task'] + '.md'], cwd=repository, text=True)
                finding = {'category': 'checkpoint', 'source': outcome['head'], 'id': claim['id'],
                           'detail': 'Recovered an interrupted assignment; implementation must finish and obtain fresh review.'}
                store.finish(claim['id'], claim['token'], outcome,
                             {'candidate': outcome['head'], 'candidate_base': metadata_receipt['base'],
                              'branch': outcome['branch'], 'task_override': body, 'repair': finding,
                              'review': None, 'verified': None}, [finding])
                return True
            store.retry(claim['id'], claim['token'],
                        {'category': 'infrastructure' if isinstance(exc, (Retry, OSError, subprocess.TimeoutExpired)) else 'execution',
                         'message': str(exc)[-4000:], 'fresh_model': getattr(exc, 'fresh_model', False)})
        except StaleClaim:
            pass
        except Exception as recovery_error:
            try:
                store.retry(claim['id'], claim['token'],
                            {'category': 'checkpoint', 'message': str(exc)[-2000:],
                             'recovery_error': str(recovery_error)[-2000:]})
            except StaleClaim:
                pass
        print(f'{stage}/{claim["task"]}: retry scheduled: {exc}', file=sys.stderr)
    return True


def status(store):
    tasks = store.tasks()
    work = [dict(row) for row in store.db.execute('SELECT * FROM work ORDER BY created DESC')]
    for row in work:
        row['input'] = json.loads(row['input'])
        row['result'] = json.loads(row['result']) if row['result'] else None
    resources = [dict(row) for row in store.db.execute("SELECT * FROM resources WHERE state<>'absent'")]
    return {'tasks': tasks, 'work': work, 'resources': resources,
            'slots': store.setting('slots', 8),
            'stages': {stage: {state: sum(row['stage'] == stage and row['state'] == state for row in work)
                               for state in ('ready', 'claimed', 'retry', 'succeeded', 'obsolete')} for stage in STAGES}}


def serve(store, interval, only_task=None):
    children = []
    stopping = False
    def stop(*_):
        nonlocal stopping
        stopping = True
    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    with (store.directory / 'supervisor.lock').open('a+') as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise RuntimeError('another pipeline supervisor is running')
        lock.seek(0)
        lock.truncate()
        lock.write(str(os.getpid()))
        lock.flush()
        store.set_setting('only_task', only_task)
        while not stopping:
            try:
                base = source(store)
                for stage in STAGES:
                    discover_stage(store, stage, base)
                children = [child for child in children if child.poll() is None]
                allowed = None
                if only_task:
                    catalog = {task['name']: task['data'] for task in store.tasks()}
                    allowed = {only_task}
                    pending = [only_task]
                    while pending:
                        for dependency in catalog.get(pending.pop(), {}).get('dependencies') or []:
                            if dependency not in allowed:
                                allowed.add(dependency)
                                pending.append(dependency)
                query = "SELECT stage,task,count(*) AS n FROM work WHERE state IN ('ready','retry') AND retry_at<=? GROUP BY stage,task"
                ready = store.db.execute(query, (time.time(),)).fetchall()
                for row in ready:
                    stage = row['stage']
                    if allowed is not None and row['task'] not in allowed:
                        continue
                    limit = store.setting('limits', {}).get(stage, 1 if stage == 'verify' else max(1, store.setting('slots', 8)))
                    live = sum(getattr(child, 'stage', None) == stage for child in children)
                    for _ in range(max(0, min(row['n'], limit - live))):
                        command = [sys.executable, str(Path(__file__).resolve()), '--state', str(store.directory), 'run', stage]
                        command += ['--task', row['task']]
                        child = subprocess.Popen(command, start_new_session=True)
                        child.stage = stage
                        children.append(child)
            except Exception as exc:
                print(f'discovery retry: {exc}', file=sys.stderr)
            time.sleep(interval)
        # Independent workers keep their leases and finish on supervisor stop.
        # A new supervisor discovers and adopts their durable claims.


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--state', type=Path, default=Path(os.environ.get('GYRE_PIPELINE_STATE', ROOT / '.gyre-pipeline')))
    commands = parser.add_subparsers(dest='command', required=True)
    for name in ('discover', 'run', 'tick'):
        command = commands.add_parser(name)
        command.add_argument('stage', choices=STAGES)
        command.add_argument('--task')
    command = commands.add_parser('serve')
    command.add_argument('--interval', type=float, default=15)
    command.add_argument('--task')
    commands.add_parser('status').add_argument('--json', action='store_true')
    commands.add_parser('retry').add_argument('task')
    commands.add_parser('retry-all')
    commands.add_parser('slots').add_argument('count', type=int)
    command = commands.add_parser('seed')
    command.add_argument('task_file', type=Path)
    command.add_argument('--candidate')
    command.add_argument('--base')
    command.add_argument('--pr')
    args = parser.parse_args()
    store = Store(args.state)
    credentials = store.directory / 'gateway.env'
    if credentials.exists():
        if credentials.stat().st_mode & 0o077:
            raise RuntimeError('gateway.env must have mode 0600')
        import shlex
        for line in credentials.read_text().splitlines():
            if not line.strip() or line.lstrip().startswith('#'):
                continue
            key, value = line.removeprefix('export ').split('=', 1)
            if key not in ('OPENSHELL_OIDC_CLIENT_SECRET', 'GITHUB_TOKEN', 'GH_TOKEN', 'ENMAAS_API_KEY'):
                raise ValueError('unsupported credential field')
            words = shlex.split(value)
            if len(words) != 1:
                raise ValueError('invalid credential value')
            os.environ.setdefault(key, words[0])
    try:
        if args.command == 'status':
            from pipeline.cockpit import snapshot
            print(json.dumps(snapshot(store)))
        elif args.command in ('retry', 'retry-all'):
            with store.transaction():
                query = "UPDATE work SET state='ready',retry_at=0 WHERE state IN ('attention','retry')"
                params = ()
                if args.command == 'retry':
                    query += ' AND task=?'
                    params = (args.task,)
                count = store.db.execute(query, params).rowcount
                store.event('operator_retry', {'count': count}, getattr(args, 'task', None))
            print(f'retried {count} work items')
        elif args.command == 'slots':
            if not 0 <= args.count <= 1000:
                parser.error('slots must be 0–1000')
            store.set_setting('slots', args.count)
        elif args.command == 'serve':
            if not 0 < args.interval <= 60:
                parser.error('interval must be 0–60 seconds')
            serve(store, args.interval, args.task)
        elif args.command == 'seed':
            import re
            for value in (args.candidate, args.base):
                if value is not None and not re.fullmatch(r'[a-f0-9]{40}', value):
                    parser.error('candidate and base must be full Git commit SHAs')
            body = args.task_file.read_text()
            name = args.task_file.stem
            if not re_task(name):
                parser.error('task file must be named task-NNN.md')
            from pipeline.catalog import contract
            specs = {str(path.relative_to(ROOT)): path.read_text() for section in ('system', 'development')
                     for path in (ROOT / 'specs' / section).glob('*.md')}
            generation = contract.generation(body, specs, (ROOT / 'specs/GOAL.md').read_text())
            data = metadata(body) | {'catalog_generation': generation}
            existing = next((task for task in store.tasks() if task['name'] == name), None)
            if existing:
                data['catalog_generation'] = existing['data'].get('catalog_generation', existing['generation'])
            if args.candidate:
                if not args.base:
                    parser.error('--candidate requires --base')
                data.update(candidate=args.candidate, candidate_base=args.base)
                if data['progress'] not in ('ready-for-review', 'complete'):
                    data['repair'] = {'category': 'resume', 'source': args.candidate,
                                      'id': generation, 'detail': 'Finish the retained implementation checkpoint before independent review.'}
            if args.pr:
                import re
                repository = store.setting('repository', 'jsell-rh/gyre')
                if not re.fullmatch(r'https://github.com/' + re.escape(repository) + r'/pull/\d+', args.pr):
                    parser.error('PR must belong to the configured repository')
                info = json.loads(subprocess.check_output(['gh', 'pr', 'view', args.pr, '--json',
                                                           'state,headRefName,headRefOid'], text=True))
                if info['state'] != 'OPEN' or info['headRefOid'] != args.candidate:
                    parser.error('PR must be open at the supplied candidate SHA')
                data.update(pr=args.pr, delivery_branch=info['headRefName'], published_head=info['headRefOid'])
            store.put_task(name, generation, body, data)
        else:
            if args.command != 'run':
                base = source(store)
                print(json.dumps(discover_stage(store, args.stage, base)))
            if args.command != 'discover':
                run_one(store, args.stage, args.task)
    finally:
        store.close()


def re_task(name):
    import re
    return re.fullmatch(r'task-\d+', name)


if __name__ == '__main__':
    main()
