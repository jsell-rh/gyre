"""Recheck stage preconditions inside claim and before recording outcomes."""
import json
import time


def eligible(store, work):
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
    if data.get('delivered'):
        return False
    if stage == 'triage':
        return data.get('triaged') != task['generation']
    if data.get('triaged') != task['generation']:
        return False
    tasks = {task['name']: task for task in store.tasks()}
    if any(dep not in tasks or not tasks[dep]['data'].get('delivered') for dep in data.get('dependencies') or []):
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
