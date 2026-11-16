"""Reconcile only holds whose recorded cause an installed policy can recover."""
import json
import re
import time

from .compute import oom_limit, profile
from .repair_budget import task_document_repair
from .repairs import satisfied


def checkbox_delta(delta):
    if not isinstance(delta,str) or not delta or len(delta)>=6000:return False
    removed=[line[1:] for line in delta.splitlines() if line.startswith('-') and not line.startswith('---')]
    added=[line[1:] for line in delta.splitlines() if line.startswith('+') and not line.startswith('+++')]
    def normalize(line):return re.sub(r'^(\s*[-*+] \[)[ xX](\] )',r'\1 \2',line)
    return bool(removed and removed!=added and list(map(normalize,removed))==list(map(normalize,added)))


def reconcile(store,now=None):
    now=time.time() if now is None else now
    inventory=store.db.execute("SELECT at FROM events WHERE kind='gateway_inventory' ORDER BY id DESC LIMIT 1").fetchone()
    gateway_healthy=bool(inventory and 0<=now-inventory['at']<120
                         and 0<=now-store.setting('dispatcher_health_at',0)<120)
    recovered=[]
    for task in store.tasks():
        name=task['name'];gen=task['generation'];data=task['data']
        if satisfied(data) or store.setting('task_paused:'+name,False):continue
        for stage in ('assess','implement','review','verify'):
            key=store.runtime_hold_key(name,stage,gen);hold=store.setting(key)
            if not hold:continue
            category=hold.get('category');policy=None;diagnostic=hold.get('diagnostic') or {}
            if category=='sandbox_oom' and oom_limit(diagnostic)==4 and profile(store,name,stage)['memory_gib']==6:
                policy='recorded-4-to-6-gib-v1'
            elif (gateway_healthy and category=='gateway_transport' and stage in ('assess','verify')
                  and hold.get('message','').startswith('Cannot establish verifier health;')):
                policy='retain-verifier-on-unknown-health-v1'
            elif (gateway_healthy and category=='sandbox_stopped' and stage=='review'
                  and data.get('candidate') and data.get('candidate_base') and not data.get('repair')
                  and not store.db.execute("SELECT 1 FROM resources r JOIN work w ON w.id=r.work WHERE w.task=? AND r.state<>'absent'",(name,)).fetchone()):
                # The reviewer owns no production edits. Once its failed runtime
                # is gone, one fresh review of the retained source is safe.
                policy='retained-source-terminal-review-v1'
            elif category=='repair_stalled' and stage=='implement':
                repair=data.get('repair') or {}
                if repair.get('category')=='contract' and checkbox_delta(repair.get('log_tail')):
                    policy='normalize-retained-checkbox-checkpoint-v3'
                elif task_document_repair(name,repair):policy='review-task-document-progress-v1'
            if not policy:continue
            receipt=f'hold_recovery:{name}:{stage}:{gen}:{policy}'
            with store.transaction():
                current=store.task(name)
                if (current['generation']!=gen or current['data']!=data or store.setting(key)!=hold
                        or store.setting(receipt) or store.db.execute(
                            "SELECT 1 FROM work WHERE task=? AND state='claimed'",(name,)).fetchone()):continue
                # Historical attempts/events and source receipts remain. The
                # normal discover/claim predicates select current input/base.
                store.db.execute("UPDATE work SET state='obsolete',token=token+1,retry_at=0,updated=? WHERE task=? AND generation=? AND stage=? AND state='attention' AND json_extract(result,'$.category')=?",
                                 (now,name,gen,stage,category))
                if policy in ('normalize-retained-checkbox-checkpoint-v3','retained-source-terminal-review-v1'):
                    # Adoption would replay the already-rejected checkbox-only
                    # receipt before the corrected finalizer ever gets to run.
                    # The retained candidate stays the seed for a fresh model.
                    store.db.execute("UPDATE work SET result=json_set(result,'$.fresh_model',json('true')) WHERE task=? AND generation=? AND stage=? AND state='obsolete' AND json_extract(result,'$.category')=?",
                                     (name,gen,stage,category))
                store.db.execute('DELETE FROM settings WHERE key=?',(key,))
                record={'at':now,'policy':policy,'previous':hold}
                store.db.execute('INSERT INTO settings VALUES(?,?)',(receipt,json.dumps(record)))
                store.event('runtime_hold_recovered',{'stage':stage,**record},name)
                recovered.append({'task':name,'stage':stage,'policy':policy})
    recovered.extend(recover_assessment_reports(store, now))
    return recovered


def recover_assessment_reports(store, now):
    """One schema-policy retry for legacy malformed reports, never spec conflicts.

    Completed/current assessments and operator pauses take precedence. Same
    immutable assignment is retried with fresh_model so adoption cannot replay
    the invalid receipt. A new base alone does not buy another policy retry.
    """
    recovered=[]
    for task in store.tasks():
        if satisfied(task['data']) or store.setting('task_paused:'+task['name'],False):continue
        for row in store.db.execute("SELECT * FROM work WHERE task=? AND generation=? AND stage='assess' AND state='attention'",(task['name'],task['generation'])).fetchall():
            reason=json.loads(row['result'] or '{}');message=reason.get('message','')
            if reason.get('category')!='assessment' or not any(text in message for text in (
                    'assessment requirement fingerprint does not match assignment',
                    'assessment needs concrete requirement-to-source evidence', 'Assessment omits requirement evidence')):continue
            inputs=json.loads(row['input'])
            if inputs.get('requirements')!=(task['data'].get('requirements') or {}).get('fingerprint'):continue
            receipt=f"assessment_report_recovery:{task['name']}:{task['generation']}:schema-v1"
            with store.transaction():
                if (store.setting(receipt) or store.task(task['name'])['data']!=task['data']
                    or store.db.execute("SELECT 1 FROM work WHERE task=? AND state='claimed'",(task['name'],)).fetchone()):continue
                store.db.execute("UPDATE work SET state='retry',retry_at=0,result=?,updated=? WHERE id=? AND state='attention'",
                    (json.dumps(reason|{'fresh_model':True}),now,row['id']))
                store.db.execute('INSERT INTO settings VALUES(?,?)',(receipt,json.dumps({'at':now,'work':row['id'],'previous':reason})))
                store.event('assessment_report_recovered',{'work':row['id'],'policy':'schema-v1'},task['name'])
                recovered.append({'task':task['name'],'stage':'assess','policy':'schema-v1'})
    return recovered
