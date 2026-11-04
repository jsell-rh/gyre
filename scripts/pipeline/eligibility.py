"""Recheck stage preconditions inside claim and before recording outcomes."""
import json
import time


def eligible(store, work):
    from .repairs import probe, satisfied
    task = store.task(work['task'])
    data = task['data']
    stage = work['stage']
    inputs = work['input'] if isinstance(work['input'], dict) else json.loads(work['input'])
    if task['generation'] != work['generation']:
        return False
    if stage == 'cleanup':
        row = store.db.execute('SELECT state FROM resources WHERE name=?', (inputs['resource'],)).fetchone()
        # A retry may still need to restore a captured checkpoint after the
        # sandbox was deleted. Deletion and recovery have separate receipts.
        return row is not None
    if store.setting('task_paused:' + task['name'], False):
        return False
    if satisfied(data):
        return False
    if stage == 'triage':
        if inputs.get('baseline_probe'):
            return (probe(task) == inputs['baseline_probe'] and
                    (data.get('baseline_checked') or {}).get('base') != inputs['base'])
        return data.get('triaged') != task['generation']
    if probe(task) and (data.get('baseline_checked') or {}).get('base') != store.setting('observed_main', inputs.get('base')):
        return False
    if data.get('triaged') != task['generation']:
        return False
    tasks = {task['name']: task for task in store.tasks()}
    if any(dep not in tasks or not satisfied(tasks[dep]['data']) for dep in data.get('dependencies') or []):
        return False
    candidate = data.get('candidate')
    if stage == 'implement':
        return (candidate == inputs.get('candidate') and data.get('repair') == inputs.get('repair')
                and (not candidate or bool(data.get('repair'))))
    if candidate != inputs.get('candidate') or data.get('repair'):
        return False
    if stage == 'review':
        return (data.get('review') or {}).get('candidate') != candidate
    review = data.get('review') or {}
    if review.get('candidate') != candidate or not review.get('approved'):
        return False
    if stage == 'verify':
        verified = data.get('verified') or {}
        return verified.get('candidate') != candidate or verified.get('base') != inputs['base']
    if stage == 'publish':
        return data.get('verified') == inputs['verified']
    return False
