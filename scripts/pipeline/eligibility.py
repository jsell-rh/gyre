"""Recheck stage preconditions inside claim and before recording outcomes."""
import json
import time


def eligible(store, work):
    from .repairs import probe, satisfied
    task = store.task(work['task'])
    data = task['data']
    stage = work['stage']
    inputs = work['input'] if isinstance(work['input'], dict) else json.loads(work['input'])
    if store.runtime_held(work):
        return False
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
    if stage in ('triage','assess','implement','review','verify'):
        repair=data.get('repair') or {}
        if repair.get('shared_environment'):
            return False
        from .priority import task_priority, PriorityError
        try:
            task_priority(store,task)
        except PriorityError:
            return False
    requirements = data.get('requirements') or {}
    if requirements.get('errors'):
        return stage == 'triage' and inputs.get('binding_errors') == requirements['errors']
    from .requirements import pending
    assessment = pending(data)
    if assessment:
        # Format migration must let an already-running implementation retain
        # its checkpoint. Discovery never offers another implementation claim.
        if (stage == 'implement' and work.get('state') == 'claimed'
                and not data.get('assessment_pending') and inputs.get('repair') == data.get('repair')):
            return True
        return (stage == 'assess' and inputs.get('requirements') == requirements.get('fingerprint')
                and inputs.get('changes') == assessment.get('changes', {}))
    if stage == 'assess':
        return False
    if stage == 'observe':
        from .ci import publication
        if inputs.get('priority_only'):
            return data.get('pr') == inputs['url']
        return (not data.get('repair') and data.get('candidate') == inputs['candidate']
                and data.get('pr') == inputs['url'] and publication(store,task) == inputs['published'])
    if stage == 'triage':
        if inputs.get('baseline_probe'):
            if (probe(task) != inputs['baseline_probe'] or
                    (data.get('baseline_checked') or {}).get('base') == inputs['base']):
                return False
            for row in store.db.execute("SELECT id,input FROM work WHERE stage='triage' AND state='claimed'"):
                other = json.loads(row['input'])
                if (row['id'] != work['id'] and other.get('base') == inputs['base'] and
                        other.get('baseline_probe') == inputs['baseline_probe']):
                    return False
            return True
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
        retry=data.get('review_retry') if (data.get('review_retry') or {}).get('candidate')==candidate else None
        return (data.get('review') or {}).get('candidate') != candidate and inputs.get('review_retry')==retry
    review = data.get('review') or {}
    if review.get('candidate') != candidate or not review.get('approved'):
        return False
    if stage == 'verify':
        from .ci import verification_ready
        if not verification_ready(store,task,inputs.get('base')) or not store.integration_available(work):
            return False
        verified = data.get('verified') or {}
        return verified.get('candidate') != candidate or verified.get('passed') is not True
    if stage == 'publish':
        return data.get('verified') == inputs['verified'] and verified.get('passed') is True and store.integration_available(work)
    return False
