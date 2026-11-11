"""Task contracts and stage discovery; no execution side effects."""
import importlib.util
import hashlib
import json
from pathlib import Path
import re
import subprocess


def helper(name):
    path = Path(__file__).resolve().parents[1] / (name + '.py')
    spec = importlib.util.spec_from_file_location(name.replace('-', '_'), path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


contract = helper('dev-contract')


def metadata(body):
    parts = body.split('---', 2)
    if len(parts) != 3:
        raise ValueError('task must have frontmatter')
    front = parts[1]
    def field(key):
        match = re.search(r'^' + key + r':[ \t]*(.*)$', front, re.M)
        return match.group(1).strip().strip('"\'') if match else None
    dep_block = re.search(r'^depends_on:[ \t]*([^\n]*)(?:\n((?:[ \t]+-[^\n]*\n?)*))', front, re.M)
    deps = re.findall(r'task-\d+', ''.join(dep_block.groups(default=''))) if dep_block else None
    return {'title': field('title'), 'progress': field('progress'),
            'dependencies': deps, 'spec_ref': field('spec_ref')}


def safe_metadata(body):
    try:
        return metadata(body)
    except ValueError as exc:
        return {'title': None, 'progress': None, 'dependencies': None,
                'spec_ref': None, 'metadata_error': str(exc)}


def sync_checkout(store, checkout, revision=None):
    checkout = Path(checkout)
    specs = {str(p.relative_to(checkout)): p.read_text()
             for section in ('system', 'development')
             for p in (checkout / 'specs' / section).glob('*.md')}
    goal = (checkout / 'specs/GOAL.md').read_text()
    existing = {task['name']: task for task in store.tasks()}
    from .requirements import snapshot, change, digest
    for path in sorted((checkout / 'specs/tasks').glob('task-*.md')):
        body = path.read_text()
        try:
            gen = contract.generation(body, specs, goal)
        except ValueError:
            gen = hashlib.sha256((body + goal).encode()).hexdigest()
        old = existing.get(path.stem)
        scoped = snapshot(checkout, body, contract, require_registry=bool(
            old and (old['data'].get('requirements') or {}).get('version') == 2))
        if scoped['version'] == 2:
            prior = (old or {}).get('data', {}).get('requirements')
            if old and (not prior or prior.get('version') != 2 or prior['fingerprint'] == scoped['fingerprint']):
                # Fingerprint FORMAT changes do not change an execution epoch.
                data = old['data'] | {'requirements': scoped, 'catalog_generation': gen,
                                      'catalog_source': revision}
                if not prior:
                    data['legacy_catalog_source'] = old['data'].get('catalog_source')
                if data != old['data'] or body != old['body']:
                    store.put_task(path.stem, old['generation'], body, data, expected=old)
                continue
            data = safe_metadata(body) | {'requirements': scoped, 'catalog_generation': gen,
                                         'catalog_source': revision}
            # An epoch identifies a transition, not just content. Reverting a
            # requirement must not resurrect succeeded work from a prior cycle.
            epoch = digest({'requirements':scoped['fingerprint'],'previous':old['generation']}) if old else scoped['fingerprint']
            if old:
                previous = old['data']
                data = previous | data | {'triaged': epoch, 'delivered': None, 'resolved': None,
                    'review': None, 'verified': None, 'repair': None,
                    'previous_delivery': previous.get('delivered') or previous.get('previous_delivery'),
                    'assessment_pending': {'changes': change(prior or {}, scoped),
                                           'previous_generation': old['generation']}}
            elif data['progress'] == 'complete':
                data['delivered'] = {'imported': True, 'generation': epoch, 'source': revision}
            store.put_task(path.stem, epoch, body, data, expected=old)
            continue
        # Operational outcomes survive catalog refresh. A changed contract
        # invalidates receipts; a changed progress label alone does not.
        if old and old['data'].get('catalog_generation') == gen:
            continue
        if old and old['generation'] == gen:
            store.put_task(path.stem, gen, body, old['data'] | {'catalog_generation': gen}, expected=old)
            continue
        data = safe_metadata(body) | {'catalog_generation': gen, 'catalog_source': revision}
        if data['progress'] == 'complete' and not old:
            data['delivered'] = {'imported': True, 'generation': gen, 'source': revision}
        elif old:
            checkpoint = (old['data'].get('candidate') or (old['data'].get('delivered') or {}).get('sha')
                          or old['data'].get('catalog_source'))
            if checkpoint:
                data.update(candidate=checkpoint,
                        candidate_base=old['data'].get('candidate_base') or old['data'].get('catalog_source'),
                        repair={'category': 'requirements_changed', 'source': checkpoint,
                                'id': gen, 'detail': 'Implement the current contract using the retained source checkpoint.'})
        store.put_task(path.stem, gen, body, data, expected=old)


def discover(store, stage, base):
    from .repairs import cached, probe, resolution, satisfied
    from .requirements import pending
    tasks = {task['name']: task for task in store.tasks()}
    graph = {name: task['data'].get('dependencies') or [] for name, task in tasks.items()}
    errors = contract.graph_errors(graph)
    selected = []
    for name, task in tasks.items():
        data = task['data']
        if satisfied(data):
            continue
        requirements = data.get('requirements') or {}
        if requirements.get('errors'):
            # Bad bindings are a triage condition, never a coding assignment.
            if stage == 'triage':
                selected.append(store.enqueue('triage',name,task['generation'],
                    {'base':base,'binding_errors':requirements['errors']},priority=20000))
            continue
        assessment = pending(data)
        if assessment:
            if stage == 'assess':
                selected.append(store.enqueue('assess',name,task['generation'],
                    {'base':base, 'candidate':base if assessment.get('migration') or data.get('previous_delivery') else data.get('candidate'),
                     'requirements':requirements['fingerprint'],
                     'required_evidence':list(requirements.get('components',{})),
                     'changes':assessment.get('changes',{})},priority=20000))
            continue
        if stage == 'observe':
            from .ci import publication
            published = publication(store,task)
            if (published and published.get('candidate') == data.get('candidate') and not data.get('repair')):
                selected.append(store.enqueue('observe',name,task['generation'],
                    {'candidate':data['candidate'],'url':data['pr'],'published':published},priority=30000))
            elif data.get('pr'):
                selected.append(store.enqueue('observe',name,task['generation'],
                    {'url':data['pr'],'priority_only':True},priority=30000))
            continue
        repair = data.get('repair') or {}
        prerequisite = tasks.get(repair.get('prerequisite'))
        reviewed = data.get('review') or {}
        if (repair.get('category') == 'baseline' and prerequisite and satisfied(prerequisite['data'])
                and data.get('candidate') and repair.get('source') == data['candidate']
                and reviewed.get('approved') and reviewed.get('candidate') == data['candidate']):
            # Its independently reviewed source did not fail. Only upstream's
            # prerequisite did; reverify that source instead of asking a model
            # to manufacture an unrelated change to clear a stale finding.
            with store.transaction():
                current = store.task(name)
                if (current['generation'] == task['generation'] and current['data'].get('repair') == repair
                        and not store.db.execute("SELECT 1 FROM work WHERE task=? AND state='claimed'",(name,)).fetchone()):
                    data = current['data'] | {'repair': None, 'verified': None,
                            'baseline_resolution': {'prerequisite':prerequisite['name'],
                                'candidate':data['candidate'],'published_head':data.get('published_head'),'generation':task['generation'],
                                'receipt':prerequisite['data'].get('resolved') or prerequisite['data'].get('delivered')}}
                    store.db.execute('UPDATE tasks SET data=? WHERE name=?',(json.dumps(data),name))
                    store.event('baseline_dependency_resolved', {'prerequisite':prerequisite['name'],
                                'candidate':data['candidate']}, name)
                    task['data'] = data
        if (data.get('repair') or {}).get('shared_environment'):
            continue  # Operational coordination must not rewrite product dependencies.
        baseline_probe = probe(task)
        if baseline_probe and (data.get('baseline_checked') or {}).get('base') != base:
            receipt = cached(store,base,baseline_probe)
            if receipt is not None:
                with store.transaction():
                    current = store.task(name)
                    if (current['generation'] == task['generation'] and
                            not store.db.execute("SELECT 1 FROM work WHERE task=? AND state='claimed'",(name,)).fetchone()):
                        data = current['data'] | resolution(current,receipt)
                        store.db.execute('UPDATE tasks SET data=? WHERE name=?',(json.dumps(data),name))
                        task['data'] = data
                        store.event('baseline_repair_resolved' if receipt['passed'] else 'baseline_reproduced',
                                    {'base':base,'probe':baseline_probe,'receipt':receipt['artifact']},name)
                if satisfied(data):
                    continue
            if stage == 'triage':
                if (data.get('baseline_checked') or {}).get('base') != base:
                    selected.append(store.enqueue('triage', name, task['generation'],
                                {'base': base, 'baseline_probe': baseline_probe}, priority=20000))
            if (data.get('baseline_checked') or {}).get('base') != base:
                continue
        triaged = data.get('triaged') == task['generation']
        candidate = data.get('candidate')
        review = data.get('review') or {}
        verified = data.get('verified') or {}
        repair = data.get('repair')
        deps_ready = name not in errors and all(satisfied(tasks[dep]['data']) for dep in graph[name])
        inputs = None
        if stage == 'triage' and not triaged:
            inputs = {'error': errors.get(name) or data.get('metadata_error'), 'metadata': safe_metadata(task['body'])}
            if (repair or {}).get('category') == 'fresh_start':
                inputs['fresh_start_id'] = repair['id']
            if (inputs['error'] or inputs['metadata'].get('dependencies') is None or
                    not inputs['metadata'].get('title') or not inputs['metadata'].get('spec_ref')):
                inputs['base'] = base
        elif stage == 'implement' and triaged and name not in errors:
            if deps_ready and (not candidate or repair):
                inputs = {'base': base, 'candidate': candidate, 'repair': repair}
        elif stage == 'review' and triaged and deps_ready and candidate and not repair:
            if review.get('candidate') != candidate:
                inputs = {'candidate': candidate, 'base': data['candidate_base']}
                if (data.get('review_retry') or {}).get('candidate')==candidate:
                    inputs['review_retry']=data['review_retry']
        elif stage == 'verify' and triaged and deps_ready and candidate and not repair:
            if review.get('candidate') == candidate and review.get('approved'):
                if verified.get('candidate') != candidate or verified.get('base') != base:
                    from .ci import verification_ready
                    if verification_ready(store,task,base):
                        inputs = {'candidate': candidate, 'base': base}
        elif stage == 'publish' and deps_ready and candidate and not repair:
            if verified.get('candidate') == candidate and (verified.get('base') == base or data.get('pr')):
                inputs = {'candidate': candidate, 'base': base, 'verified': verified}
        if inputs is not None:
            priority = 1000 if repair else 0
            selected.append(store.enqueue(stage, name, task['generation'], inputs, priority))
    with store.transaction():
        placeholders = ','.join('?' for _ in selected)
        exclusion = f' AND id NOT IN ({placeholders})' if selected else ''
        store.db.execute("UPDATE work SET state='obsolete',token=token+1 WHERE stage=? AND state IN ('ready','retry')" + exclusion,
                         (stage, *selected))
    return selected


def generation_for(store, body, task_name=None):
    checkout = store.directory / 'source'
    specs = {str(path.relative_to(checkout)): path.read_text()
             for section in ('system', 'development')
             for path in (checkout / 'specs' / section).glob('*.md')}
    from .requirements import snapshot
    scoped = snapshot(checkout,body,contract, require_registry=bool(task_name and
        (store.task(task_name)['data'].get('requirements') or {}).get('version') == 2))
    if scoped['version'] == 2:
        if task_name:
            task = store.task(task_name)
            if (task['data'].get('requirements') or {}).get('fingerprint') == scoped['fingerprint']:
                return task['generation']
        return scoped['fingerprint']
    return contract.generation(body, specs, (checkout / 'specs/GOAL.md').read_text())
