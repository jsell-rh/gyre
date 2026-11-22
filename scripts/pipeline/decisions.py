"""Durable three-option human decisions; selections create reviewed spec work."""
import getpass
import hashlib
import json
from pathlib import Path
import re
import time

from .catalog import contract, metadata
from .execution import Retry, Attention


def initialize(store):
    store.db.execute('''CREATE TABLE IF NOT EXISTS decisions(
        id TEXT PRIMARY KEY,task TEXT NOT NULL,generation TEXT NOT NULL,
        requirements TEXT NOT NULL,context_files TEXT NOT NULL,body TEXT NOT NULL,
        selection TEXT,application_task TEXT,created REAL NOT NULL,updated REAL NOT NULL)''')


def request(store, task):
    stage='spec_audit' if task['data'].get('spec_planner') else 'assess'
    hold=store.setting(store.runtime_hold_key(task['name'],stage,task['generation'])) or {}
    if hold.get('category')!='requirements_triage':return None
    desired=contract.requirement_parts(task['body'])
    text=json.dumps([task['data'].get('spec_ref'),hold,task['data'].get('assessment'),desired])
    root=store.directory/'source';files={}
    for name in sorted(set(re.findall(r'[\w-]+\.md',text))):
        for folder in ('specs/system','specs/development','docs'):
            path=root/folder/name
            if path.is_file():
                files[str(path.relative_to(root))]=hashlib.sha256(path.read_bytes()).hexdigest()
    fp=(task['data'].get('requirements') or {}).get('fingerprint',task['generation'])
    payload={'task':task['name'],'generation':task['generation'],'requirements':fp,'context_files':files,'conflict':hashlib.sha256(hold.get('message','').encode()).hexdigest()}
    return payload|{'id':hashlib.sha256(json.dumps(payload,sort_keys=True).encode()).hexdigest()[:24],
                    'reason':hold.get('message','Task/spec conflict')}


def validate(report):
    if not isinstance(report,dict):raise ValueError('Missing three-option decision report')
    for key,limit in (('question',800),('context',6000)):
        if not isinstance(report.get(key),str) or not report[key].strip() or len(report[key])>limit:
            raise ValueError('Decision needs bounded, nonempty '+key)
    options=report.get('options')
    if not isinstance(options,list) or len(options)!=3:raise ValueError('Decision must offer exactly three options')
    ids=[]
    for option in options:
        if not isinstance(option,dict) or not re.fullmatch(r'[a-z][a-z0-9_-]{0,39}',str(option.get('id',''))):
            raise ValueError('Decision option needs a stable id')
        ids.append(option['id'])
        for key,limit in (('title',120),('summary',1400),('tradeoffs',1400),('next_step',1400)):
            if not isinstance(option.get(key),str) or not option[key].strip() or len(option[key])>limit:
                raise ValueError('Every option needs bounded '+key)
    if len(set(ids))!=3 or report.get('recommendation') not in ids:
        raise ValueError('Decision needs three distinct options and one recommendation')
    sources=report.get('sources',[])
    if not isinstance(sources,list) or len(sources)>12 or any(not isinstance(s,str) or len(s)>800 or
        not (s.startswith(('https://','specs/','docs/'))) for s in sources):raise ValueError('Invalid decision sources')
    return {k:report[k] for k in ('question','context','options','recommendation')}|{'sources':sources}


def create(store, task, expected, report, producer='triage'):
    initialize(store);report=validate(report)
    with store.transaction():
        current=store.task(task['name'])
        if request(store,current)!=expected:raise ValueError('Decision inputs changed; regenerate the options')
        now=time.time()
        inserted=store.db.execute('INSERT OR IGNORE INTO decisions VALUES(?,?,?,?,?,?,NULL,NULL,?,?)',
            (expected['id'],task['name'],task['generation'],expected['requirements'],json.dumps(expected['context_files']),json.dumps(report),now,now)).rowcount
        if inserted:store.event('decision_requested',{'id':expected['id'],'producer':producer,'question':report['question']},task['name'])
    return expected['id']


def list_decisions(store):
    initialize(store);result=[]
    for row in store.db.execute('SELECT * FROM decisions ORDER BY created DESC'):
        task=store.task(row['task']);expected=request(store,task)
        selection=json.loads(row['selection']) if row['selection'] else None
        current=bool(expected and expected['id']==row['id'])
        if not current and not selection:continue
        if selection and len(result)>=50:continue
        result.append(json.loads(row['body'])|{'id':row['id'],'task':row['task'],'generation':row['generation'],
            'requirements':row['requirements'],'selection':selection,'application_task':row['application_task'],
            'current':current,'status':'selected' if selection else 'pending','created':row['created']})
    return result[:50]


def exists(store, identity):
    initialize(store)
    return store.db.execute('SELECT 1 FROM decisions WHERE id=?',(identity,)).fetchone() is not None


def choose(store, identity, option_id, actor=None, source='local cockpit', instruction=None):
    """Record intent and enqueue its amendment atomically; never unpause old code."""
    from .requirements import snapshot
    initialize(store)
    with store.transaction():
        row=store.db.execute('SELECT * FROM decisions WHERE id=?',(identity,)).fetchone()
        if not row:raise ValueError('Unknown decision')
        if row['selection']:
            if json.loads(row['selection'])['option']!=option_id:raise ValueError('A different choice is already recorded; request a new decision to change direction')
            return {'id':identity,'application_task':row['application_task'],'recorded':True}
        parent=store.task(row['task']);expected=request(store,parent)
        if not expected or expected['id']!=identity:raise ValueError('The task/spec context changed; refresh and review the new options')
        report=json.loads(row['body']);option=next((o for o in report['options'] if o['id']==option_id),None)
        if option is None:raise ValueError('Unknown decision option')
        selection={'option':option_id,'actor':actor or getpass.getuser(),'source':source,'instruction':instruction,'at':time.time()}
        from .spec_backlog import allocate
        name=allocate(store,1,'operator decision '+identity)[0]
        title=f"Apply operator decision for {parent['name']}: {option['title']}"
        from .spec_backlog import GOAL_REF
        spec_ref=GOAL_REF if parent['data'].get('spec_planner') else parent['data']['spec_ref']
        body='---\ntitle: '+json.dumps(title)+'\nspec_ref: '+json.dumps(spec_ref,ensure_ascii=False)+'\ndepends_on: []\nprogress: not-started\n---\n\n## Approved operator decision\n\n'
        body+=json.dumps({'id':identity,'parent':parent['name'],'question':report['question'],'context':report['context'],
            'options':report['options'],'recommendation':report['recommendation'],'selection':selection,'sources':report.get('sources',[])},indent=2)+'\n\n'
        body+=f"Only option `{option_id}` is authorized. The other alternatives are recorded history, not requirements.\n\n## Selected direction\n\n{option['title']}\n\n{option['summary']}\n\n{option['next_step']}\n\n"
        body+='''## Required behavior

Prepare the task/spec amendment authorized by the recorded choice. This is a
requirements correction, not permission to guess at product behavior. Amend the
canonical spec, the parent task's requirements/spec binding, and documentation
so they express the selected direction consistently. Preserve historical intent
in clearly labelled history; never claim the newly selected behavior is already
implemented. Update specs/task-spec-inputs.json if a spec binding changes. Preserve registry
bindings still referenced by historical tasks, including this amendment task.

Do not change product code, tests, scripts, CI, exemptions or verification gates.
The amendment must pass independent review, remote verification and GitHub checks.
Once it merges, the controller will reassess the parent task against the new
requirements and implement any remaining gaps. Keep changes within specs/ and
docs/ (plus this task's own bookkeeping). Do not modify unrelated task contracts.

## Acceptance Criteria

- [ ] Canonical spec and parent task agree with the recorded choice.
- [ ] The parent task is no longer instructed to implement contradictory behavior.
- [ ] The choice, rationale and authority remain recorded in this task document.
- [ ] Existing implementation status is reported honestly; no fabricated coverage.
- [ ] No production code, tests or enforcement have been changed in this amendment.
'''
        if parent['data'].get('spec_planner'):
            body=body.replace("the parent task's requirements/spec binding","related real task bindings (the spec audit is a controller record, not a task file to create)")
        scoped=snapshot(store.directory/'source',body,contract,require_registry=True)
        if scoped.get('errors'):raise ValueError('Cannot queue amendment until its spec binding is corrected: '+'; '.join(scoped['errors']))
        generation=scoped['fingerprint']
        data=metadata(body)|{'requirements':scoped,'catalog_generation':generation,'catalog_source':store.setting('observed_main'),
            'source_decision':identity,'decision_parent':parent['name'],'triaged':None,
            'decision_allowed_paths':list(expected['context_files'])+[f"specs/tasks/{parent['name']}.md",f'specs/tasks/{name}.md','specs/task-spec-inputs.json']}
        store.db.execute('INSERT INTO tasks VALUES(?,?,?,?,?)',(name,generation,body,json.dumps(data),time.time()))
        store.db.execute('UPDATE decisions SET selection=?,application_task=?,updated=? WHERE id=?',
            (json.dumps(selection),name,time.time(),identity))
        store.event('decision_selected',{'id':identity,'selection':selection,'application_task':name},parent['name'])
    return {'id':identity,'application_task':name,'recorded':True}


def prepare(execution, task):
    expected=execution.claim['input']['decision_request']
    if request(execution.store,task)!=expected:raise ValueError('Decision context changed')
    if exists(execution.store,expected['id']):return {'decision':expected['id']},{},[]
    from .stages import prompt
    result=execution.cloud_step(task,prompt(execution,task)+'\nOperator guidance and verified external context:\n'+json.dumps(execution.store.setting('decision_guidance:'+task['name'],{})))
    def fail(message):
        key='decision_report_retry:'+expected['id'];count=execution.store.setting(key,0)+1
        execution.store.set_setting(key,count)
        error=Retry(message,fresh_model=True) if count<2 else Attention(message+'; repeated decision-report failure needs diagnosis')
        error.category='decision_report';error.fresh_model=True
        raise error
    if (not result.get('valid') or result.get('source_valid') is False or result.get('agent_exit')!=0
            or contract.requirement_parts(result.get('task_body',''))!=contract.requirement_parts(task['body'])):
        fail('Decision preparation must remain read-only and preserve the task contract')
    try:identity=create(execution.store,task,expected,result.get('decision'))
    except ValueError as exc:fail(str(exc))
    return {'decision':identity,'waiting_for_operator':True},{},[]
