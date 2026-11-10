"""Compact read model for the development cockpit."""
import json
import time

from .catalog import contract
from .store import STAGES


def snapshot(store):
    from .repairs import satisfied
    from .priority import task_priority, classes, PriorityError
    from .execution import gateway
    records = store.tasks()
    work = [dict(row) for row in store.db.execute('SELECT * FROM work ORDER BY created DESC')]
    tasks, attempts, counts = [], [], {}
    graph = {task['name']: task['data'].get('dependencies') or [] for task in records}
    errors = contract.graph_errors(graph)
    delivered = {task['name'] for task in records if satisfied(task['data'])}
    for task in records:
        data = task['data']
        try:
            priority=task_priority(store,task)
        except PriorityError as exc:
            priority={'class':'invalid','error':str(exc),'source':'invalid'}
        related = [item for item in work if item['task'] == task['name'] and item['generation'] == task['generation']]
        active = next((item for item in related if item['stage'] != 'observe' and item['state'] == 'claimed' and item['expires'] > time.time()), None)
        retry = next((item for item in related if item['stage'] != 'observe' and item['state'] == 'retry'), None)
        attention = next((item for item in related if item['state'] == 'attention'), None)
        shared_wait = (data.get('repair') or {}).get('shared_environment')
        state = ('merged' if data.get('delivered') else 'resolved' if data.get('resolved') else 'failed' if attention else
                 'running' if active else 'published' if retry and retry['stage'] == 'publish' and data.get('pr') else 'deferred' if retry else
                 'blocked' if shared_wait or task['name'] in errors or not set(graph[task['name']]) <= delivered else
                 'ready' if data.get('repair') or not data.get('candidate') else
                 'published' if data.get('pr') else 'candidate')
        condition = priority.get('error') or (data.get('resolved') or {}).get('reason') or errors.get(task['name']) or (json.loads((attention or retry)['result']).get('message') if attention or retry else None)
        if shared_wait and not condition:
            condition = 'Waiting for shared CI environment repair: ' + data['repair']['prerequisite']
        tasks.append({'name': task['name'], 'title': data.get('title'), 'spec_ref': data.get('spec_ref'),
                      'deps': graph[task['name']], 'state': state, 'stage': active['stage'] if active else None,
                      'progress': data.get('progress'), 'candidate': data.get('candidate'),
                      'seed': data.get('candidate'), 'generation': task['generation'],
                      'observed_generation': task['generation'] if data.get('delivered') else None,
                      'merge_sha': (data.get('delivered') or {}).get('sha'), 'pr': data.get('pr'),
                      'condition': condition, 'feedback': data.get('repair'), 'ci':data.get('ci'),'priority':priority,
                      'retry_at': retry['retry_at'] if retry else None, 'attempts': len(related)})
        counts[state] = counts.get(state, 0) + 1
    by_id = {item['id']: item for item in work}
    claims = store.db.execute("SELECT work,at,detail FROM events WHERE kind='claimed' ORDER BY id DESC LIMIT 300").fetchall()
    for event in claims:
        item = by_id[event['work']]
        token = json.loads(event['detail'])['token']
        ident = f"{item['id']}-{token}"
        path = store.directory / 'attempts' / item['id'] / str(token)
        phase = {}
        try:
            phase = json.loads((path / 'phase.json').read_text())
        except (OSError, ValueError):
            pass
        result = json.loads(item['result']) if item['result'] else {}
        result.pop('task_body', None)
        attempts.append({'id': ident, 'task': item['task'], 'kind': item['stage'],
                         'state': ('fenced' if token != item['token'] else
                                   'running' if item['state'] == 'claimed' else item['state']),
                         'started': event['at'], 'phase': phase.get('phase'),
                         'reason': phase.get('reason'), 'detail': json.dumps(result)[:3000]})
    resources = store.db.execute("SELECT state,count(*) FROM resources WHERE kind IN ('sandbox','admission') AND state<>'absent' GROUP BY state").fetchall()
    inventory = store.db.execute("SELECT at FROM events WHERE kind='gateway_inventory' ORDER BY id DESC LIMIT 1").fetchone()
    events = []
    for row in store.db.execute('SELECT * FROM events ORDER BY id DESC LIMIT 100'):
        detail = json.loads(row['detail'])
        detail.pop('task_body', None)
        events.append(dict(row) | {'detail': detail, 'message': row['kind'] + ': ' + json.dumps(detail)[:2000]})
    stages = {stage: {state: sum(item['stage'] == stage and item['state'] == state for item in work)
                      for state in ('ready', 'claimed', 'retry', 'succeeded', 'attention')}
              for stage in STAGES}
    return {'tasks': tasks, 'attempts': attempts, 'events': events, 'counts': counts,
            'running': counts.get('running', 0), 'eligible': sum(task['state'] == 'ready' for task in tasks),
            'slots': store.setting('slots', 8), 'stages': stages,
            'confirmed_merges': sum(bool(task['merge_sha']) for task in tasks),
            'resources': {'used': store.compute_used(), 'deletion_pending': dict(resources).get('deleting', 0),
                          'inventory_ready': bool(inventory and time.time() - inventory['at'] < 180)},
            'health': {'condition': 'Paused' if store.setting('admission_paused', False) else store.setting('admission_condition', 'Reconciling'),
                       'gateway_auth':gateway.auth_status(store.directory,store.setting('gateway','gyre-gyre')),
                       'inference':{key:value for key,value in store.setting('inference_health',{}).items() if key in ('condition','expires','failures')},
                       'effective_slots': 0 if store.setting('admission_paused', False) else store.setting('slots', 8)},
            'dispatch': {'only_task': store.setting('only_task'), 'integration':store.integration_lane(),
                         'verification_reserved':store.verification_reserve(),'priority_classes':classes(store),
                         'integration_waiting':[task['name'] for task in records if not satisfied(task['data']) and store.integration_waiting(task['data'])]}}
