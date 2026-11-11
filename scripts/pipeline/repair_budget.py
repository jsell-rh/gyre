"""Bound unchanged repair cycles across work identities and main advances."""
import hashlib
import json
import re
from pathlib import PurePosixPath

from .execution import Attention


def source_findings(findings):
    if not isinstance(findings,list):return findings,[]
    source=[];generated=[]
    for finding in findings:
        name=finding.get('file','') if isinstance(finding,dict) else ''
        # Only explicit generated paths qualify. General frontend, build, test,
        # security or source findings are never suppressed by text matching.
        name=name if isinstance(name,str) else ''
        path=PurePosixPath(name.split(':',1)[0])
        target=generated if re.fullmatch(r'web/dist/[A-Za-z0-9_.@/-]*(?::[0-9]+(?::[0-9]+)?)?',name) and '..' not in path.parts else source
        target.append(finding)
    return source,generated


def check(execution, task, path, candidate, findings):
    tree=execution.command('git','ls-tree','-r',candidate,cwd=path).stdout
    production='\n'.join(line for line in tree.splitlines() if not line.split('\t',1)[-1].startswith(
        ('specs/tasks/','specs/reviews/','web/dist/')))
    fingerprint=hashlib.sha256(json.dumps([production,findings],sort_keys=True).encode()).hexdigest()
    key='repair_budget:'+task['name']+':'+task['generation']+':'+execution.claim['stage']
    state=execution.store.setting(key,{})
    count=state.get('count',0)+1 if state.get('fingerprint')==fingerprint else 1
    record={'fingerprint':fingerprint,'count':count,'candidate':candidate,'work':execution.claim['id']}
    execution.store.set_setting(key,record)
    if count>=execution.store.setting('repair_stall_limit',3):
        error=Attention('Repair stalled: identical findings and unchanged production source across repeated rounds; retained checkpoint needs diagnosis')
        error.category='repair_stalled';error.diagnostic=record;raise error


def reconcile_generated(store):
    """Re-review old generated-only rejections without another coding round."""
    resolved=[]
    for task in store.tasks():
        data=task['data'];repair=data.get('repair') or {};review=data.get('review') or {}
        if data.get('assessment_pending') or (data.get('requirements') or {}).get('errors'):continue
        if repair.get('category')=='review' and review.get('candidate')==data.get('candidate'):
            findings,deferred=source_findings(repair.get('details'))
            if findings or not deferred:continue
        elif not repair and not review and not data.get('review_retry'):
            # Recover an older executor that already deferred this rejection
            # but did not give the new review a distinct work identity.
            event=store.db.execute("SELECT detail FROM events WHERE task=? AND kind='generated_only_review_rescheduled' ORDER BY id DESC LIMIT 1",(task['name'],)).fetchone()
            previous=json.loads(event['detail']) if event else {}
            if not data.get('candidate') or previous.get('candidate')!=data['candidate']:continue
            deferred=previous.get('deferred',[])
        else:continue
        with store.transaction():
            current=store.task(task['name'])
            if current['generation']!=task['generation'] or current['data']!=data:continue
            if store.db.execute("SELECT 1 FROM work WHERE task=? AND state='claimed'",(task['name'],)).fetchone():continue
            store.db.execute('INSERT INTO task_history(task,generation,body,data,at) VALUES(?,?,?,?,unixepoch())',
                (task['name'],task['generation'],task['body'],json.dumps(data)))
            store.db.execute('UPDATE tasks SET data=? WHERE name=?',(json.dumps(data|{'repair':None,'review':None,'verified':None,'review_retry':{'candidate':data['candidate'],'policy':'generated-assets-owned-by-verifier-v1'}}),task['name']))
            store.db.execute("UPDATE work SET state='obsolete',token=token+1,updated=unixepoch() WHERE task=? AND stage='implement' AND state IN ('ready','retry')",(task['name'],))
            store.event('generated_only_review_rescheduled',{'candidate':data['candidate'],'deferred':deferred},task['name'])
            resolved.append(task['name'])
    return resolved


def reset(store, task, stage):
    """An explicit operator retry grants a fresh budget and retains its audit."""
    prefixes=['repair_budget:'+task+':']
    if stage in ('observe','publish'):prefixes.append('ci_infra_retry:'+task+':')
    for prefix in prefixes:
        for row in store.db.execute('SELECT key,value FROM settings WHERE key LIKE ?',(prefix+'%',)).fetchall():
            if row['key'].startswith('repair_budget:') and not row['key'].endswith(':'+stage):continue
            store.event('operator_retry_budget_reset',{'key':row['key'],'previous':json.loads(row['value'])},task)
            store.db.execute('DELETE FROM settings WHERE key=?',(row['key'],))
