"""Authenticated REST bridge to the Gyre platform's task coordination surface.

Dogfooding progression stage 1 (specs/development/dogfooding.md, bullet 3):
the platform product assumes the dev-orchestrator's shared-state and
sequencing responsibilities for Gyre's own development.

- Shared state: catalog sync upserts every specs/tasks/*.md task into the
  platform as an UNTYPED record (no task_type) carrying title, spec_ref,
  priority, order, and depends_on edges resolved through the persisted
  task-NNN to platform-id mapping. Untyped records never reach the
  mechanical scheduler (it only spawns task_type=Implementation workers),
  so the ledger records development workflow state without spawning agents
  and without the approval gate typed implementation tasks carry.
- Lifecycle write-back: implement admission, publication, and structural
  holds transition the platform record; sync reconciles transitions whose
  push failed. A transition is pushed once; an operator resetting a
  non-terminal platform status is respected, not overwritten.
- Authoritative sequencing: new implement-stage admission requires the
  platform to report every depends_on task done, queried over the real
  API. The platform's answer is authoritative over local pipeline state
  (a dependency reported not done blocks admission even when local task
  data satisfies it), and platform unavailability fails closed: a durable
  error event is recorded and no new implement work is admitted, while
  already-claimed in-flight work proceeds.

All HTTP goes through the authenticated REST surface under real ABAC policy
evaluation with the Developer-role service principal provisioned by
scripts/dev-platform.py; there is no direct platform-storage access and no
privileged bypass. The bridge uses no orchestrator-spawn route and no
gyre_spawn_worker: worker dispatch stays with the pipeline's own scheduler
(scheduler-driven task-to-agent mapping is the next documented progression
stage).

The bridge is inactive until scripts/dev-platform.py registers the
platform_endpoint/platform_token/platform_workspace settings in the pipeline
store. An unconfigured checkout keeps running on local state alone; a
CONFIGURED but unreachable platform blocks new implement admission.
"""
import json
import re
import time
import urllib.error
import urllib.request

# The admission gate runs inside the store's claim transaction, so its query
# must stay well inside the SQLite lock timeout other dispatchers wait on.
GATE_TIMEOUT = 5
SYNC_TIMEOUT = 15
PLATFORM_LABEL = 'dev-ledger'
SUPERSEDED_LABEL = 'dev-ledger-superseded'
EVENT_WINDOW = 60
DESCRIPTION = ('Development ledger record synced from specs/tasks/ by the '
               'dev pipeline bridge (dogfooding progression stage 1).')

# The platform's task status machine (gyre-domain Task::transition_status):
# done and cancelled are terminal.
TRANSITIONS = {
    'backlog': ('in_progress', 'cancelled'),
    'in_progress': ('review', 'blocked', 'cancelled'),
    'review': ('done', 'in_progress', 'cancelled'),
    'blocked': ('in_progress', 'cancelled'),
    'done': (),
    'cancelled': (),
}

# Per-process view of the platform's dependency statuses. One real query per
# process invocation: a claim sweep evaluates many work rows against a single
# fetch instead of holding the store transaction open per row. The dispatcher
# and every dev-pipeline command run as fresh processes, so each admission
# cycle observes the platform's current answer.
_INDEX = {}


class Unavailable(Exception):
    """The platform could not be reached or refused the bridge's request."""


def configured(store):
    """Whether dev-platform.py registered this checkout with a platform."""
    return bool(store.setting('platform_endpoint') and store.setting('platform_token')
                and store.setting('platform_workspace'))


def invalidate():
    """Drop the per-process platform view so the next query re-fetches."""
    _INDEX.clear()


def _call(store, method, path, body=None, timeout=GATE_TIMEOUT):
    """One authenticated REST call; network and server failures raise."""
    endpoint = store.setting('platform_endpoint').rstrip('/')
    request = urllib.request.Request(endpoint + path, method=method)
    request.add_header('Authorization', 'Bearer ' + store.setting('platform_token'))
    payload = None
    if body is not None:
        payload = json.dumps(body).encode()
        request.add_header('Content-Type', 'application/json')
    try:
        with urllib.request.urlopen(request, payload, timeout=timeout) as response:
            raw = response.read()
            status = response.status
    except urllib.error.HTTPError as exc:
        if exc.code >= 500:
            raise Unavailable(f'{method} {path}: platform error {exc.code}') from exc
        raw = exc.read()
        status = exc.code
    except (urllib.error.URLError, TimeoutError, OSError) as exc:
        raise Unavailable(f'{method} {path}: {exc}') from exc
    try:
        parsed = json.loads(raw) if raw else None
    except ValueError as exc:
        raise Unavailable(f'{method} {path}: malformed response') from exc
    return status, parsed


def _records(store):
    workspace = store.setting('platform_workspace')
    status, body = _call(store, 'GET', f'/api/v1/workspaces/{workspace}/tasks')
    if status != 200 or not isinstance(body, list):
        raise Unavailable(f'workspace task listing returned {status}')
    return {record['id']: record for record in body
            if isinstance(record, dict) and record.get('id')}


def _note(store, kind, detail, task=None):
    """Record a durable pipeline event, deduplicated to one per window."""
    row = store.db.execute('SELECT max(at) AS at FROM events WHERE kind=?', (kind,)).fetchone()
    if row and row['at'] and time.time() - row['at'] < EVENT_WINDOW:
        return
    store.event(kind, detail, task)


def _mapping(store):
    return store.setting('platform_tasks', {}) or {}


def _save(store, mapping):
    with store.transaction():
        store.db.execute('INSERT INTO settings VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value',
                         ('platform_tasks', json.dumps(mapping)))


def _remember(store, name, **fields):
    mapping = _mapping(store)
    mapping[name] = (mapping.get(name) or {}) | fields
    _save(store, mapping)


def _platform_priority(store, task):
    from .priority import PriorityError, task_priority
    try:
        return {'background': 'low', 'normal': 'medium', 'repair': 'high',
                'high': 'high', 'urgent': 'critical'}.get(task_priority(store, task)['class'], 'medium')
    except PriorityError:
        return 'medium'


def _fields(store, name, task, mapping):
    """The platform-record fields for a task, deps resolved through the mapping."""
    data = task['data']
    deps = [mapping[dep]['id'] for dep in (data.get('dependencies') or [])
            if mapping.get(dep, {}).get('id')]
    return {'title': data.get('title') or name,
            'spec_ref': data.get('spec_ref'),
            'order': int(name.removeprefix('task-')) if re.fullmatch(r'task-\d+', name) else None,
            'priority': _platform_priority(store, task),
            'depends_on': deps}


def _fingerprint(fields):
    """Identity of the create-only contract fields (spec_ref, order, edges)."""
    return json.dumps([fields['spec_ref'], fields['order'], sorted(fields['depends_on'])],
                      sort_keys=True)


def _desired_status(store, task):
    """The platform status the pipeline's real state says the record should hold."""
    from .repairs import satisfied
    data = task['data']
    if satisfied(data):
        return 'done'
    held = store.db.execute('SELECT 1 FROM settings WHERE key LIKE ? LIMIT 1',
                            (f"runtime_hold:{task['name']}:%:{task['generation']}",)).fetchone()
    if held:
        return 'blocked'
    if store.db.execute("SELECT 1 FROM work WHERE task=? AND generation=? AND stage='implement' LIMIT 1",
                        (task['name'], task['generation'])).fetchone() or data.get('candidate'):
        review = data.get('review') or {}
        if review.get('approved') and review.get('candidate') == data.get('candidate'):
            return 'review'
        return 'in_progress'
    return 'backlog'


def _walk(current, target):
    """Shortest legal transition path from current to target, or None."""
    if current == target:
        return []
    seen, queue = {current}, [(current, [])]
    while queue:
        state, path = queue.pop(0)
        for step in TRANSITIONS.get(state, ()):
            if step == target:
                return path + [step]
            if step not in seen:
                seen.add(step)
                queue.append((step, path + [step]))
    return None


def _create(store, name, tasks, mapping, records, summary):
    fields = _fields(store, name, tasks[name], mapping)
    body = {'title': fields['title'], 'description': DESCRIPTION, 'priority': fields['priority'],
            'order': fields['order'], 'depends_on': fields['depends_on'], 'spec_ref': fields['spec_ref'],
            'workspace_id': store.setting('platform_workspace'), 'repo_id': store.setting('platform_repo'),
            'labels': [PLATFORM_LABEL]}
    status, record = _call(store, 'POST', '/api/v1/tasks', body, timeout=SYNC_TIMEOUT)
    if status != 201 or not (record or {}).get('id'):
        raise Unavailable(f'registering {name} returned {status}')
    mapping[name] = {'id': record['id'], 'fingerprint': _fingerprint(fields),
                     'status': record.get('status', 'backlog')}
    records[record['id']] = record
    store.event('platform_task_registered',
                {'id': record['id'], 'order': fields['order'], 'depends_on': fields['depends_on']}, name)
    summary['registered'] += 1
    return record


def _supersede(store, name, entry, summary):
    """Retire a record whose contract drifted or whose work reopened."""
    status, _ = _call(store, 'PUT', f"/api/v1/tasks/{entry['id']}",
                      {'labels': [SUPERSEDED_LABEL]}, timeout=SYNC_TIMEOUT)
    store.event('platform_task_superseded', {'id': entry['id'], 'relabel_status': status}, name)
    summary['superseded'] += 1


def _upsert(store, tasks, mapping, records, summary):
    pending = [name for name, task in tasks.items()
               if not (mapping.get(name, {}).get('id') in records)]
    while pending:
        progressed = []
        for name in pending:
            deps = tasks[name]['data'].get('dependencies') or []
            if all(mapping.get(dep, {}).get('id') for dep in deps):
                _create(store, name, tasks, mapping, records, summary)
                progressed.append(name)
        if not progressed:
            # Remaining tasks reference dependency documents that do not
            # exist; register them with the resolvable subset of edges. The
            # admission gate still fails closed on any unresolved name.
            for name in pending:
                _create(store, name, tasks, mapping, records, summary)
            break
        pending = [name for name in pending if name not in progressed]
    for name, task in tasks.items():
        entry = mapping.get(name)
        if not entry or entry['id'] not in records:
            continue  # just (re-)registered
        record = records[entry['id']]
        fields = _fields(store, name, task, mapping)
        if entry.get('fingerprint') != _fingerprint(fields) or (
                entry.get('status') == 'done' and _desired_status(store, task) != 'done'):
            # Create-only contract fields drifted, or delivered work reopened:
            # a terminal record cannot be reused, so supersede and re-register.
            _supersede(store, name, entry, summary)
            _create(store, name, tasks, mapping, records, summary)
            continue
        updates = {key: fields[key] for key in ('title', 'priority')
                   if record.get(key) != fields[key]}
        if updates:
            status, _ = _call(store, 'PUT', f"/api/v1/tasks/{entry['id']}", updates, timeout=SYNC_TIMEOUT)
            if status != 200:
                raise Unavailable(f'updating {name} returned {status}')
            summary['updated'] += 1


def _reconcile(store, tasks, mapping, records, summary):
    for name, entry in list(mapping.items()):
        task = tasks.get(name)
        if task is None:
            continue  # document removed; the platform record keeps its last state
        record = records.get(entry['id'])
        if record is None:
            continue  # superseded and re-registered during this pass
        desired = _desired_status(store, task)
        if entry.get('status') == desired:
            continue
        path = _walk(record.get('status'), desired)
        if path is None:
            _note(store, 'platform_reconcile_failed',
                  {'task': name, 'actual': record.get('status'), 'desired': desired}, name)
            continue
        for step in path:
            status, _ = _call(store, 'PUT', f"/api/v1/tasks/{entry['id']}/status",
                              {'status': step}, timeout=SYNC_TIMEOUT)
            if status != 200:
                raise Unavailable(f'transitioning {name} to {step} returned {status}')
        entry['status'] = desired
        summary['transitioned'] += 1


def sync(store):
    """Upsert every task into the platform; retry transitions that failed.

    Registration is a mirror, so a failure here never breaks discovery: it
    is recorded as a durable event and the next sync catches up. Admission
    enforcement lives in dependencies_satisfied, which fails closed.
    """
    if not configured(store):
        return None
    summary = {'registered': 0, 'updated': 0, 'superseded': 0, 'transitioned': 0}
    try:
        tasks = {task['name']: task for task in store.tasks()}
        mapping = _mapping(store)
        records = _records(store)
        _upsert(store, tasks, mapping, records, summary)
        _save(store, mapping)
        _reconcile(store, tasks, mapping, records, summary)
        _save(store, mapping)
    except Exception as exc:
        _note(store, 'platform_sync_failed', {'error': str(exc)[-1000:]})
        summary['error'] = str(exc)[-1000:]
    return summary


def _push(store, name, target):
    """Transition one task's platform record toward target (best effort)."""
    if not configured(store):
        return False
    try:
        entry = _mapping(store).get(name)
        if not entry:
            return False  # not registered yet; sync registers and reconciles
        record = _records(store).get(entry['id'])
        if record is None:
            return False  # record missing; sync re-registers and reconciles
        path = _walk(record.get('status'), target)
        if path is None:
            if record.get('status') != target:
                _note(store, 'platform_transition_refused',
                      {'task': name, 'actual': record.get('status'), 'target': target}, name)
            return record.get('status') == target
        for step in path:
            status, _ = _call(store, 'PUT', f"/api/v1/tasks/{entry['id']}/status",
                              {'status': step}, timeout=SYNC_TIMEOUT)
            if status != 200:
                raise Unavailable(f'transitioning {name} to {step} returned {status}')
        _remember(store, name, status=target)
        return True
    except Exception as exc:
        # Write-back is best effort: a failed or buggy push must never
        # break the pipeline stage itself; sync reconciles later.
        _note(store, 'platform_transition_failed',
              {'task': name, 'target': target, 'error': str(exc)}, name)
        return False


def admitted(store, claim):
    """First implement-stage admission sets the platform record in_progress."""
    if claim.get('stage') == 'implement':
        return _push(store, claim['task'], 'in_progress')
    return False


def completed(store, claim):
    """Publication (the progress: complete code path) sets the record done."""
    return _push(store, claim['task'], 'done')


def blocked(store, claim):
    """A structural hold leaving the task held sets the record blocked."""
    return _push(store, claim['task'], 'blocked')


def _dependency_index(store):
    """The platform's current task statuses, fetched once per process."""
    if not configured(store):
        return None
    key = (str(store.directory), store.setting('platform_endpoint'), store.setting('platform_workspace'))
    if key not in _INDEX:
        try:
            records = _records(store)
            _INDEX[key] = ('ok', {id: record.get('status') for id, record in records.items()})
        except Exception as exc:
            # Any failure to obtain the platform's answer - network, server,
            # authorization, or a malformed response - is unavailability:
            # admission fails closed rather than proceeding on local state.
            _INDEX[key] = ('down', str(exc))
    return _INDEX[key]


def dependencies_satisfied(store, dependencies):
    """The platform-authoritative dependency gate for new implement admission.

    True only when the platform reports every dependency done. An
    unconfigured bridge (dev-platform.py never run) is inactive; a configured
    but unreachable or refusing platform fails closed with a durable error
    event, so no new implement work is admitted on local state alone.
    """
    index = _dependency_index(store)
    if index is None:
        return True
    state, value = index
    if state == 'down':
        _note(store, 'platform_unavailable',
              {'error': value, 'effect': 'implement admission fails closed'})
        return False
    mapping = _mapping(store)
    for dep in dependencies or []:
        entry = mapping.get(dep)
        if not entry or value.get(entry.get('id')) != 'done':
            return False
    return True
