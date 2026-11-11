#!/usr/bin/env python3
"""Gyre domain adapter for the extracted Fullsend dispatcher.

The existing pipeline ledger owns claims, checkpoints, findings and resources.
This protocol never starts a second scheduling loop or imports HyperShell state.
"""
import argparse
import contextlib
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import time
import uuid

from pipeline.catalog import helper
from pipeline.credentials import load
from pipeline.eligibility import eligible
from pipeline.execution import ROOT, gateway
from pipeline.store import Store, StaleClaim, STAGES
from pipeline.settings import configuration, save_configuration

pipeline = helper('dev-pipeline')
COMPUTE = {'triage', 'assess', 'implement', 'review', 'verify'}
AGENTS = {'implement': 'code-implementation', 'review': 'code-review'}


def scope(store):
    return 'gyre/' + store.setting('owner')


def allowed_tasks(store):
    only = store.setting('only_task')
    if not only:
        return None
    graph = {t['name']: t['data'].get('dependencies') or [] for t in store.tasks()}
    result, pending = {only}, [only]
    while pending:
        for name in graph.get(pending.pop(), []):
            if name not in result:
                result.add(name)
                pending.append(name)
    return result


def job(store, row, tasks=None):
    from pipeline.priority import info
    tasks = tasks or {t['name']: t['data'] for t in store.tasks()}
    item = row['task']
    inputs = row['input'] if isinstance(row['input'], dict) else json.loads(row['input'])
    if row['stage'] == 'cleanup':
        item += '/' + inputs['resource']
    priority=info(store,row)
    return {'key': row['id'], 'scope': scope(store), 'agent': AGENTS.get(row['stage'], row['stage']),
            'item': item, 'title': tasks[row['task']].get('title') or row['task'],
            'url': 'https://github.com/' + store.setting('repository', 'jsell-rh/gyre') + '/blob/main/specs/tasks/' + row['task'] + '.md',
            'repo': store.setting('repository', 'jsell-rh/gyre'),
            'fingerprint': row['generation'] + ':' + row['input_key'] + ':' + priority['class'] + ':' + str(priority['value']),
            'labels': ['priority/'+priority['class']], 'priority_class':priority['class'],
            'priority_value':priority['value'], 'effective_priority':priority['effective'],
            'pr_number': '', 'pr_url': tasks[row['task']].get('pr') or '',
            'unmetered': row['stage'] not in COMPUTE}


def read_rows(store):
    tasks = {t['name']: t['data'] for t in store.tasks()}
    rows = store.db.execute('''SELECT * FROM (
        SELECT w.*,row_number() OVER (PARTITION BY w.task,w.stage ORDER BY w.updated DESC) AS rank
        FROM work w JOIN tasks t ON t.name=w.task AND t.generation=w.generation
        WHERE w.state<>'obsolete') WHERE state<>'succeeded' OR rank=1 ORDER BY updated DESC''').fetchall()
    result = []
    for raw in rows:
        r = dict(raw)
        status = {'claimed': 'running', 'retry': 'failed', 'attention': 'uncertain',
                  'ready': 'queued', 'succeeded': 'succeeded'}[r['state']]
        outcome = json.loads(r['result']) if r['result'] else {}
        if status == 'failed' and outcome.get('category') == 'observation':
            status = 'waiting'
        canceled = store.setting(f"cancel:{r['id']}:{r['token']}", False)
        if canceled and status != 'running':
            status = 'canceled'
        started = store.db.execute("SELECT at FROM events WHERE work=? AND kind='claimed' ORDER BY id DESC LIMIT 1", (r['id'],)).fetchone()
        readiness = None
        ci = tasks[r['task']].get('ci') or {}
        inputs = json.loads(r['input'])
        bound = (inputs.get('published') or inputs.get('verified') or {}).get('head')
        if r['stage'] == 'review':
            published = tasks[r['task']].get('published_verified') or {}
            if published.get('candidate') == inputs.get('candidate'):
                bound = published.get('head')
        if ci.get('head') and bound == ci['head']:
            readiness = {key:ci.get(key) for key in ('status','merge_state')}
            readiness['repair_queued'] = bool(tasks[r['task']].get('repair'))
        result.append(job(store, r, tasks) | {'status': status, 'failures': r['failures'],
            'code_readiness':readiness,
            'next_at': r['retry_at'], 'started': started['at'] if started else r['created'],
            'finished': r['updated'] if status not in ('running','queued') else 0,
            'run_id': f"{r['id']}:{r['token']}",
            'log': str(store.directory/'attempts'/r['id']/str(r['token'])/'output.log'),
            'error': outcome.get('message', ''), 'cancel_requested': canceled})
    return result


def authorize(store, request):
    if request.get('scope') != scope(store):
        return False
    row = store.db.execute('SELECT * FROM work WHERE id=?', (request['key'],)).fetchone()
    if not row or row['state'] not in ('ready','retry') or row['retry_at'] > time.time():
        return False
    selected = allowed_tasks(store)
    if selected is not None and row['task'] not in selected:
        return False
    if row['stage'] in COMPUTE and (store.setting('admission_paused', False) or store.setting('slots', 8) == 0):
        return False
    return (request.get('fingerprint') == job(store,dict(row))['fingerprint']
            and store.admission_available(row['stage']) and eligible(store, dict(row)))


def launch(store, request):
    if not authorize(store, request):
        return False
    stage = store.db.execute('SELECT stage FROM work WHERE id=?', (request['key'],)).fetchone()['stage']
    limit = store.setting('limits', {}).get(stage, max(1, store.setting('slots', 8)))
    claim = store.claim(stage, f'gyre-dispatcher:{socket.gethostname()}:{uuid.uuid4().hex}', concurrency=limit,
                        ident=request['key'], eligible=lambda w: eligible(store, w), capacity=store.setting('slots', 8))
    if not claim:
        return False
    directory = store.directory/'attempts'/claim['id']/str(claim['token'])
    directory.mkdir(parents=True, exist_ok=True, mode=0o700)
    try:
        with (directory/'worker.log').open('ab') as output:
            process = subprocess.Popen([sys.executable, str(ROOT/'scripts/dev-pipeline.py'), '--state', str(store.directory),
                                        'execute', claim['id'], str(claim['token'])], cwd=ROOT,
                                       stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
        with store.transaction():
            store.event('dispatcher_worker', {'pid': process.pid, 'token': claim['token']}, claim['task'], claim['id'])
    except Exception:
        store.retry(claim['id'], claim['token'], {'category':'infrastructure','message':'detached worker could not start'})
        store.resource_state(store.admission_name(claim['id'], claim['token']), 'absent')
        raise
    return True


def recover(store):
    with store.transaction():
        rows = store.db.execute("SELECT id,task FROM work WHERE state='claimed' AND expires<=?", (time.time(),)).fetchall()
        for row in rows:
            store.db.execute("UPDATE work SET state='ready',token=token+1,owner=NULL,expires=NULL,updated=?,ready_since=? WHERE id=?", (time.time(),time.time(), row['id']))
            store.event('lease_expired', {}, row['task'], row['id'])


def control(store, request, cancel=False):
    if request.get('scope') != scope(store):
        raise ValueError('work belongs to a different dispatcher scope')
    with store.transaction():
        row = store.db.execute('SELECT * FROM work WHERE id=?', (request['key'],)).fetchone()
        if not row or request.get('run_id', f"{row['id']}:{row['token']}") != f"{row['id']}:{row['token']}":
            raise ValueError('attempt changed; refresh the run list')
        if cancel:
            if row['state'] != 'claimed' or row['stage'] not in COMPUTE:
                raise ValueError('only active implementation, review, triage, or verification can be canceled')
            if not (row['owner'] or '').startswith('gyre-dispatcher:'):
                raise ValueError('This worker predates dispatcher cancellation; it is being adopted for monitoring and must finish normally')
            store.db.execute('INSERT OR REPLACE INTO settings VALUES(?,?)', ('task_paused:'+row['task'], 'true'))
            store.db.execute('INSERT OR REPLACE INTO settings VALUES(?,?)', (f"cancel:{row['id']}:{row['token']}", 'true'))
        else:
            if row['state'] == 'claimed':
                raise ValueError('cannot retry an active attempt')
            if row['state'] not in ('attention','retry') and not store.setting(f"cancel:{row['id']}:{row['token']}",False):
                raise ValueError('only a failed or canceled attempt can be retried')
            store.db.execute("UPDATE work SET state='ready',retry_at=0,ready_since=? WHERE id=? AND state IN ('attention','retry')", (time.time(),row['id']))
            store.db.execute('DELETE FROM settings WHERE key=?', (store.runtime_hold_key(row['task'],row['stage'],row['generation']),))
            store.db.execute('DELETE FROM settings WHERE key=?', ('task_paused:'+row['task'],))
            store.db.execute('DELETE FROM settings WHERE key=?', (f"cancel:{row['id']}:{row['token']}",))
        store.event('operator_cancel' if cancel else 'operator_retry', {'token':row['token']}, row['task'], row['id'])


def main():
    action = sys.argv[1]
    parser = argparse.ArgumentParser()
    parser.add_argument('--state', type=Path, required=True)
    parser.add_argument('--interval', type=float, default=15)
    parser.add_argument('--task')
    parser.add_argument('--plain', action='store_true')
    args = parser.parse_args(sys.argv[2:])
    store = Store(args.state)
    load(store)
    try:
        request = json.load(sys.stdin) if action in ('authorize','launch','retry','cancel','history','settings-save') else None
        if action == 'config':
            result = {'adapter_backend': True, 'scope':scope(store), 'watch':True, 'plain':args.plain,
                'concurrency':store.setting('slots',8), 'poll_interval':max(1, int(args.interval)),
                'auth_retry':15, 'auth_check_interval':60, 'backoff':5, 'backoff_max':900,
                'repo':store.setting('repository','jsell-rh/gyre'), 'state_dir':str(store.directory/'dispatcher'),
                'settings_file':str(store.directory/'pipeline.sqlite3'), 'settings':configuration(store),
                'supervisor_lock':str(store.directory/'supervisor.lock'), 'log_dir':str(store.directory/'dispatcher/logs'),
                'gateway_name':store.setting('gateway','gyre-gyre'),
                'environment':{'OPENSHELL_GATEWAY':store.setting('gateway','gyre-gyre'), 'OPENSHELL_GATEWAY_ENDPOINT':None}}
        elif action == 'settings-save':
            result = save_configuration(store, request)
        elif action == 'activate':
            store.set_setting('only_task', args.task)
            store.set_setting('dispatcher_engine', 'fullsend-dispatcher')
            result = {}
        elif action == 'rows':
            from pipeline.runtime import observe
            observe(store)
            result = {'rows':read_rows(store), 'capacity':store.setting('slots',8), 'settings':configuration(store)}
        elif action == 'recover':
            recover(store); result = {}
        elif action in ('authorize','launch'):
            result = {'allowed':authorize(store, request)} if action == 'authorize' else {'started':launch(store, request)}
        elif action in ('health','gateway'):
            from pipeline.isolation import isolate_gateway, ensure_github_provider
            previous = store.setting('gateway','gyre-gyre')
            selected = isolate_gateway(store)
            os.environ.update(GYRE_OPEN_GATEWAY=selected, OPENSHELL_GATEWAY=selected, OPENSHELL_NO_BROWSER='1', OPENSHELL_GATEWAY_INSECURE='true')
            if action == 'gateway' or previous != selected or time.time()-store.setting('dispatcher_health_at',0)>30:
                gateway.login(store.directory,force=action=='gateway')
                if previous != selected:
                    # Publish the alias only after its independent login works;
                    # in-flight hooks must never be redirected to an unauthenticated registration.
                    with store.transaction():
                        store.db.execute('INSERT OR REPLACE INTO settings VALUES(?,?)',
                            ('dispatcher_gateway_registration',json.dumps({'source':previous,'alias':selected})))
                        store.db.execute('INSERT OR REPLACE INTO settings VALUES(?,?)',('gateway',json.dumps(selected)))
                ensure_github_provider(store)
                store.set_setting('dispatcher_health_at',time.time())
            result = {}
        elif action == 'discover':
            with contextlib.redirect_stdout(sys.stderr):
                base = pipeline.source(store)
                for stage in STAGES:
                    pipeline.discover_stage(store, stage, base)
            selected = allowed_tasks(store)
            tasks = {t['name']:t['data'] for t in store.tasks()}
            rows = store.ordered_ready()
            result = {'jobs':[job(store,r,tasks) for r in rows if (selected is None or r['task'] in selected)
                      and eligible(store,r) and store.admission_available(r['stage'])], 'errors':[]}
        elif action == 'retry':
            control(store,request); result = {}
        elif action == 'cancel':
            control(store,request,cancel=True); result = {}
        elif action == 'history':
            if request.get('scope') != scope(store):
                raise ValueError('history belongs to another dispatcher scope')
            task = request['item'].split('/')[0]
            records = {r['id']:dict(r) for r in store.db.execute('SELECT * FROM work WHERE task=?',(task,))}
            result = []
            for event in store.db.execute("SELECT work,at,detail FROM events WHERE task=? AND kind='claimed' ORDER BY id DESC LIMIT 200",(task,)):
                row = records[event['work']];token=json.loads(event['detail'])['token']
                result.append(job(store,row) | {'status':row['state'] if token==row['token'] else 'fenced',
                    'key':f"{row['id']}:{token}", 'run_id':f"{row['id']}:{token}", 'started':event['at'],
                    'log':str(store.directory/'attempts'/row['id']/str(token)/'output.log'), 'activity':'history'})
        else:
            raise ValueError('unknown adapter operation')
        print(json.dumps(result))
    finally:
        store.close()


if __name__ == '__main__':
    try:
        main()
    except Exception as exc:
        # Never send credential values or model transcripts into dispatcher diagnostics.
        message = str(exc)
        for key in ('GH_TOKEN','GITHUB_TOKEN','ENMAAS_API_KEY','OPENSHELL_OIDC_CLIENT_SECRET'):
            if os.environ.get(key): message = message.replace(os.environ[key], '[redacted]')
        print(message, file=sys.stderr)
        sys.exit(1)
