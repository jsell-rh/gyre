"""Thin defaults with an operator-bounded retry after authoritative OOM."""
BUILD_DISK_MIN_GIB=30


def memory_ceiling(store):
    value = store.setting('worker_memory_max_gib',6)
    return 6 if type(value) is int and value == 6 else 4


def oom_limit(evidence):
    """Unknown limits and generic process exits do not authorize escalation."""
    try:
        if int((evidence.get('memory.events') or {}).get('oom_kill',0)) > 0:
            return int(evidence['memory.max']) / 1024**3
        if evidence.get('reason') == 'OOMKilled' and evidence.get('exit_code') == 137:
            memory = evidence['resources']['limits']['memory']
            if memory.endswith('Gi'):return float(memory[:-2])
            if memory.endswith('Mi'):return float(memory[:-2]) / 1024
    except (KeyError,TypeError,ValueError):
        pass
    return None


def profile(store, task, stage):
    memory = 4
    if memory_ceiling(store) == 6:
        # Bind historical evidence through its owned allocation and work record,
        # so another task, stage or requirements generation cannot raise memory.
        records = store.db.execute("""SELECT e.detail FROM events e
            JOIN resources r ON r.name=json_extract(e.detail,'$.evidence.sandbox')
            JOIN work w ON w.id=r.work JOIN tasks t ON t.name=w.task AND t.generation=w.generation
            WHERE e.kind='compute_oom' AND e.task=? AND w.task=e.task AND w.stage=?
                AND json_extract(e.detail,'$.stage')=w.stage
                AND r.kind='sandbox' AND json_extract(e.detail,'$.observed_gib')=4""",(task,stage))
        import json
        if any(oom_limit(json.loads(row['detail']).get('evidence') or {}) == 4 for row in records):
            memory = 6
    # Historical ad-hoc 8 GiB settings are never consulted.
    return {'cpu':'1','memory':f'{memory}Gi','memory_gib':memory,'build_jobs':1,'build_disk_min_gib':BUILD_DISK_MIN_GIB}


def record_oom(store, task, stage, observed_gib, evidence):
    from .execution import Attention, Retry
    ceiling = memory_ceiling(store)
    store.event('compute_oom',{'stage':stage,'observed_gib':observed_gib,'budget_gib':ceiling,'evidence':evidence},task)
    allocation = store.db.execute("SELECT json_extract(data,'$.compute.memory_gib') FROM resources WHERE name=?",
                                  (evidence.get('sandbox'),)).fetchone()
    requested = allocation[0] if allocation else None
    if observed_gib == 4 and requested != 6 and oom_limit(evidence) == 4 and profile(store,task,stage)['memory_gib'] == 6:
        error=Retry('Sandbox exhausted 4 GiB; after cleanup retry this task stage at the authorized 6 GiB ceiling')
    else:
        error=Attention(f'Sandbox exhausted its {observed_gib:g} GiB allocation (ceiling {ceiling} GiB); inspect retained evidence before retrying')
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
