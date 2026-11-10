"""Fixed platform budget; OOM evidence never authorizes larger workers."""
BUILD_DISK_MIN_GIB=30


def profile(store, task, stage):
    # Historical per-task escalation settings must never bypass this ceiling.
    return {'cpu':'1','memory':'4Gi','memory_gib':4,'build_jobs':1,'build_disk_min_gib':BUILD_DISK_MIN_GIB}


def record_oom(store, task, stage, observed_gib, evidence):
    from .execution import Attention
    store.event('compute_oom',{'stage':stage,'observed_gib':observed_gib,'budget_gib':4,'evidence':evidence},task)
    error=Attention('Sandbox exhausted its fixed 4 GiB budget; inspect retained build evidence and reduce peak memory before retrying')
    error.category='sandbox_oom';error.diagnostic=evidence
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
