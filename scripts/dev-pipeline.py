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
from pipeline.execution import Execution, ROOT, Retry, Wait
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
        recovery = store.setting('requirements_recovery_revision')
        if recovery:
            from pipeline.requirement_migration import plan, apply
            apply(store, plan(store, path, recovery))
        from pipeline.fulfillment import reconcile
        reconcile(store, path, revision)
        from pipeline.repair_budget import reconcile_generated
        reconcile_generated(store)
        from pipeline.integration import resolve_upstream_assessments
        resolve_upstream_assessments(store,path,revision)
        from pipeline.recovery_policy import reconcile as recover_holds
        recover_holds(store)
        store.set_setting('observed_main', revision)
        return revision


def discover_cleanup(store):
    from pipeline.execution import gateway
    os.environ.update(OPENSHELL_NO_BROWSER='1', OPENSHELL_GATEWAY_INSECURE='true', OPENSHELL_WORKSPACE='default')
    os.environ['GYRE_OPEN_GATEWAY'] = store.setting('gateway', 'gyre-gyre')
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
    limit = store.setting('limits', {}).get(stage, max(1, store.setting('slots', 8)))
    if stage in ('implement', 'assess', 'review', 'triage', 'verify') and (store.setting('slots', 8) == 0 or store.setting('admission_paused', False)):
        return False
    claim = store.claim(stage, f'{socket.gethostname()}:{os.getpid()}:{uuid.uuid4().hex[:8]}',
                        concurrency=limit, task=task, eligible=lambda work: eligible(store, work),
                        capacity=store.setting('slots', 8))
    if not claim:
        return False
    return run_claim(store, claim)


def run_claim(store, claim):
    stage = claim['stage']
    execution = Execution(store, claim)
    try:
        with execution.heartbeat():
            item = store.task(claim['task'])
            if item['data'].get('task_override'):
                from pipeline.catalog import contract
                override = item['data']['task_override']
                try:
                    matches = contract.requirement_parts(override) == contract.requirement_parts(item['body'])
                except ValueError:
                    matches = False
                if matches:
                    item['body'] = override
                else:
                    # Interrupted agents can leave a truncated task document.
                    # Keep their source candidate, but restore the authoritative
                    # requirements before prompt projection or sandbox creation.
                    store.event('checkpoint_contract_restored', {'candidate': item['data'].get('candidate')},
                                claim['task'],claim['id'])
            result, updates, findings = HANDLERS[stage](execution, item)
            execution.current()
            if not eligible(store, claim):
                raise StaleClaim('stage preconditions changed during execution')
            body, generation = None, None
            if stage in ('verify', 'publish', 'observe') and 'dependencies' in updates:
                # A prerequisite is desired task metadata, not an ephemeral
                # field an implementation body can overwrite on completion.
                import re
                declaration = 'depends_on: [' + ', '.join(updates['dependencies']) + ']'
                body, count = re.subn(r'(?m)^depends_on:[^\n]*(?:\n[ \t]+-[^\n]*)*', declaration, item['body'], count=1)
                if count != 1:
                    raise ValueError('cannot persist prerequisite without a unique dependency field')
                generation = generation_for(store, body, item['name'])
                updates['triaged'] = None
            if stage in ('triage', 'implement') and result.get('task_body'):
                body = result['task_body']
                generation = generation_for(store, body, item['name'])
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
            if stage == 'implement' and receipt.exists() and not getattr(exc, 'attention', False):
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
            category = getattr(exc, 'category', None) or ('observation' if isinstance(exc, Wait) else
                       stage if getattr(exc, 'fresh_model', False) else
                       'infrastructure' if isinstance(exc, (Retry, OSError, subprocess.TimeoutExpired)) else 'execution')
            from pipeline.backend_capacity import classify as capacity_failure
            reason=capacity_failure(execution,{'category':category,'message':str(exc)[-4000:],
                'fresh_model':getattr(exc,'fresh_model',False),'diagnostic':getattr(exc,'diagnostic',None)})
            store.retry(claim['id'], claim['token'],
                        reason,
                        delay=getattr(exc, 'retry_delay', None), count_failure=not isinstance(exc, Wait), attention=getattr(exc,'attention',False))
            execution.phase('NeedsAttention' if getattr(exc,'attention',False) else 'RetryScheduled', reason=reason['message'][-1000:], category=reason['category'])
        except StaleClaim:
            pass
        except Exception as recovery_error:
            try:
                store.retry(claim['id'], claim['token'],
                            {'category': 'checkpoint', 'message': str(exc)[-2000:],
                             'recovery_error': str(recovery_error)[-2000:]})
            except StaleClaim:
                pass
        action='needs attention' if getattr(exc,'attention',False) else 'retry scheduled'
        print(f'{stage}/{claim["task"]}: {action}: {exc}', file=sys.stderr)
    finally:
        store.resource_state(store.admission_name(claim['id'], claim['token']), 'absent')
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


def serve(store, interval, only_task=None, plain=False):
    from pipeline.dispatcher import serve as dispatch
    dispatch(store, interval, only_task, plain)


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
    command.add_argument('--plain', action='store_true', help='run without the terminal UI')
    command = commands.add_parser('execute', help=argparse.SUPPRESS)
    command.add_argument('work')
    command.add_argument('token', type=int)
    commands.add_parser('status').add_argument('--json', action='store_true')
    commands.add_parser('retry').add_argument('task')
    commands.add_parser('retry-all')
    commands.add_parser('pause')
    commands.add_parser('resume')
    commands.add_parser('slots').add_argument('count', type=int)
    commands.add_parser('reserve-verification').add_argument('count',type=int)
    commands.add_parser('priority-classes')
    command=commands.add_parser('priority-class')
    command.add_argument('name');command.add_argument('value',type=int)
    command=commands.add_parser('priority')
    command.add_argument('task');command.add_argument('class_name',nargs='?')
    command.add_argument('--clear',action='store_true')
    command = commands.add_parser('seed')
    command.add_argument('task_file', type=Path)
    command.add_argument('--candidate')
    command.add_argument('--base')
    command.add_argument('--pr')
    command.add_argument('--fresh', action='store_true', help='start from main with a new work identity; retain prior checkpoints and receipts')
    args = parser.parse_args()
    store = Store(args.state)
    from pipeline.credentials import load
    load(store)
    try:
        if args.command == 'status':
            from pipeline.cockpit import snapshot
            print(json.dumps(snapshot(store)))
        elif args.command in ('retry', 'retry-all'):
            with store.transaction():
                query = "UPDATE work SET state='ready',retry_at=0,ready_since=? WHERE state IN ('attention','retry')"
                params = (time.time(),)
                if args.command == 'retry':
                    query += ' AND task=?'
                    params += (args.task,)
                for row in store.db.execute("SELECT task,stage,generation FROM work WHERE state IN ('attention','retry')" + (' AND task=?' if args.command=='retry' else ''), (args.task,) if args.command=='retry' else ()).fetchall():
                    store.db.execute('DELETE FROM settings WHERE key=?',(store.runtime_hold_key(row['task'],row['stage'],row['generation']),))
                    from pipeline.repair_budget import reset
                    reset(store,row['task'],row['stage'])
                count = store.db.execute(query, params).rowcount
                store.event('operator_retry', {'count': count}, getattr(args, 'task', None))
            print(f'retried {count} work items')
        elif args.command in ('pause', 'resume'):
            store.set_setting('admission_paused', args.command == 'pause')
            store.event('operator_admission', {'paused': args.command == 'pause'})
        elif args.command == 'slots':
            if not 0 <= args.count <= 1000:
                parser.error('slots must be 0–1000')
            store.set_setting('slots', args.count)
        elif args.command == 'reserve-verification':
            if not 0 <= args.count <= 1000:
                parser.error('verification reservation must be 0–1000')
            store.set_setting('verification_slots',args.count)
        elif args.command == 'priority-classes':
            from pipeline.priority import classes
            print(json.dumps({'classes':classes(store),'default':'normal','preemptionPolicy':'Never'}))
        elif args.command == 'priority-class':
            from pipeline.priority import define,PriorityError
            try:
                define(store,args.name,args.value)
            except PriorityError as exc:
                parser.error(str(exc))
        elif args.command == 'priority':
            from pipeline.priority import set_task,task_priority,PriorityError
            if args.clear and args.class_name is not None:
                parser.error('--clear cannot be combined with a class name')
            try:
                if args.clear or args.class_name is not None:
                    set_task(store,args.task,None if args.clear else args.class_name)
                print(json.dumps(task_priority(store,store.task(args.task))))
            except (PriorityError,KeyError) as exc:
                parser.error(str(exc))
        elif args.command == 'serve':
            if not 0 < args.interval <= 60:
                parser.error('interval must be 0–60 seconds')
            serve(store, args.interval, args.task, args.plain)
        elif args.command == 'execute':
            claim = dict(store._current(args.work, args.token))
            claim['input'] = json.loads(claim['input'])
            run_claim(store, claim)
        elif args.command == 'seed':
            import re
            if args.fresh and any((args.candidate, args.base, args.pr)):
                parser.error('--fresh cannot be combined with --candidate, --base, or --pr')
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
                if args.fresh:
                    if store.db.execute("SELECT 1 FROM work WHERE task=? AND state='claimed'", (name,)).fetchone():
                        parser.error('drain active work for this task before starting fresh')
                    if existing['data'].get('delivered'):
                        parser.error('a delivered task cannot be restarted as fresh work')
                    archive = store.directory / 'task-resets' / (name + '-' + uuid.uuid4().hex + '.json')
                    archive.parent.mkdir(exist_ok=True)
                    archive.write_text(json.dumps(existing, indent=2) + '\n')
                    archive.chmod(0o600)
                    store.event('operator_fresh_start', {'archive': str(archive)}, name)
                elif existing['generation'] == generation and existing['data'].get('triaged') == generation:
                    data['triaged'] = generation
            if args.fresh:
                data['repair'] = {'category': 'fresh_start', 'id': uuid.uuid4().hex,
                                  'detail': 'Implement this task afresh from the assigned current main. Prior checkpoints are archived; do not resume their source or treat historical baseline diagnostics as current failures.'}
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
