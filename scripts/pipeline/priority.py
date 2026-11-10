"""Non-preempting priority classes, with queue aging and protected control work."""
import json
import re
import time

DEFAULT_CLASSES = {'background':-100, 'normal':0, 'high':100, 'repair':200, 'urgent':300}
CONTROL = {'cleanup':1_000_000_300, 'observe':1_000_000_200,
           'publish':1_000_000_100, 'verify':1_000_000_000}
COMPUTE = {'triage','assess','implement','review','verify'}
REPAIRS = {'ci','verification','rebase','review','requirements_gap'}


class PriorityError(ValueError):
    pass


def classes(store):
    return DEFAULT_CLASSES | store.setting('priority_classes', {})


def define(store, name, value):
    if not re.fullmatch(r'[a-z][a-z0-9-]{0,62}',name) or type(value) is not int or not -1000 <= value <= 1000:
        raise PriorityError('priority class must have a lowercase name and integer value from -1000 to 1000')
    with store.transaction():
        definitions=store.setting('priority_classes', {})
        definitions[name]=value
        store.db.execute('INSERT OR REPLACE INTO settings VALUES(?,?)',('priority_classes',json.dumps(definitions)))


def validate(store, name):
    if name not in classes(store):
        raise PriorityError(f'Unknown priority class: {name}')
    return name


def set_task(store, name, value):
    store.task(name)
    if value is not None:
        validate(store,value)
    store.set_setting('priority_override:'+name,value)
    store.event('operator_priority',{'class':value,'preemptionPolicy':'Never'},name)


def task_priority(store, task):
    name=store.setting('priority_override:'+task['name'])
    source='operator'
    if name is None:
        labels=task['data'].get('priority_labels') or []
        names=[label.removeprefix('priority/') for label in labels if label.startswith('priority/')]
        if len(set(names)) > 1:
            raise PriorityError('Multiple priority labels; select exactly one priority/<class> label')
        name=next(iter(names),None)
        source='github'
    if name is None:
        data=task['data']
        finding=data.get('repair') or data.get('review_context') or {}
        unresolved=store.db.execute("""SELECT 1 FROM findings WHERE task=? AND generation=?
            AND resolved_by IS NULL AND category IN ('ci','verification','rebase','review') LIMIT 1""",
            (task['name'],task['generation'])).fetchone()
        name='repair' if finding.get('category') in REPAIRS or unresolved or data.get('assessment_pending') or data.get('ci_environment') else 'normal'
        source='automatic'
    validate(store,name)
    return {'class':name,'value':classes(store)[name],'source':source,'preemptionPolicy':'Never'}


def info(store, work, now=None):
    now=time.time() if now is None else now
    try:
        value=task_priority(store,store.task(work['task']))
    except PriorityError as exc:
        value={'class':'invalid','value':-1000,'source':'invalid','error':str(exc),'preemptionPolicy':'Never'}
    if work['stage'] in CONTROL:
        return value | {'stage_class':'system/'+work['stage'], 'effective':CONTROL[work['stage']], 'age_seconds':0}
    since=work.get('ready_since') or now
    age=max(0,now-max(since,work.get('retry_at',0))) if work['state'] in ('ready','retry') else 0
    return value | {'stage_class':value['class'], 'age_seconds':int(age),
                    'effective':min(999_999_999,value['value']+int(age//60))}


def sort_key(store, work, now=None):
    value=info(store,work,now)
    return (-value['effective'],work.get('ready_since') or work['created'],work['created'],work['id'])
