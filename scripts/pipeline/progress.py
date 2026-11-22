"""Explain delivery progress separately from scheduler and gateway liveness."""
import json
import time
from collections import Counter


def blockers(store, task, tasks, work):
    from .repairs import satisfied
    from .requirements import pending
    data=task['data'];name=task['name'];generation=task['generation'];reasons=[]
    if satisfied(data):return reasons
    def add(code,message):reasons.append({'code':code,'message':message})
    if store.setting('task_paused:'+name,False):add('paused','Operator paused this task; explicitly retry to resume')
    missing=[dep for dep in data.get('dependencies') or [] if dep not in tasks or not satisfied(tasks[dep]['data'])]
    if missing:add('dependencies','Waiting for dependencies: '+', '.join(missing))
    if (data.get('requirements') or {}).get('errors'):
        add('requirements_binding','Correct the task/spec bindings: '+'; '.join(data['requirements']['errors']))
    for stage in ('triage','assess','implement','review','verify','publish'):
        hold=store.setting(store.runtime_hold_key(name,stage,generation))
        if hold:add(hold.get('category','hold'),f"{stage}: {hold.get('message','Needs diagnosis')}")
    if (data.get('repair') or {}).get('shared_environment'):
        add('shared_environment','Waiting for shared CI repair: '+str(data['repair'].get('prerequisite')))
    resources=store.blocking_resources(task=name)
    if resources and not any(w['state']=='claimed' and w['stage'] not in ('observe','cleanup') for w in work):
        add('cleanup','Waiting for owned resource cleanup: '+', '.join(r['name'] for r in resources))
    for item in work:
        if item['state']=='attention':
            reason=json.loads(item['result'] or '{}')
            category=reason.get('category','attention')
            if not any(r['code']==category for r in reasons):add(category,reason.get('message','Inspect retained evidence and retry'))
    if not reasons and pending(data):add('assessment','Current requirements need independent assessment')
    return reasons


def delivery(store, tasks):
    now=time.time();unfinished=[t for t in tasks if not t['current_satisfied'] and t.get('state')!='retired']
    row=store.db.execute("""SELECT max(at) at FROM events WHERE kind='completed'
        AND json_extract(detail,'$.pending_head') IS NULL
        AND (json_extract(detail,'$.merged') IS NOT NULL OR json_extract(detail,'$.status')='merged')""").fetchone()
    last=row['at'] if row else None
    active=sum(bool(t['stage']) for t in unfinished)
    age=now-last if last else None
    stalled=bool(unfinished and ((age is not None and age>store.setting('delivery_stall_seconds',3600))
                                or (not active and all(t.get('blockers') for t in unfinished))))
    return {'condition':'Complete' if not unfinished else 'Stalled' if stalled else 'Progressing' if active else 'Waiting',
            'last_merge_at':last,'seconds_since_merge':age,'unfinished':len(unfinished),
            'active':active,'blockers':dict(Counter(r['code'] for t in unfinished for r in t.get('blockers',[])))}
