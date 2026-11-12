"""Observe a published head independently of integration/base freshness."""
import json
import time
import shutil
import subprocess
from pathlib import Path

from .catalog import helper
from .execution import Wait
from .store import StaleClaim


def publication(store, task):
    data = task['data']
    head = data.get('published_head')
    if not data.get('pr') or not head:
        return None
    for receipt in (data.get('published_verified'), data.get('verified')):
        if isinstance(receipt,dict) and receipt.get('head') == head:
            return receipt
    # Adopt pre-upgrade publication receipts from the existing durable event;
    # never infer a tested base from the current tip of main.
    for row in store.db.execute("SELECT e.detail,w.input FROM events e JOIN work w ON w.id=e.work WHERE e.task=? AND e.kind='pull_request' ORDER BY e.id DESC",(task['name'],)):
        if json.loads(row['detail']).get('head') == head:
            receipt = json.loads(row['input']).get('verified')
            if isinstance(receipt,dict) and receipt.get('head') == head:
                return receipt
    return None


def includes_base(store, base, target):
    if not base or not target:return False
    if base==target:return True
    try:
        return subprocess.run(['git','-C',str(store.directory/'source'),'merge-base','--is-ancestor',target,base],
                              capture_output=True,timeout=10).returncode==0
    except (OSError,subprocess.TimeoutExpired):return False


def verification_ready(store, task, base=None):
    published = publication(store,task)
    if not published or published.get('candidate') != task['data'].get('candidate'):
        return True
    for key in ('ci_refresh','baseline_resolution'):
        recovery=task['data'].get(key) or {}
        if (recovery.get('candidate')==task['data'].get('candidate') and recovery.get('published_head')==published['head']
                and recovery.get('generation')==task['generation']):
            if key=='ci_refresh':return includes_base(store,base,recovery.get('target_base'))
            from .repairs import satisfied
            prerequisite=store.task(recovery['prerequisite'])
            receipt=recovery.get('receipt') or {}
            if satisfied(prerequisite['data']) and receipt in (prerequisite['data'].get('delivered'),prerequisite['data'].get('resolved')):
                return includes_base(store,base,receipt.get('sha') or receipt.get('base') or receipt.get('source'))
    ci = task['data'].get('ci') or {}
    return (ci.get('head') == published['head'] and ci.get('checks') == 'green'
            and ci.get('status') in ('passed','policy_pending','artifact_refresh'))


def completed_refresh(store, task, verified, previous, observation):
    """Do not discard a successful refresh by rediscovering its old CI failure."""
    marker=task['data'].get('ci_refresh') or {}
    return (observation.get('status')=='environment_refresh'
        and marker.get('candidate')==verified.get('candidate')==task['data'].get('candidate')
        and marker.get('published_head')==previous['head']
        and marker.get('generation')==task['generation']
        and marker.get('failure_signature')==observation.get('failure_signature')
        and includes_base(store,verified['base'],observation['target_base']))


def generated_conflict(execution, task, receipt, base):
    """Reproduce conflict paths using Git only; never allocate a sandbox/model."""
    from .stages import checkout
    path=checkout(execution,receipt['head'])
    try:
        execution.command('git','fetch','--quiet','origin',base,cwd=path,timeout=180)
        execution.command('git','config','user.name','gyre-pipeline',cwd=path)
        execution.command('git','config','user.email','jsell-rh@users.noreply.github.com',cwd=path)
        merged=execution.command('git','-c','core.hooksPath=/dev/null','merge','--no-commit','--no-ff',base,cwd=path,check=False)
        conflicts=execution.command('git','diff','--name-only','--diff-filter=U',cwd=path).stdout.splitlines()
        if merged.returncode and not conflicts:
            raise Wait('Could not reproduce PR conflict paths; retry observation')
        return all(name.startswith('web/dist/') for name in conflicts),conflicts
    finally:
        shutil.rmtree(path,ignore_errors=True)


def read(execution, task, receipt):
    module = helper('dev-ci')
    observation = module.observe(execution.command, task['data']['pr'], receipt['head'],
                                          receipt['base'], execution.directory)
    latest = json.loads(execution.command('gh','pr','view',task['data']['pr'],'--json',
                       'headRefOid,statusCheckRollup,state,baseRefOid,mergeStateStatus,mergeable',timeout=30).stdout)
    if latest['headRefOid'] != receipt['head']:
        raise Wait('PR changed during CI observation; discard the stale observation')
    if observation['status'] == 'merge_conflict' and (not module.merge_conflict(latest) or latest['baseRefOid'] != observation['base']):
        raise Wait('PR conflict or target base changed during observation; recheck before requesting implementation')
    latest, reconciled = module.reconcile_checks(execution.command,task['data']['pr'],receipt['head'],latest)
    pending, failed, infrastructure = module.check_sets(latest)
    if observation['status'] in ('candidate_failed','baseline_failed','environment_failed','environment_refresh') and (not failed or latest['state'] != 'OPEN'):
        raise Wait('Failed CI changed while evidence was collected; observe again before requesting implementation')
    if observation['status'] in ('passed','policy_pending') and (pending or failed or infrastructure or not latest.get('statusCheckRollup')):
        raise Wait('CI readiness changed during observation; recheck before publication')
    if observation['status']=='merge_conflict' and latest.get('statusCheckRollup') and not (pending or failed or infrastructure):
        old=task['data'].get('ci') or {}
        cached=(old.get('status')=='artifact_refresh' and old.get('head')==receipt['head'] and old.get('target_base')==observation['base'])
        generated,conflicts=(True,old.get('conflicts',[])) if cached else generated_conflict(execution,task,receipt,observation['base'])
        if not cached:
            confirmed=json.loads(execution.command('gh','pr','view',task['data']['pr'],'--json',
                'headRefOid,statusCheckRollup,state,baseRefOid,mergeStateStatus,mergeable',timeout=30).stdout)
            cp,cf,ci=module.check_sets(confirmed)
            if (confirmed.get('headRefOid')!=receipt['head'] or confirmed.get('baseRefOid')!=observation['base']
                    or not module.merge_conflict(confirmed) or cp or cf or ci or not confirmed.get('statusCheckRollup')):
                raise Wait('PR changed during conflict-path inspection; discard stale classification')
        observation['conflicts']=conflicts
        if generated:
            observation.update(status='artifact_refresh',checks='green')
    snapshot = {'head':receipt['head'],'base':receipt['base'],'status':observation['status'],
                'checks':observation.get('checks'), 'merge_state':observation.get('merge_state'),
                'checked_at':time.time(), 'log':observation.get('log'),
                'target_base':observation.get('target_base') if observation['status']=='environment_refresh' else observation.get('base') if observation['status']=='artifact_refresh' else None,
                'conflicts':observation.get('conflicts',[])}
    with execution.store.transaction():
        execution.store._current(execution.claim['id'],execution.claim['token'])
        current = execution.store.task(task['name'])
        if (current['generation'] != task['generation'] or
                current['data'].get('candidate') != task['data'].get('candidate') or
                current['data'].get('published_head') != receipt['head'] or
                current['data'].get('pr') != task['data'].get('pr')):
            raise StaleClaim('published candidate changed during CI observation')
        data = current['data']
        old = data.get('ci') or {}
        if snapshot['status'] in ('pending','policy_pending','infrastructure'):
            snapshot['waiting_since'] = (old.get('waiting_since') if old.get('head') == snapshot['head'] else None) or snapshot['checked_at']
        if reconciled:
            snapshot['reconciled_checks'] = reconciled
        data['ci'] = snapshot
        execution.store.db.execute('UPDATE tasks SET data=? WHERE name=?',(json.dumps(data),task['name']))
        if old.get('head') != snapshot['head'] or old.get('status') != snapshot['status']:
            execution.store.event('ci_observed',snapshot,task['name'],execution.claim['id'])
    return observation


def failure(execution, task, receipt, observation):
    status = observation['status']
    if status not in ('candidate_failed','baseline_failed','environment_failed','environment_refresh','closed','merge_conflict'):
        return None
    finding = observation | {'id':execution.claim['id'],'category':'ci','source':receipt['head']}
    if observation.get('log'):
        finding['log_tail'] = Path(observation.get('candidate_log',observation['log'])).read_text()[-16000:]
    if status=='environment_refresh':
        return observation, {'verified':None,'ci_refresh':{
            'candidate':task['data']['candidate'],'published_head':receipt['head'],'generation':task['generation'],
            'target_base':observation['target_base'],'failure_signature':observation['failure_signature']}}, []
    if status == 'closed':
        # Human closure is not a request for another implementation/PR.
        raise Wait('Associated PR was closed; awaiting an operator decision')
    if status == 'merge_conflict':
        finding.update(category='rebase',published_head=receipt['head'])
        return observation, {'repair':finding,'candidate':receipt['head'],'candidate_base':receipt['base'],
                             'review':None,'verified':None}, [finding]
    if status in ('baseline_failed','environment_failed'):
        from .stages import baseline_repair
        prerequisite = baseline_repair(execution,observation)
        finding.update(category='baseline',prerequisite=prerequisite,
                       source=task['data']['candidate'],published_head=receipt['head'])
        if status=='environment_failed':
            finding['shared_environment']=True
            return observation, {'repair':finding}, [finding]
        return observation, {'repair':finding,
                'dependencies':sorted(set(task['data'].get('dependencies') or []) | {prerequisite})}, [finding]
    return observation, {'repair':finding,'candidate':receipt['head'],'candidate_base':receipt['base'],
                          'review':None,'verified':None}, [finding]


def observe(execution, task):
    labels=json.loads(execution.command('gh','pr','view',task['data']['pr'],'--json','labels',timeout=30).stdout)
    priority_labels=sorted({label['name'] for label in labels.get('labels',[]) if label['name'].startswith('priority/')})
    with execution.store.transaction():
        execution.store._current(execution.claim['id'],execution.claim['token'])
        current=execution.store.task(task['name'])
        if current['data'].get('pr') != task['data'].get('pr'):
            raise StaleClaim('associated PR changed during priority observation')
        data=current['data']
        if data.get('priority_labels') != priority_labels:
            data['priority_labels']=priority_labels
            execution.store.db.execute('UPDATE tasks SET data=? WHERE name=?',(json.dumps(data),task['name']))
            execution.store.event('priority_labels_observed',{'labels':priority_labels},task['name'],execution.claim['id'])
    if execution.claim['input'].get('priority_only'):
        task=execution.store.task(task['name'])
        receipt=publication(execution.store,task)
        if (task['data'].get('repair') or {}).get('category')=='ci' and receipt:
            observation=read(execution,task,receipt)
            recover_setup_handoff(execution,execution.store.task(task['name']),receipt,observation)
            if observation['status']=='infrastructure':retry_infrastructure(execution,task,receipt,observation)
        wait=Wait('Watching PR priority labels while implementation handles the recorded finding')
        wait.retry_delay=execution.store.setting('priority_poll_seconds',600)
        raise wait
    receipt = execution.claim['input']['published']
    observation = read(execution,task,receipt)
    rejected = failure(execution,task,receipt,observation)
    if rejected:
        return rejected
    if observation['status'] == 'merged':
        # Publication must still verify the authoritative merge tree.
        return observation, {'verified':receipt}, []
    if observation['status'] == 'infrastructure':
        retry_infrastructure(execution,task,receipt,observation)
    raise Wait('PR CI readiness: ' + observation['status'])


def retry_infrastructure(execution, task, receipt, observation):
    """Bound reruns per exact head, deduplicate attempts and back off durably."""
    from .execution import Attention
    key='ci_infra_retry:'+task['name']+':'+receipt['head']
    state=execution.store.setting(key,{})
    now=time.time()
    if state.get('next',0)>now:
        error=Wait('GitHub infrastructure retry is backing off on the same head')
        error.retry_delay=max(1,state['next']-now);raise error
    count=state.get('count',0)
    if count>=execution.store.setting('ci_infra_retry_limit',6):
        error=Attention('GitHub setup infrastructure remains unavailable after bounded reruns; source is retained')
        error.category='ci_infrastructure';raise error
    repo=execution.store.setting('repository','jsell-rh/gyre')
    attempts=[]
    for ident in observation.get('runs',[]):
        if ident is None:continue
        run=json.loads(execution.command('gh','api',f'repos/{repo}/actions/runs/{ident}',timeout=30).stdout)
        if run.get('head_sha')!=receipt['head']:raise Wait('Workflow head changed before infrastructure rerun')
        attempt=[ident,run.get('run_attempt')]
        if run.get('status')!='completed' or attempt in state.get('attempts',[]):continue
        execution.command('gh','run','rerun',str(ident),'--failed','--repo',repo,timeout=30)
        attempts.append(attempt)
    delay=min(1800,60*2**min(count,5))
    execution.store.set_setting(key,{'count':count+bool(attempts),'next':now+delay,
        'attempts':state.get('attempts',[])+attempts})
    if attempts:execution.store.event('ci_infrastructure_rerun',{'head':receipt['head'],'attempts':attempts,'delay':delay},task['name'],execution.claim['id'])
    error=Wait('GitHub infrastructure checks retry on the same verified head; no coding requested')
    error.retry_delay=delay;raise error


def recover_setup_handoff(execution,task,receipt,observation):
    """Undo an old CI-to-coding handoff only when exact-source evidence permits.

    Running code work is never canceled. Source changed since publication is
    retained for normal review. Only the unchanged published head can recover
    its actual recorded independent approval and verification.
    """
    if observation['status'] not in ('infrastructure','passed','policy_pending','environment_refresh'):return False
    data=task['data'];repair=data.get('repair') or {}
    if (repair.get('category')!='ci' or repair.get('source')!=receipt['head']
            or data.get('candidate')!=receipt['head'] or not receipt.get('passed')):return False
    review=None
    for row in execution.store.db.execute("SELECT * FROM work WHERE task=? AND generation=? AND stage='review' AND state='succeeded' ORDER BY updated DESC",(task['name'],task['generation'])):
        result=json.loads(row['result'] or '{}');verdict=result.get('verdict') or {}
        if (json.loads(row['input']).get('candidate')==receipt['candidate'] and result.get('valid')
                and verdict.get('approved') is True and not verdict.get('findings')
                and verdict.get('candidate')==receipt['candidate']):
            review={'candidate':receipt['candidate'],'approved':True,'checkpoint':result['head'],
                    'branch':result.get('branch'),'recovered_from':row['id']};break
    if not review:return False
    with execution.store.transaction():
        current=execution.store.task(task['name'])
        if current['generation']!=task['generation'] or current['data']!=data:return False
        if execution.store.db.execute("SELECT 1 FROM work WHERE task=? AND state='claimed' AND stage NOT IN ('observe','cleanup')",(task['name'],)).fetchone():return False
        execution.store.db.execute('INSERT INTO task_history(task,generation,body,data,at) VALUES(?,?,?,?,?)',
            (task['name'],task['generation'],task['body'],json.dumps(data),time.time()))
        restored=data|{'candidate':receipt['candidate'],'candidate_base':receipt['base'],'repair':None,'review':review,'verified':receipt}
        execution.store.db.execute('UPDATE tasks SET data=? WHERE name=?',(json.dumps(restored),task['name']))
        execution.store.db.execute("UPDATE work SET state='obsolete',token=token+1,updated=? WHERE task=? AND stage='implement' AND state IN ('ready','retry')",(time.time(),task['name']))
        execution.store.event('ci_setup_handoff_recovered',{'head':receipt['head'],'review_work':review['recovered_from']},task['name'],execution.claim['id'])
    return True
