"""Per-task compute budgets. Escalation requires an observed OOM, never a timeout."""
import json


def profile(store, task, stage):
    memory = store.setting(f'compute_memory:{task}:{stage}', 4)
    if memory not in (4, 8):
        raise ValueError('unsupported compute memory tier')
    return {'cpu': '1', 'memory': f'{memory}Gi', 'memory_gib': memory, 'build_jobs': 1}


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
