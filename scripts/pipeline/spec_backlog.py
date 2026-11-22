"""Spec inventory, read-only fidelity audits, reviewed plans and durable task leases.

Coverage is a scheduling hint, never evidence. Planning records are not deliveries.
Only a merged backlog publication releases generated tasks to implementation.
"""
import fcntl
import json
import os
from pathlib import Path
import re
import time

from .requirements import digest, sections, snapshot
from .catalog import contract, metadata, helper
from .execution import Attention, Retry

STAGES = ('spec_audit', 'decompose', 'plan_review', 'backlog')
MODEL_STAGES = STAGES[:3]
EXCLUDED = {'specs/system/trusted-foundry-integration.md'}
GOAL_REF = 'GOAL.md — real implementations and meaningful verification'
EVIDENCE = helper('pipeline-evidence')


def initialize(store):
    store.db.executescript('''
        CREATE TABLE IF NOT EXISTS task_allocations(name TEXT PRIMARY KEY, purpose TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS backlog_publications(
            planner TEXT NOT NULL, generation TEXT NOT NULL, plan TEXT NOT NULL,
            names TEXT NOT NULL, head TEXT, PRIMARY KEY(planner,generation,plan));
    ''')


def allocate(store, count, purpose):
    """Caller holds the ledger transaction; share IDs with decisions and CI repair."""
    names=[r[0] for r in store.db.execute('SELECT name FROM tasks UNION SELECT name FROM task_allocations')]
    number=max([int(n[5:]) for n in names if re.fullmatch(r'task-\d+',n)]+[0])+1
    result=[f'task-{n:03d}' for n in range(number,number+count)]
    store.db.executemany('INSERT INTO task_allocations VALUES(?,?)',[(n,purpose) for n in result])
    return result


def inventory(store, root, revision):
    initialize(store)
    root=Path(root)
    goal=(root/'specs/GOAL.md').read_text()
    present=set()
    for folder in ('system','development'):
        for path in sorted((root/'specs'/folder).glob('*.md')):
            relative=path.relative_to(root).as_posix()
            if relative in EXCLUDED:continue
            if path.is_symlink() or not path.resolve().is_relative_to(root.resolve()):
                raise ValueError('Canonical spec escapes its source checkout: '+relative)
            text=path.read_text()
            # Exact breadcrumbs avoid retargeting renamed or duplicate headings.
            headings=[n['path'] for n in sections(text) if n['path']]
            if len({tuple(h) for h in headings})!=len(headings):
                raise ValueError('Ambiguous canonical headings: '+relative)
            requirements={digest([relative,h])[:24]:h for h in headings} or {digest([relative,[]])[:24]:[]}
            content=digest([relative,text,goal,requirements])
            name='spec-'+digest(relative)[:24];present.add(name)
            old=store.db.execute('SELECT 1 FROM tasks WHERE name=?',(name,)).fetchone()
            previous=store.task(name) if old else None
            if previous and previous['data'].get('spec_content')==content and not previous['data'].get('spec_retired'):
                resume_decision(store,previous)
                continue
            generation=digest([content,previous['generation']]) if previous else content
            body=('---\ntitle: '+json.dumps('Reconcile '+relative)+'\nspec_ref: '+json.dumps(relative)+
                  '\ndepends_on: []\nprogress: not-started\n---\n\n'
                  'Inspect the entire canonical specification against production code. Never alter its contract.\n')
            data=metadata(body)|{'spec_planner':True,'spec_content':content,'document':relative,'sections':requirements,
                'spec_text':text,'goal':goal,'catalog_source':revision,'status':'pending',
                'requirements':{'version':2,'fingerprint':generation,'errors':[],
                                'components':{'canonical_spec':text,'goal':goal}}}
            store.put_task(name,generation,body,data,expected=previous)
    for task in store.tasks():
        if task['data'].get('spec_planner') and task['name'] not in present and not task['data'].get('spec_retired'):
            store.put_task(task['name'],digest([task['generation'],'retired']),task['body'],
                           task['data']|{'spec_retired':True,'status':'retired'},expected=task)


def enabled(store):
    return store.setting('spec_reconciliation_enabled',False)


def resume_decision(store, task):
    """A reviewed human amendment can resolve a conflict in another referenced spec."""
    key=store.runtime_hold_key(task['name'],'spec_audit',task['generation'])
    if (store.setting(key) or {}).get('category')!='requirements_triage':return
    if not store.db.execute("SELECT 1 FROM sqlite_master WHERE type='table' AND name='decisions'").fetchone():return
    from .repairs import satisfied
    for row in store.db.execute('SELECT id,application_task FROM decisions WHERE task=? AND generation=? AND selection IS NOT NULL',
                                (task['name'],task['generation'])):
        applied=store.db.execute('SELECT data FROM tasks WHERE name=?',(row['application_task'],)).fetchone()
        if not applied or not satisfied(json.loads(applied['data'])):continue
        with store.transaction():
            data=store.task(task['name'])['data']
            data.update(status='pending',spec_audit=None,decomposition=None,plan_review=None,
                        audit_cycle=data.get('audit_cycle',0)+1)
            store.db.execute('UPDATE tasks SET data=? WHERE name=?',(json.dumps(data),task['name']))
            store.db.execute('DELETE FROM settings WHERE key=?',(key,))
            store.db.execute("UPDATE work SET state='obsolete',token=token+1 WHERE task=? AND generation=? AND stage='spec_audit' AND state='attention'",
                             (task['name'],task['generation']))
            store.event('spec_decision_applied',{'decision':row['id'],'amendment':row['application_task']},task['name'])
        return


def current_evidence(root, evidence):
    for record in evidence:
        path=Path(root)/record['file']
        if not path.is_file() or path.is_symlink() or EVIDENCE.file_digest(path)!=record['sha256']:
            return False
    return True


def audit_input(store, task, base, cycle):
    # Main moving is not a new attempt. Finish the exact-source observation;
    # evidence freshness is checked before it can create implementation work.
    row=store.db.execute("""SELECT input FROM work WHERE task=? AND generation=?
        AND stage='spec_audit' AND state IN ('claimed','ready','retry','attention')
        AND json_extract(input,'$.cycle')=?
        ORDER BY (state='claimed') DESC,created LIMIT 1""",(task['name'],task['generation'],cycle)).fetchone()
    return json.loads(row['input']) if row else {'base':base,'cycle':cycle}


def revision_draft(data):
    """One bounded prior revision, never an approval or an accumulating ledger."""
    audit=data.get('spec_audit')
    if audit:audit={k:audit[k] for k in ('base','requirements','sections')}
    draft={'audit':audit,'plan':data.get('decomposition')}
    return draft if len(json.dumps(draft))<=131072 else None


def reconcile_rejected_publication(store, task):
    """A semantic backlog rejection is planning feedback, never a code repair."""
    data=task['data'];publication=data.get('backlog_publication')
    if data.get('status')!='assigned' or not publication:return False
    with store.transaction():
        parent=store.task(task['name']);current=parent['data']
        row=store.db.execute('SELECT data FROM tasks WHERE name=?',(publication,)).fetchone()
        child=json.loads(row['data']) if row else {}
        rejection=child.get('backlog_rejected')
        if (parent['generation']!=task['generation'] or current.get('status')!='assigned'
                or current.get('backlog_publication')!=publication or not rejection
                or child.get('backlog_plan')!=digest(current.get('decomposition'))
                or child.get('backlog_manifest',{}).get('requirements')!=parent['generation']):return False
        plan=child['backlog_plan']
        prefix='spec_gap_owner:'+task['name']+':'+task['generation']+':'
        # Only the rejected plan relinquishes ownership. Reserved task IDs and
        # immutable private Git receipts remain historical and are never reused.
        store.db.execute('DELETE FROM settings WHERE substr(key,1,?)=? AND value=?',
                         (len(prefix),prefix,json.dumps(plan)))
        current.update(revision_draft=revision_draft(current),status='pending',spec_audit=None,decomposition=None,plan_review=None,
            backlog_publication=None,backlog_tasks=[],audit_cycle=current.get('audit_cycle',0)+1,
            plan_revision=current.get('plan_revision',0)+1,
            publication_rejections=current.get('publication_rejections',0)+1,
            rejected_plans=[*current.get('rejected_plans',[]),plan],
            plan_feedback=[{'publication':publication,'candidate':rejection['source'],
                            'artifact':rejection.get('artifact'),'findings':rejection['details']}])
        store.db.execute('UPDATE tasks SET data=?,updated=? WHERE name=?',
                         (json.dumps(current),time.time(),task['name']))
        store.event('spec_backlog_rejected',{'publication':publication,'plan':plan,
                    'feedback':rejection,'cycle':current['audit_cycle']},task['name'])
    return True


def discover(store, task, stage, base):
    if not enabled(store) or task['data'].get('spec_retired'):return []
    if reconcile_rejected_publication(store,task):return []
    selected=store.setting('spec_reconciliation_documents')
    if selected and task['data']['document'] not in selected:return []
    from .decisions import request, exists
    decision=request(store,task)
    if decision:
        if stage=='triage' and not exists(store,decision['id']):
            return [store.enqueue('triage',task['name'],task['generation'],{'base':base,'decision_request':decision},priority=25000)]
        return []
    data=task['data'];audit=data.get('spec_audit');plan=data.get('decomposition');review=data.get('plan_review')
    status=data.get('status','pending')
    if status in ('observed','assigned'):
        linked=data.get('backlog_tasks',[])
        from .repairs import satisfied
        all_done=bool(linked) and all((r:=store.db.execute('SELECT data FROM tasks WHERE name=?',(n,)).fetchone())
                                    and satisfied(json.loads(r['data'])) for n in linked)
        due=(time.time()-data.get('audited_at',0)>store.setting('spec_audit_interval',86400)
             or not current_evidence(store.directory/'source',(audit or {}).get('source_evidence',[])))
        if not all_done and (status=='assigned' or not due):return []
        # A new audit cycle never recycles a succeeded work identity.
        if stage!='spec_audit':return []
        return [store.enqueue(stage,task['name'],task['generation'],
                audit_input(store,task,base,data.get('audit_cycle',0)+1),priority=-100)]
    inputs=None
    if stage=='spec_audit' and not audit:inputs=audit_input(store,task,base,data.get('audit_cycle',0))
    if stage=='decompose' and audit and audit['gaps'] and not plan:
        inputs={'base':audit['base'],'audit':digest(audit),'revision':data.get('plan_revision',0)}
    if stage=='plan_review' and plan and not review:inputs={'base':audit['base'],'plan':digest(plan),'audit':digest(audit)}
    if stage=='backlog' and audit and audit['gaps'] and review and review['approved'] and status!='assigned':
        inputs={'base':audit['base'],'plan':digest(plan)}
    return [store.enqueue(stage,task['name'],task['generation'],inputs,priority=-100)] if inputs else []


def eligible(store, work, task, inputs):
    if not enabled(store) or task['data'].get('spec_retired'):return False
    selected=store.setting('spec_reconciliation_documents')
    if selected and task['data']['document'] not in selected:return False
    stage=work['stage'];data=task['data']
    if stage not in STAGES:return False
    # One small planning pool cannot consume the implementation/verifier budget.
    if stage in MODEL_STAGES and work.get('state')!='claimed':
        active=store.db.execute("SELECT count(*) FROM work WHERE state='claimed' AND stage IN ('spec_audit','decompose','plan_review')").fetchone()[0]
        if active>=store.setting('spec_planning_concurrency',2):return False
    audit=data.get('spec_audit');plan=data.get('decomposition')
    if stage=='spec_audit':
        return inputs['cycle']==data.get('audit_cycle',0)+(1 if data.get('status') in ('observed','assigned') else 0)
    if stage=='decompose':return bool(audit and digest(audit)==inputs['audit'] and not plan
                                     and inputs['revision']==data.get('plan_revision',0))
    if stage=='plan_review':return bool(plan and digest(plan)==inputs['plan'] and not data.get('plan_review'))
    return bool(plan and digest(plan)==inputs['plan'] and (data.get('plan_review') or {}).get('approved')
                and data.get('status')!='assigned')


def bounded(value, key, limit=12000):
    result=value.get(key)
    if not isinstance(result,str) or not result.strip() or len(result)>limit:raise ValueError('Missing/bad '+key)
    return result


def evidence(root, record):
    return EVIDENCE.evidence(root,record)


def validate_audit(report, task, base, root):
    if not isinstance(report,dict) or report.get('base')!=base or report.get('requirements')!=task['generation']:
        raise ValueError('Audit identity differs from exact assigned source/spec')
    rows=report.get('sections');required=task['data']['sections']
    if not isinstance(rows,list) or len(rows)!=len(required) or {r.get('id') for r in rows if isinstance(r,dict)}!=set(required):
        raise ValueError('Audit must account for every canonical heading exactly once')
    records=[];gaps=[]
    for row in rows:
        if not isinstance(row,dict):raise ValueError('Invalid audit section')
        decision=row.get('decision')
        if decision not in ('satisfied','gap','context','conflict'):raise ValueError('Invalid section disposition')
        bounded(row,'detail');quote=bounded(row,'requirement_quote',3000)
        nodes=sections(task['data']['spec_text'])
        matches=[n for n in nodes if n['path']==required[row['id']]]
        if len(matches)!=1 or quote not in task['data']['spec_text'][matches[0]['start']:matches[0]['end']]:
            raise ValueError('Requirement quote does not belong to assigned section')
        items=row.get('evidence')
        if not isinstance(items,list) or (not items and decision!='context') or any(not isinstance(r,dict) for r in items):
            raise ValueError('Every disposition requires concrete evidence objects')
        if decision in ('gap','satisfied') and not any(isinstance(r,dict) and EVIDENCE.production_source(str(r.get('file',''))) for r in items):
            raise ValueError('Behavior claims require inspected production source, not spec/task prose alone')
        records.extend(evidence(root,r) for r in items)
        if decision=='conflict':
            error=Attention('Canonical specification conflict in '+task['data']['document']+': '+row['detail'])
            error.category='requirements_triage';raise error
        if decision=='gap':gaps.append(row)
    return report|{'gaps':gaps,'source_evidence':list({r['file']:r for r in records}.values())}


def validate_plan(report, audit, store):
    if not isinstance(report,dict) or report.get('audit')!=digest(audit):raise ValueError('Plan targets another audit')
    tasks=report.get('tasks');gaps={r['id'] for r in audit['gaps']}
    if not isinstance(tasks,list) or not 1<=len(tasks)<=8:raise ValueError('Plan requires 1–8 bounded tasks')
    if any(not isinstance(t,dict) for t in tasks):raise ValueError('Invalid plan task')
    keys=[bounded(t,'key',60) for t in tasks]
    if len(set(keys))!=len(keys) or any(not re.fullmatch('[a-z][a-z0-9-]*',k) for k in keys):raise ValueError('Invalid plan task keys')
    covered=[];graph={}
    by_key={t['key']:t for t in tasks}
    for task in tasks:
        owned=task.get('covers')
        if not isinstance(owned,list) or not owned or any(not isinstance(i,str) or i not in gaps for i in owned):
            raise ValueError('Task coverage must name known gap IDs')
        covered.extend(owned);bounded(task,'title',200)
        bounded(task,'required_behavior');bounded(task,'negative_probe',3000)
        if any(re.search(r'^## (?:Shipped|Review|Implementation Notes|Implementation Log)\s*$',task[k],re.M) for k in ('required_behavior','negative_probe')):
            raise ValueError('Requirements cannot be hidden in operational sections')
        acceptance=task.get('acceptance')
        if not isinstance(acceptance,list) or not acceptance or any(not isinstance(a,str) or not a.strip() or len(a)>2000 or '\n' in a for a in acceptance):
            raise ValueError('Concrete acceptance criteria required')
        deps=task.get('dependencies')
        if not isinstance(deps,list) or any(not isinstance(d,str) or (d not in keys and not store.db.execute('SELECT 1 FROM tasks WHERE name=?',(d,)).fetchone()) for d in deps):
            raise ValueError('Plan has unknown dependencies')
        graph[task['key']]=[d for d in deps if d in keys]
        existing=task.get('existing_task')
        if existing is not None and (not isinstance(existing,str) or not re.fullmatch(r'task-\d+',existing)):
            raise ValueError('Invalid existing task identity')
        if existing:
            from .repairs import satisfied
            row=store.db.execute('SELECT data FROM tasks WHERE name=?',(existing,)).fetchone()
            if not row or satisfied(json.loads(row['data'])):raise ValueError('Reuse requires an unfinished existing task')
            current=json.loads(row['data'])
            if current.get('backlog_plan') or current.get('backlog_rejected'):
                raise ValueError('Backlog publication is not a product implementation task')
            for dependency in deps:
                target=by_key[dependency].get('existing_task') if dependency in by_key else dependency
                if not target or target not in (current.get('dependencies') or []):
                    raise ValueError('Reusing a task cannot silently add dependencies; plan a separately reviewed dependency amendment')
    if set(covered)!=gaps or len(covered)!=len(set(covered)):raise ValueError('Each gap must have exactly one owner in the plan')
    if contract.graph_errors(graph):raise ValueError('Cyclic decomposition dependencies')
    return report


def evidence_identity(audit):
    return digest(sorted((r['file'],r['sha256']) for r in audit.get('source_evidence',[])))


def rejected_contract(plan, audit):
    return digest([plan['tasks'],evidence_identity(audit)])


def review_revision(data, report):
    """Bound unresolved issues, not legitimate successive improvements."""
    findings=report['findings'];target=report.get('revision_target','audit')
    if target not in ('audit','plan'):raise ValueError('Unknown review revision target')
    if target=='plan' and (report.get('audit_approved') is not True or not data['spec_audit']['gaps']):
        raise ValueError('Plan-only revision requires explicit confirmation that the existing nonempty gap audit remains sound')
    source=evidence_identity(data['spec_audit']);previous=data.get('review_budget',{})
    counts=dict(previous.get('issues',{})) if previous.get('evidence')==source else {}
    keys=set()
    for finding in findings:
        identity=finding.get('id') if isinstance(finding,dict) else None
        if identity is not None:
            if (not isinstance(identity,str) or not re.fullmatch(r'[0-9a-f]{24}:[a-z0-9-]{1,64}',identity)
                    or identity.split(':')[0] not in data['sections']):
                raise ValueError('Review finding ID must name a known heading and a stable issue slug')
        keys.add(identity or digest(finding))
    for key in keys:counts[key]=counts.get(key,0)+1
    attempts=previous.get('revisions',data.get('plan_revision',0))+1
    if any(counts[k]>=3 for k in keys) or attempts>12:
        error=Attention('Spec planning stalled: repeated unresolved findings or twelve revisions without approval: '+json.dumps(findings)[:5000])
        error.category='spec_plan';raise error
    result={'revision_draft':revision_draft(data),'decomposition':None,'plan_review':None,
        'plan_revision':data.get('plan_revision',0)+1,'plan_feedback':findings,
        'review_budget':{'revisions':attempts,'evidence':source,'issues':counts},
        'rejected_plans':[*data.get('rejected_plans',[])[-23:],digest(data['decomposition'])],
        'rejected_task_contracts':[*data.get('rejected_task_contracts',[])[-23:],rejected_contract(data['decomposition'],data['spec_audit'])]}
    if target=='plan':result['status']='audited'
    else:result.update(spec_audit=None,status='pending',audit_cycle=data.get('audit_cycle',0)+1)
    return result


def run_model(execution, task):
    from .stages import prompt, checkout
    stage=execution.claim['stage'];data=task['data'];base=execution.claim['input']['base']
    result=execution.cloud_step(task,prompt(execution,task))
    if (not result.get('valid') or not result.get('source_valid') or result.get('agent_exit') or
        contract.requirement_parts(result.get('task_body',''))!=contract.requirement_parts(task['body'])):
        raise Attention('Spec planning changed source or failed its assignment; no plan can be accepted')
    report=result.get('planning');path=checkout(execution,base)
    try:
        if stage=='spec_audit':
            audit=validate_audit(report,task,base,path)
            updates={'spec_audit':audit,'decomposition':None,'plan_review':None,'status':'audited',
                     'audited_at':time.time(),'audit_cycle':execution.claim['input']['cycle']}
            if not audit['gaps']:updates['decomposition']={'audit':digest(audit),'tasks':[]}
        elif stage=='decompose':
            if digest(report) in data.get('rejected_plans',[]):
                raise ValueError('This exact plan was rejected at backlog review; address the retained findings before resubmitting')
            if isinstance(report,dict) and isinstance(report.get('tasks'),list) and rejected_contract(report,data['spec_audit']) in data.get('rejected_task_contracts',[]):
                raise ValueError('Unchanged rejected task contracts cannot be resubmitted against unchanged source evidence')
            updates={'decomposition':validate_plan(report,data['spec_audit'],execution.store),'status':'planned'}
        else:
            if (not isinstance(report,dict) or report.get('plan')!=digest(data['decomposition']) or
                    type(report.get('approved')) is not bool or not isinstance(report.get('findings'),list)):
                raise ValueError('Independent plan review lacks exact plan identity/verdict')
            checks=report.get('checks',[])
            if (len(checks)!=len(data['sections']) or {c.get('id') for c in checks if isinstance(c,dict)}!=set(data['sections'])
                    or any(not isinstance(c,dict) or not isinstance(c.get('detail'),str) or not c['detail'].strip() for c in checks)):
                raise ValueError('Independent reviewer must challenge every audited section, including no-gap/context claims')
            if report['approved'] and report['findings']:raise ValueError('Approved plan has unresolved findings')
            if not report['approved']:
                if not report['findings']:raise ValueError('Rejection requires actionable findings')
                updates=review_revision(data,report)
            else:
                updates={'revision_draft':None,'plan_review':report,'review_budget':{'revisions':0,'issues':{}},
                         'rejected_task_contracts':[],'status':'reviewed' if data['spec_audit']['gaps'] else 'observed'}
    except (ValueError,TypeError,KeyError) as exc:
        key=f"spec_report_failures:{task['name']}:{task['generation']}:{stage}:"+digest(execution.claim['input'])
        feedback={'category':'spec_report','message':str(exc),'input':execution.claim['input']}
        with execution.store.transaction():
            execution.current()
            count=execution.store.setting(key,0)+1
            execution.store.db.executemany('INSERT INTO settings VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value',[
                (key,json.dumps(count)),
                ('spec_report_rejected:'+execution.claim['id']+':'+digest(report),json.dumps(feedback)),
                ('spec_report_feedback:'+execution.claim['id'],json.dumps(feedback))])
        error=Retry(str(exc),fresh_model=True) if count<3 else Attention(str(exc))
        error.category='spec_report';raise error from exc
    # Never let the normal task-body importer treat a planning receipt as delivery.
    result={k:v for k,v in result.items() if k!='task_body'}
    return result,updates,[]


def previous_report(execution):
    """One input-bound draft, not an ever-growing failed-attempt prompt ledger."""
    previous=execution.claim.get('result') or {}
    if isinstance(previous,str):previous=json.loads(previous)
    if previous.get('category')!='spec_report':
        previous=execution.store.setting('spec_report_feedback:'+execution.claim['id'],{})
        if previous.get('input')!=execution.claim['input']:return None
    if previous.get('category')!='spec_report':return None
    for path in sorted(execution.directory.parent.glob('*/outcome.json'),key=lambda p:int(p.parent.name),reverse=True):
        if path.parent==execution.directory:continue
        try:
            value=json.loads(path.read_text());job=json.loads((path.parent/'bundle/job.json').read_text())
            report=value.get('planning')
            if (job['stage']!=execution.claim['stage'] or job['task']!=execution.claim['task']
                    or job['input']!=execution.claim['input'] or not value.get('valid')
                    or not value.get('source_valid') or value.get('agent_exit')!=0
                    or not isinstance(report,dict) or len(json.dumps(report))>131072):continue
            return {'error':previous.get('message'),'draft':report}
        except (OSError,ValueError,KeyError):continue
    return None


def other_plans_context(store, task, documents=None):
    """A small index; complete immutable plan snapshots are staged on demand."""
    from .repairs import satisfied
    tasks=store.tasks();by_name={t['name']:t for t in tasks};result=[]
    for other in tasks:
        data=other['data'];plan=data.get('decomposition')
        if (other['name']==task['name'] or not data.get('spec_planner') or data.get('spec_retired')
                or not plan or not plan.get('tasks')):continue
        linked=data.get('backlog_tasks',[])
        if data.get('status')=='assigned' and linked and all(n in by_name and satisfied(by_name[n]['data']) for n in linked):continue
        identity=digest(plan);filename=digest([other['name'],identity])[:32]+'.json'
        files=sorted({e['file'] for e in (data.get('spec_audit') or {}).get('source_evidence',[])})
        record={'planner':other['name'],'document':data['document'],'audit_at':data.get('audited_at'),
                'status':data.get('status'),'plan_id':identity,'publication':data.get('backlog_publication')}
        result.append(record|{'path':'/tmp/stage/other-plans/'+filename,'source_files':files[:5],
                              'source_file_count':len(files),'task_count':len(plan['tasks'])})
        if documents is not None:documents[filename]=record|{'plan':plan,'source_files':files,'backlog_tasks':linked}
    return result


def planning_context(task, store=None, documents=None):
    data=task['data']
    draft=data.get('revision_draft') if not data.get('decomposition') else None
    if draft and data.get('spec_audit'):draft={'plan':draft.get('plan')}
    return {'document':data['document'],'requirements':task['generation'],'sections':data['sections'],
            'audit':data.get('spec_audit'),'plan':data.get('decomposition'),'review_feedback':data.get('plan_feedback'),
            'revision_draft':draft,
            'catalog':catalog_context(store,data['document']) if store else [],
            'audit_at':data.get('audited_at'),
            'other_plans':other_plans_context(store,task,documents) if store else []}


def catalog_context(store, document):
    from .repairs import satisfied
    result=[]
    registry_path=store.directory/'source/specs/task-spec-inputs.json'
    registry=json.loads(registry_path.read_text()) if registry_path.is_file() else {}
    for task in store.tasks():
        data=task['data']
        if data.get('spec_planner') or data.get('backlog_plan') or data.get('backlog_rejected'):continue
        inputs=(data.get('requirements') or {}).get('inputs') or registry.get('bindings',{}).get(data.get('spec_ref'),[])
        related=any(registry.get('sections',{}).get(i,{}).get('file')==document for i in inputs)
        if satisfied(data) and not related and Path(document).name not in (data.get('spec_ref') or ''):continue
        result.append({'task':task['name'],'title':data.get('title'),'spec_ref':data.get('spec_ref'),
                       'dependencies':data.get('dependencies'),'satisfied':satisfied(data)})
    return result


def summary(store):
    rows=[t for t in store.tasks() if t['data'].get('spec_planner') and not t['data'].get('spec_retired')]
    items=[]
    from .priority import task_priority, PriorityError
    for task in rows:
        data=task['data']
        work=store.db.execute("SELECT * FROM work WHERE task=? AND generation=? AND state IN ('claimed','attention','retry','ready') ORDER BY (state='claimed') DESC,(state='attention') DESC,updated DESC LIMIT 1",(task['name'],task['generation'])).fetchone()
        state=('running' if work['state']=='claimed' else 'failed' if work['state']=='attention' else 'deferred' if work['state']=='retry' else 'ready') if work else data['status']
        result=json.loads(work['result'] or '{}') if work else {}
        try:priority=task_priority(store,task)
        except PriorityError as exc:priority={'class':'invalid','source':'invalid','error':str(exc)}
        items.append({'name':task['name'],'title':data['title'],'document':data['document'],
                      'state':state,'stage':work['stage'] if work else None,'progress':data['status'],'spec_ref':data['document'],
                      'condition':result.get('message'),'generation':task['generation'],'deps':[],'priority':priority,
                      'backlog_publication':data.get('backlog_publication'),'backlog_tasks':data.get('backlog_tasks',[]),
                      'audited_at':data.get('audited_at'),'gaps':len((data.get('spec_audit') or {}).get('gaps',[]))})
    return {'enabled':enabled(store),'documents':len(rows),'items':items,
        'states':{state:sum(t['data'].get('status')==state for t in rows)
        for state in ('pending','audited','planned','reviewed','observed','assigned')},
        'note':'Observed is reviewed source evidence, not verified coverage or a shipped implementation.'}


def reconcile(execution, task):
    """Materialize an approved plan once; publish through ordinary review/CI/merge."""
    from .stages import checkout
    store=execution.store;data=task['data'];plan=data['decomposition'];audit=data['spec_audit']
    plan_id=digest(plan)
    if plan_id in data.get('rejected_plans',[]):raise Attention('Rejected backlog plan cannot be republished unchanged')
    # No network operation or checkout mutation runs under the SQLite transaction.
    with (store.directory/'backlog.lock').open('a') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        execution.current()
        if not current_evidence(store.directory/'source',audit['source_evidence']):
            return {'stale_plan':True},{'spec_audit':None,'decomposition':None,'plan_review':None,
                'status':'pending','audit_cycle':data.get('audit_cycle',0)+1},[]
        validate_plan(plan,audit,store)
        with store.transaction():
            gap_identity=digest([sorted(g['id'] for g in audit['gaps']),
                sorted((r['file'],r['sha256']) for r in audit['source_evidence'] if EVIDENCE.production_source(r['file']))])
            key='spec_gap_owner:'+task['name']+':'+task['generation']+':'+gap_identity
            owner=store.setting(key)
            if owner and owner!=plan_id:
                error=Attention('The same spec gaps and unchanged production evidence already have a reviewed plan. Diagnose its delivery instead of creating redundant repair work.')
                error.category='spec_fidelity';raise error
            store.db.execute('INSERT OR IGNORE INTO settings VALUES(?,?)',(key,json.dumps(plan_id)))
            row=store.db.execute('SELECT * FROM backlog_publications WHERE planner=? AND generation=? AND plan=?',
                                (task['name'],task['generation'],plan_id)).fetchone()
            if row:
                names=json.loads(row['names'])
            else:
                new=[t for t in plan['tasks'] if not t.get('existing_task')]
                allocated=allocate(store,len(new)+(1 if new else 0),'spec plan '+plan_id)
                names={t['key']:t['existing_task'] for t in plan['tasks'] if t.get('existing_task')}
                names.update({t['key']:allocated[i] for i,t in enumerate(new)})
                names['_publication']=allocated[-1] if new else None
                names['_base']=store.setting('observed_main',execution.claim['input']['base'])
                store.db.execute('INSERT INTO backlog_publications VALUES(?,?,?,?,NULL)',
                    (task['name'],task['generation'],plan_id,json.dumps(names)))
        linked=[names[t['key']] for t in plan['tasks']]
        publication=names['_publication']
        if publication and not store.db.execute('SELECT 1 FROM tasks WHERE name=?',(publication,)).fetchone():
            base=names['_base']
            path=checkout(execution,base)
            registry=json.loads((path/'specs/task-spec-inputs.json').read_text())
            original_registry=json.loads(json.dumps(registry))
            if GOAL_REF not in registry['bindings']:raise Attention('Backlog publication requires the explicit GOAL binding')
            expected={};bindings={};definitions={}
            for item in plan['tasks']:
                if item.get('existing_task'):continue
                name=names[item['key']]
                ref=f"Spec gap {task['name']} / {item['key']} / {task['generation'][:12]}"
                ids=[]
                for identity in item['covers']:
                    section='backlog/'+identity
                    heading=data['sections'][identity]
                    definition={'file':data['document'],'heading':heading} if heading else {'file':data['document'],'document':True}
                    if section in registry['sections'] and registry['sections'][section]!=definition:
                        raise Attention('Existing spec section ID would be retargeted: '+section)
                    registry['sections'][section]=definition;definitions[section]=definition;ids.append(section)
                if ref in registry['bindings'] and registry['bindings'][ref]!=ids:raise Attention('Existing binding collision')
                registry['bindings'][ref]=ids;bindings[ref]=ids
                deps=[names.get(d,d) for d in item['dependencies']]
                body=('---\ntitle: '+json.dumps(item['title'],ensure_ascii=False)+'\nspec_ref: '+json.dumps(ref,ensure_ascii=False)+
                    '\ndepends_on: ['+', '.join(deps)+']\nprogress: not-started\ncommits: []\n---\n\n## Required behavior\n\n'+
                    item['required_behavior']+'\n\n## Acceptance Criteria\n\n'+
                    '\n'.join('- [ ] '+a for a in item['acceptance'])+'\n\n## Verification intent\n\n'+item['negative_probe']+
                    '\n\nUpdate the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.\n\n## Discovery evidence\n\nCanonical spec: `'+data['document']+'`. Source: `'+audit['base']+'`; plan: `'+plan_id+'`.\n\n'+
                    json.dumps([g for g in audit['gaps'] if g['id'] in item['covers']],ensure_ascii=False,indent=2)+'\n')
                expected['specs/tasks/'+name+'.md']=body
            pub_body=('---\ntitle: "Publish reviewed spec backlog for '+data['document']+'"\nspec_ref: '+
                json.dumps(GOAL_REF,ensure_ascii=False)+'\ndepends_on: []\nprogress: ready-for-review\ncommits: []\n---\n\n'
                '## Required behavior\n\nPublish exactly the independently reviewed missing-work plan below. '
                'Only create the assigned task documents and add their exact section bindings. '
                'Preserve every existing task, spec, registry entry, coverage status, product file and gate. '
                'Task creation is not implementation. Re-read the canonical spec and challenge the audit, '
                'decomposition, dependencies and duplicate-work analysis before approving.\n\n'+
                json.dumps({'document':data['document'],'plan_id':plan_id,'audit':audit,'plan':plan,
                            'assigned_names':names},ensure_ascii=False,indent=2)+
                '\n\n## Acceptance Criteria\n\n- [ ] Every planned task faithfully implements its assigned canonical requirements.\n'
                '- [ ] No duplicate active assignment or missing dependency is introduced.\n'
                '- [ ] No product behavior, spec requirement, coverage status or enforcement is changed.\n'
                '- [ ] New tasks remain not-started until this backlog publication merges.\n\n'
                '## Shipped\n\nTask definitions and explicit bindings only; no product implementation claimed.\n')
            expected['specs/tasks/'+publication+'.md']=pub_body
            for relative,body in expected.items():
                target=path/relative;target.parent.mkdir(parents=True,exist_ok=True);target.write_text(body)
            (path/'specs/task-spec-inputs.json').write_text(json.dumps(registry,ensure_ascii=False,indent=2)+'\n')
            scoped=snapshot(path,pub_body,contract,require_registry=True)
            if scoped['errors']:raise Attention('Invalid backlog publication binding: '+json.dumps(scoped['errors']))
            for relative,body in expected.items():
                inputs=snapshot(path,body,contract,require_registry=True)
                if inputs['errors']:raise Attention('Generated task has invalid bindings: '+relative)
            branch='pipeline/backlog/'+plan_id[:24]
            execution.command('git','checkout','-B',branch,cwd=path)
            execution.command('git','add','specs/tasks','specs/task-spec-inputs.json',cwd=path)
            timestamp=execution.command('git','show','-s','--format=%cI',base,cwd=path).stdout.strip()
            prior=os.environ.get('GIT_COMMITTER_DATE');os.environ['GIT_COMMITTER_DATE']=timestamp
            try:
                execution.command('git','-c','user.name=gyre-pipeline','-c','user.email=jsell-rh@users.noreply.github.com',
                                  '-c','core.hooksPath=/dev/null','commit','--no-gpg-sign','--date='+timestamp,
                                  '-m','chore: publish reviewed spec backlog '+plan_id[:12],cwd=path)
            finally:
                if prior is None:os.environ.pop('GIT_COMMITTER_DATE',None)
                else:os.environ['GIT_COMMITTER_DATE']=prior
            head=execution.command('git','rev-parse','HEAD',cwd=path).stdout.strip()
            # Private immutable branch. Never overwrite an unexpected remote receipt.
            result=execution.command('git','push','--force-with-lease=refs/heads/'+branch+':','origin','HEAD:refs/heads/'+branch,
                                     cwd=path,check=False,timeout=180)
            if result.returncode:
                remote=execution.command('git','ls-remote','--heads','origin',branch,cwd=path).stdout.split()
                if not remote or remote[0]!=head:raise Retry('Backlog checkpoint publication unavailable; retained plan/IDs remain reserved')
            pub_data=metadata(pub_body)|{'requirements':scoped,'catalog_generation':scoped['fingerprint'],
                'catalog_source':base,'triaged':scoped['fingerprint'],'candidate':head,'candidate_base':base,
                'branch':branch,'backlog_manifest':{'files':{p:digest(b) for p,b in expected.items() if not p.endswith(publication+'.md')},
                    'bindings':bindings,'sections':definitions,'document':data['document'],'requirements':task['generation'],'original_registry':original_registry,'spec_hash':digest(data['spec_text'])},
                'backlog_plan':plan_id,'decision_allowed_paths':list(expected)+['specs/task-spec-inputs.json']}
            store.put_task(publication,scoped['fingerprint'],pub_body,pub_data,fence=(execution.claim['id'],execution.claim['token']))
            with store.transaction():
                store.db.execute('UPDATE backlog_publications SET head=? WHERE planner=? AND generation=? AND plan=?',
                                 (head,task['name'],task['generation'],plan_id))
                store.event('spec_backlog_published',{'plan':plan_id,'publication':publication,'tasks':linked,'head':head},task['name'])
        return {'backlog_publication':publication,'tasks':linked},{'status':'assigned','backlog_tasks':linked,
                'backlog_publication':publication},[]


def validate_manifest(path, manifest):
    """Implementation/rebase may not rewrite reviewed generated task contracts."""
    root=Path(path)
    validate_spec_source(root,manifest)
    for relative,expected in manifest['files'].items():
        target=root/relative
        if not target.is_file() or digest(target.read_text())!=expected:
            raise Attention('Backlog task definition differs from independently reviewed plan: '+relative)
    registry=json.loads((root/'specs/task-spec-inputs.json').read_text())
    for kind in ('bindings','sections'):
        for key,value in manifest[kind].items():
            if registry[kind].get(key)!=value:raise Attention('Backlog binding differs from approved plan: '+key)

    original=manifest['original_registry']
    for kind,values in original.items():
        if kind in ('bindings','sections'):
            if any(registry.get(kind,{}).get(key)!=value for key,value in values.items()):
                raise Attention('Backlog publication rewrote an existing spec binding')
        elif registry.get(kind)!=values:
            raise Attention('Backlog publication changed global registry policy')


def validate_spec_source(root, manifest):
    path=Path(root)/manifest['document']
    if not path.is_file() or digest(path.read_text())!=manifest['spec_hash']:
        error=Attention('Canonical spec changed after this backlog plan was reviewed; a fresh audit/plan is required')
        error.category='spec_plan_stale';raise error
