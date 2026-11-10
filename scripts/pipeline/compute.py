"""Per-task compute budgets. Escalation requires an observed OOM, never a timeout."""
import json
import re
import subprocess
from decimal import Decimal, InvalidOperation

WORKSPACE_GIB=30


def memory_bytes(value):
    match=re.fullmatch(r'(.+?)(Ki|Mi|Gi|Ti|Pi|Ei|[numkKMGTPE])?',str(value))
    if not match:raise ValueError('invalid Kubernetes memory quantity')
    scales={'Ki':1024,'Mi':1024**2,'Gi':1024**3,'Ti':1024**4,'Pi':1024**5,'Ei':1024**6,
            'n':Decimal('1e-9'),'u':Decimal('1e-6'),'m':Decimal('.001'),
            'k':1000,'K':1000,'M':1000**2,'G':1000**3,'T':1000**4,'P':1000**5,'E':1000**6}
    amount=Decimal(match[1])*scales.get(match[2],1)
    if not amount.is_finite() or amount<0:raise ValueError('invalid Kubernetes memory quantity')
    return amount


def check_admission(execution):
    """Detect deterministic namespace memory denials before creating resources."""
    from .execution import Retry, Wait
    store=execution.store
    context,namespace=store.setting('runtime_kube_context'),store.setting('runtime_namespace')
    if not context or not namespace:return  # Other compute backends have no Kubernetes policy.
    execution.current()
    budget=profile(store,execution.claim['task'],execution.claim['stage'])
    try:
        result=subprocess.run(['kubectl','--context',context,'--request-timeout=8s','-n',namespace,
                               'get','limitranges','-o','json'],capture_output=True,text=True,timeout=10,check=True)
        value=json.loads(result.stdout)
        if value.get('metadata',{}).get('continue'):raise ValueError('incomplete LimitRange inventory')
        constraints=[]
        for item in value['items']:
            for limit in item['spec']['limits']:
                maximum=(limit.get('max') or {}).get('memory')
                if limit['type']=='Container' and maximum is not None:
                    constraints.append((item['metadata']['name'],maximum,memory_bytes(maximum)))
    except (OSError,ValueError,KeyError,TypeError,InvalidOperation,subprocess.SubprocessError) as exc:
        raise Retry('Cannot verify namespace memory policy; no sandbox created; retry scoped Kubernetes access') from exc
    execution.current()
    for name,maximum,amount in constraints:
        if memory_bytes(budget['memory'])>amount:
            error=Wait(f"Namespace {namespace} LimitRange {name} allows at most {maximum} per container; "
                            f"this assignment requires {budget['memory']}. Correct the memory policy; automatic recheck in five minutes; no sandbox created")
            error.category='sandbox_admission'
            error.retry_delay=300
            error.diagnostic={'context':context,'namespace':namespace,'limitrange':name,
                              'requested':budget['memory'],'maximum':maximum,'stage':execution.claim['stage']}
            raise error

def profile(store, task, stage):
    memory = store.setting(f'compute_memory:{task}:{stage}', 4)
    if memory not in (4, 8):
        raise ValueError('unsupported compute memory tier')
    return {'cpu': '1', 'memory': f'{memory}Gi', 'memory_gib': memory, 'build_jobs': 1, 'workspace_gib':WORKSPACE_GIB}


def record_oom(store, task, stage, observed_gib, evidence):
    """Persist a bounded increase, scoped to this task/stage, with its evidence."""
    from .execution import Retry, Attention
    desired = 4 if observed_gib < 4 else 8
    with store.transaction():
        if observed_gib >= 8:
            error = Attention('Sandbox exceeded the maximum 8 GiB tier; inspect OOM evidence instead of repeating the assignment')
        else:
            key = f'compute_memory:{task}:{stage}'
            previous = store.setting(key, 4)
            desired = max(previous, desired)
            store.db.execute('INSERT INTO settings VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value', (key,json.dumps(desired)))
            store.event('compute_memory_escalated', {'stage':stage,'observed_gib':observed_gib,'next_gib':desired,'evidence':evidence},task)
            error = Retry(f'Sandbox OOM at {observed_gib:g} GiB; next {stage} assignment uses {desired} GiB with one build job')
        if observed_gib < 8:
            error.retry_delay = 0  # New resource configuration, not another identical failed attempt.
        error.category = 'sandbox_oom'
        error.diagnostic = evidence
    return error


def from_usage(execution, name, usage):
    """Child-only OOMs need not terminate PID 1 or mark the pod OOMKilled."""
    disk=usage.get('workspace') or {}
    if disk.get('total',0)>0 and (disk.get('free',1024**3)<1024**2 or disk.get('inode_free',1)==0):
        from .execution import Attention
        error=Attention('Sandbox workspace exhausted; retain source/logs and correct the storage/build profile before explicitly retrying')
        error.category='sandbox_storage';error.diagnostic={'sandbox':name,'source':'statvfs',**disk}
        return error
    counters=usage.get('memory.events',{})
    if not isinstance(counters,dict) or int(counters.get('oom_kill',0))<=0:return None
    try:gib=int(usage['memory.max'])/(1024**3)
    except (KeyError,ValueError,TypeError):
        from .execution import Attention
        error=Attention('Cgroup recorded an OOM but its memory limit is unavailable');error.category='sandbox_oom';return error
    return record_oom(execution.store,execution.claim['task'],execution.claim['stage'],gib,
                      {'source':'cgroup','sandbox':name,**usage})
