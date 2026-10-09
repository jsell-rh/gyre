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
    for path in sorted((checkout / 'specs/tasks').glob('task-*.md')):
        body = path.read_text()
        try:
            gen = contract.generation(body, specs, goal)
        except ValueError:
            gen = hashlib.sha256((body + goal).encode()).hexdigest()
        old = existing.get(path.stem)
        # Operational outcomes survive catalog refresh. A changed contract
        # invalidates receipts; a changed progress label alone does not.
        if old and old['data'].get('catalog_generation') == gen:
            continue
        if old and old['generation'] == gen:
            store.put_task(path.stem, gen, body, old['data'] | {'catalog_generation': gen})
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
        store.put_task(path.stem, gen, body, data)


def discover(store, stage, base):
    tasks = {task['name']: task for task in store.tasks()}
    graph = {name: task['data'].get('dependencies') or [] for name, task in tasks.items()}
    errors = contract.graph_errors(graph)
    selected = []
    for name, task in tasks.items():
        data = task['data']
        if data.get('delivered'):
            continue
        triaged = data.get('triaged') == task['generation']
        candidate = data.get('candidate')
        review = data.get('review') or {}
        verified = data.get('verified') or {}
        repair = data.get('repair')
        deps_ready = name not in errors and all(tasks[dep]['data'].get('delivered') for dep in graph[name])
        inputs = None
        if stage == 'triage' and not triaged:
            inputs = {'error': errors.get(name) or data.get('metadata_error'), 'metadata': safe_metadata(task['body'])}
            if (inputs['error'] or inputs['metadata'].get('dependencies') is None or
                    not inputs['metadata'].get('title') or not inputs['metadata'].get('spec_ref')):
                inputs['base'] = base
        elif stage == 'implement' and triaged and name not in errors:
            if deps_ready and (not candidate or repair):
                inputs = {'base': base, 'candidate': candidate, 'repair': repair}
        elif stage == 'review' and triaged and deps_ready and candidate and not repair:
            if review.get('candidate') != candidate:
                inputs = {'candidate': candidate, 'base': data['candidate_base']}
        elif stage == 'verify' and triaged and deps_ready and candidate and not repair:
            if review.get('candidate') == candidate and review.get('approved'):
                if verified.get('candidate') != candidate or verified.get('base') != base:
                    if not (data.get('pr') and verified.get('candidate') == candidate):
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


def generation_for(store, body):
    checkout = store.directory / 'source'
    specs = {str(path.relative_to(checkout)): path.read_text()
             for section in ('system', 'development')
             for path in (checkout / 'specs' / section).glob('*.md')}
    return contract.generation(body, specs, (checkout / 'specs/GOAL.md').read_text())
