"""Observe a published head independently of integration/base freshness."""
import json
import time
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


def verification_ready(store, task):
    published = publication(store,task)
    if not published or published.get('candidate') != task['data'].get('candidate'):
        return True
    ci = task['data'].get('ci') or {}
    return (ci.get('head') == published['head'] and ci.get('checks') == 'green'
            and ci.get('status') in ('passed','policy_pending'))


def read(execution, task, receipt):
    module = helper('dev-ci')
    observation = module.observe(execution.command, task['data']['pr'], receipt['head'],
                                          receipt['base'], execution.directory)
    latest = json.loads(execution.command('gh','pr','view',task['data']['pr'],'--json',
                       'headRefOid,statusCheckRollup,state',timeout=30).stdout)
    if latest['headRefOid'] != receipt['head']:
        raise Wait('PR changed during CI observation; discard the stale observation')
    pending, failed, infrastructure = module.check_sets(latest)
    if observation['status'] in ('candidate_failed','baseline_failed') and (not failed or latest['state'] != 'OPEN'):
        raise Wait('Failed CI changed while evidence was collected; observe again before requesting implementation')
    if observation['status'] in ('passed','policy_pending') and (pending or failed or infrastructure or not latest.get('statusCheckRollup')):
        raise Wait('CI readiness changed during observation; recheck before publication')
    snapshot = {'head':receipt['head'],'base':receipt['base'],'status':observation['status'],
                'checks':observation.get('checks'), 'merge_state':observation.get('merge_state'),
                'checked_at':time.time(), 'log':observation.get('log')}
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
        data['ci'] = snapshot
        execution.store.db.execute('UPDATE tasks SET data=? WHERE name=?',(json.dumps(data),task['name']))
        if old.get('head') != snapshot['head'] or old.get('status') != snapshot['status']:
            execution.store.event('ci_observed',snapshot,task['name'],execution.claim['id'])
    return observation


def failure(execution, task, receipt, observation):
    status = observation['status']
    if status not in ('candidate_failed','baseline_failed','closed'):
        return None
    finding = observation | {'id':execution.claim['id'],'category':'ci','source':receipt['head']}
    if observation.get('log'):
        finding['log_tail'] = Path(observation.get('candidate_log',observation['log'])).read_text()[-16000:]
    if status == 'closed':
        # Human closure is not a request for another implementation/PR.
        raise Wait('Associated PR was closed; awaiting an operator decision')
    if status == 'baseline_failed':
        from .stages import baseline_repair
        prerequisite = baseline_repair(execution,observation)
        finding.update(category='baseline',prerequisite=prerequisite)
        return observation, {'repair':finding,
                'dependencies':sorted(set(task['data'].get('dependencies') or []) | {prerequisite})}, [finding]
    return observation, {'repair':finding,'candidate':receipt['head'],'candidate_base':receipt['base'],
                          'review':None,'verified':None}, [finding]


def observe(execution, task):
    receipt = execution.claim['input']['published']
    observation = read(execution,task,receipt)
    rejected = failure(execution,task,receipt,observation)
    if rejected:
        return rejected
    if observation['status'] == 'merged':
        # Publication must still verify the authoritative merge tree.
        return observation, {'verified':receipt}, []
    if observation['status'] == 'infrastructure':
        repo = execution.store.setting('repository','jsell-rh/gyre')
        for ident in observation.get('runs',[]):
            if ident is not None:
                execution.command('gh','run','rerun',str(ident),'--failed','--repo',repo,timeout=30,check=False)
    raise Wait('PR CI readiness: ' + observation['status'])
