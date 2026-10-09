"""Compact read model for the development cockpit."""
import json
import time

from .catalog import contract
from .store import STAGES


def snapshot(store):
    records = store.tasks()
    work = [dict(row) for row in store.db.execute('SELECT * FROM work ORDER BY created DESC')]
    tasks, attempts, counts = [], [], {}
    graph = {task['name']: task['data'].get('dependencies') or [] for task in records}
    errors = contract.graph_errors(graph)
    delivered = {task['name'] for task in records if task['data'].get('delivered')}
    for task in records:
        data = task['data']
        related = [item for item in work if item['task'] == task['name'] and item['generation'] == task['generation']]
        active = next((item for item in related if item['state'] == 'claimed' and item['expires'] > time.time()), None)
        retry = next((item for item in related if item['state'] == 'retry'), None)
        attention = next((item for item in related if item['state'] == 'attention'), None)
        state = ('merged' if data.get('delivered') else 'failed' if attention else
                 'running' if active else 'deferred' if retry else
                 'blocked' if task['name'] in errors or not set(graph[task['name']]) <= delivered else
                 'ready' if data.get('repair') or not data.get('candidate') else
                 'published' if data.get('pr') else 'candidate')
        condition = errors.get(task['name']) or (json.loads((attention or retry)['result']).get('message') if attention or retry else None)
        tasks.append({'name': task['name'], 'title': data.get('title'), 'spec_ref': data.get('spec_ref'),
                      'deps': graph[task['name']], 'state': state, 'stage': active['stage'] if active else None,
                      'progress': data.get('progress'), 'candidate': data.get('candidate'),
                      'seed': data.get('candidate'), 'generation': task['generation'],
                      'observed_generation': task['generation'] if data.get('delivered') else None,
                      'merge_sha': (data.get('delivered') or {}).get('sha'), 'pr': data.get('pr'),
                      'condition': condition, 'feedback': data.get('repair'),
                      'retry_at': retry['retry_at'] if retry else None, 'attempts': len(related)})
        counts[state] = counts.get(state, 0) + 1
    for item in work[:300]:
        ident = f"{item['id']}-{item['token']}"
        path = store.directory / 'attempts' / item['id'] / str(item['token'])
        phase = {}
        try:
            phase = json.loads((path / 'phase.json').read_text())
        except (OSError, ValueError):
            pass
        result = json.loads(item['result']) if item['result'] else {}
        result.pop('task_body', None)
        attempts.append({'id': ident, 'task': item['task'], 'kind': item['stage'],
                         'state': 'running' if item['state'] == 'claimed' else item['state'],
                         'started': item['created'], 'phase': phase.get('phase'),
                         'reason': phase.get('reason'), 'detail': json.dumps(result)[:3000]})
    resources = store.db.execute("SELECT state,count(*) FROM resources WHERE kind='sandbox' AND state<>'absent' GROUP BY state").fetchall()
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
            'resources': {'used': sum(row[1] for row in resources), 'deletion_pending': dict(resources).get('deleting', 0)},
            'health': {'condition': store.setting('admission_condition', 'Reconciling'), 'effective_slots': store.setting('slots', 8)},
            'dispatch': {'only_task': store.setting('only_task')}}
