"""Independent stage handlers. Outcomes become the next stage's discoveries."""
import json
import hashlib
import os
from pathlib import Path
import re
import shlex
import subprocess
import time

from .catalog import contract, helper, metadata, safe_metadata
from .execution import ROOT, Retry, gateway


def prompt(execution, task):
    role = execution.claim['stage']
    task_path = execution.directory / 'task.md'
    task_path.write_text(task['body'])
    bounded = (task['body'][:16000] if role == 'triage' and safe_metadata(task['body']).get('metadata_error') else
               execution.command('python3', str(ROOT / 'scripts/dev-context.py'), 'task', str(task_path)).stdout)
    findings = [finding['detail'] for finding in execution.store.findings(task['name'])
                if not finding['resolved_by'] and finding['generation'] == task['generation']]
    if task['data'].get('repair') and task['data']['repair'] not in findings:
        findings.append(task['data']['repair'])
    findings = findings[-6:]
    allowance = 18000 // max(1, len(findings))
    findings = [finding | {'log_tail': finding['log_tail'][-allowance:]}
                if finding.get('log_tail') else finding for finding in findings]
    return '\n\n'.join(((ROOT / 'specs/GOAL.md').read_text(),
                        (ROOT / f'specs/prompts/pipeline-{role}.md').read_text(),
                        'Assigned task:\n' + bounded,
                        'Exact assignment:\n' + json.dumps(execution.claim['input']),
                        'Previous assignment outcome:\n' + str(execution.claim.get('result') or 'none')[-4000:],
                        'Durable findings:\n' + json.dumps(findings)))


def triage(execution, task):
    store = execution.store
    data = task['data']
    graph = {item['name']: item['data'].get('dependencies') or [] for item in store.tasks()}
    error = contract.graph_errors(graph).get(task['name']) or data.get('metadata_error')
    complete = data.get('dependencies') is not None and data.get('title') and data.get('spec_ref')
    if complete and not error:
        return {'metadata': 'valid'}, {'triaged': task['generation']}, []
    # Semantic changes require a model; validate its dependency graph rather
    # than trusting its claim that the task is now workable.
    result = execution.cloud_step(task, prompt(execution, task))
    try:
        new = metadata(result['task_body'])
        if not data.get('metadata_error'):
            if contract.requirement_parts(task['body'])[1] != contract.requirement_parts(result['task_body'])[1]:
                raise ValueError('triage changed normative task behavior')
            if any(data.get(key) and data[key] != new[key] for key in ('title', 'spec_ref')):
                raise ValueError('triage changed an existing title or spec binding')
    except ValueError as exc:
        raise Retry(str(exc), fresh_model=True) from exc
    graph[task['name']] = new.get('dependencies') or []
    error = contract.graph_errors(graph).get(task['name'])
    if not result['valid'] or error or new['dependencies'] is None or not new['title'] or not new['spec_ref']:
        raise Retry(error or 'triage did not produce valid task metadata', fresh_model=True)
    return result, new | {'triaged': task['generation'], 'task_override': result['task_body'], 'metadata_error': None}, []


def implement(execution, task):
    result = execution.cloud_step(task, prompt(execution, task))
    # A partial source checkpoint survives inference failures. It is never
    # sent to review as though the agent completed its assignment.
    progress = safe_metadata(result['task_body'])['progress']
    complete = result['valid'] and progress in ('ready-for-review', 'complete')
    try:
        contract_changed = contract.requirement_parts(task['body']) != contract.requirement_parts(result['task_body'])
    except ValueError:
        contract_changed = True
    if contract_changed:
        complete = False
        # An implementer cannot approve its own weaker desired requirements.
        # Retain its source for repair while keeping the assigned contract.
        result['task_body'] = task['body']
    repair = task['data'].get('repair') or {}
    requires_fix = (repair.get('category') in ('review', 'verification', 'ci', 'contract') and
                    execution.claim['input']['base'] == task['data'].get('candidate_base'))
    if not contract_changed and (not complete or requires_fix):
        path = checkout(execution, result['head'])
        seed = execution.claim['input'].get('candidate') or execution.claim['input']['base']
        execution.command('git', 'fetch', '--quiet', 'origin', seed, cwd=path, timeout=180)
        changed = execution.command('git', 'diff', '--name-only', seed, result['head'], cwd=path).stdout.splitlines()
        substantive = [name for name in changed if name != f"specs/tasks/{task['name']}.md"
                       and not name.startswith(('web/dist/', 'specs/reviews/'))]
        if not substantive:
            raise Retry('Implementation did not change source relevant to its unfinished assignment or repair. Resume the same task and address its findings; changing progress or notes alone does not resolve them.', fresh_model=True)
    updates = {'candidate': result['head'], 'candidate_base': result.get('base', execution.claim['input']['base']),
               'branch': result['branch'], 'task_override': result['task_body'],
               'review': None, 'verified': None,
               'repair': None if complete else {'category': 'implementation',
                                                'source': result['head'], 'checkpoint': True,
                                                'id': execution.claim['id'],
                                                'agent_exit': result['agent_exit'],
                                                'detail': 'Continue from this saved source checkpoint. The previous assignment did not complete; do not repeat broad repository exploration or full integration gates.',
                                                'log_tail': execution.log.read_text(errors='replace')[-12000:]}}
    if contract_changed:
        updates['repair'].update(category='contract', detail='The implementation changed the assigned requirements. Restore the original task contract and implement it; normative changes require separate spec review.')
    return result, updates, []


def review(execution, task):
    result = execution.cloud_step(task, prompt(execution, task))
    if result.get('agent_exit') != 0 or result.get('stream_exit') != 0:
        raise Retry('review model did not complete; retry independent review on the same candidate')
    path = checkout(execution, result['head'])
    assigned = execution.claim['input']['candidate']
    execution.command('git', 'fetch', '--quiet', 'origin', assigned, cwd=path, timeout=180)
    changes = execution.command('git', 'diff', '--name-only', assigned, result['head'], cwd=path).stdout.splitlines()
    invalid = [name for name in changes if not (
        name == f"specs/tasks/{task['name']}.md" or name.startswith('web/dist/') or
        re.fullmatch(r'specs/reviews/(?:audit-)?' + re.escape(task['name']) + r'(?:[.-].*)?', name))]
    before = execution.command('git', 'show', assigned + ':specs/tasks/' + task['name'] + '.md', cwd=path).stdout
    after = execution.command('git', 'show', result['head'] + ':specs/tasks/' + task['name'] + '.md', cwd=path).stdout
    if contract.requirement_parts(before) != contract.requirement_parts(after):
        invalid.append('review changed the task contract')
    if invalid:
        result['valid'] = False
        result['source_edits'] = invalid
    verdict = result.get('verdict') or {}
    approved = (result['valid'] and verdict.get('approved') is True and
                verdict.get('candidate') == execution.claim['input']['candidate'] and
                isinstance(verdict.get('findings'), list) and not verdict['findings'])
    receipt = {'candidate': execution.claim['input']['candidate'], 'approved': approved,
               'checkpoint': result['head'], 'branch': result['branch'],
               'record': next((name for name in changes if name.startswith('specs/reviews/')), None),
               'artifact': str(execution.directory / 'outcome.json')}
    if approved:
        return result, {'review': receipt, 'retained_artifacts': None}, []
    finding = {'category': 'review', 'source': execution.claim['input']['candidate'],
               'review_checkpoint': result['head'], 'branch': result['branch'],
               'details': result.get('source_edits') or verdict.get('findings') or ['review did not produce an affirmative exact-source verdict'],
               'artifact': receipt['artifact'], 'id': execution.claim['id']}
    return result, {'review': receipt, 'repair': finding}, [finding]


def checkout(execution, source):
    path = execution.directory / 'checkout'
    if not (path / '.git').exists():
        execution.command('git', 'clone', '--quiet', '--no-checkout', '--shared',
                          str(ROOT), str(path), timeout=120)
        execution.command('git', 'remote', 'set-url', 'origin',
                          execution.store.setting('repo_url', 'https://github.com/jsell-rh/gyre.git'), cwd=path)
    # A shared clone of a partial repository must preserve promisor semantics.
    # Otherwise Git advertises ancestors whose omitted blobs it cannot resolve.
    execution.command('git', 'config', 'remote.origin.promisor', 'true', cwd=path)
    execution.command('git', 'config', 'remote.origin.partialclonefilter', 'blob:none', cwd=path)
    fetch = execution.command('git', 'fetch', '--quiet', '--filter=blob:none', 'origin', source,
                              timeout=180, cwd=path, check=False)
    if fetch.returncode:
        # Refetch avoids negotiating against an earlier incomplete pack. Keep
        # the same checkout, logs and work identity throughout recovery.
        execution.command('git', 'fetch', '--quiet', '--filter=blob:none', '--refetch',
                          'origin', source, timeout=180, cwd=path)
    execution.command('git', 'checkout', '--detach', source, cwd=path)
    return path


def verify(execution, task):
    inp = execution.claim['input']
    host_resource = 'host-' + execution.claim['id'] + '-' + str(execution.claim['token'])
    if not execution.store.reserve(host_resource, execution.claim['id'], execution.claim['token'], 'host', 1):
        raise Retry('host verification capacity is occupied')
    try:
        execution.phase('Verifying')
        path = checkout(execution, inp['base'])
        execution.command('git', 'fetch', '--quiet', 'origin', inp['candidate'], cwd=path, timeout=180)
        execution.command('git', 'config', 'user.name', 'gyre-pipeline', cwd=path)
        execution.command('git', 'config', 'user.email', 'jsell-rh@users.noreply.github.com', cwd=path)
        execution.command('git', 'checkout', '--detach', inp['candidate'], cwd=path)
        task_file = path / 'specs/tasks' / (task['name'] + '.md')
        body, count = re.subn(r'(?m)^progress:[^\n]*$', 'progress: complete', task_file.read_text(), count=1)
        if count != 1:
            raise ValueError('candidate task has no unique progress field')
        task_file.write_text(body)
        execution.command('git', 'add', '--', str(task_file), cwd=path)
        changed = execution.command('git', 'diff', '--cached', '--quiet', cwd=path, check=False)
        if changed.returncode:
            execution.command('git', '-c', 'core.hooksPath=/dev/null', 'commit', '-m',
                              f"process({task['name']}): record independent approval", cwd=path)
        approved_candidate = execution.command('git', 'rev-parse', 'HEAD', cwd=path).stdout.strip()
        execution.command('git', 'checkout', '--detach', inp['base'], cwd=path)
        merge = execution.command('git', '-c', 'core.hooksPath=/dev/null', 'merge', '--no-ff',
                                  '-m', f"verify({task['name']}): reconcile candidate with main",
                                  approved_candidate, cwd=path, check=False)
        if merge.returncode:
            finding = {'id': execution.claim['id'], 'category': 'rebase', 'source': inp['candidate'],
                       'base': inp['base'], 'details': merge.stdout + merge.stderr}
            return {'passed': False}, {'repair': finding}, [finding]
        head = execution.command('git', 'rev-parse', 'HEAD', cwd=path).stdout.strip()
        tree = execution.command('git', 'rev-parse', 'HEAD^{tree}', cwd=path).stdout.strip()
        tools = execution.directory / 'tools'
        tools.mkdir(exist_ok=True)
        for name in ('dev-static-gate.py', 'check-rustfmt-diff.py', 'check-clippy-diff.py'):
            (tools / name).write_bytes((ROOT / 'scripts' / name).read_bytes())
        # Reuse the mechanically maintained checks, with explicit absolute
        # locations. Both upstream and candidate verifiers are executed.
        gate = (ROOT / 'scripts/dev-check.sh').read_text().replace('/tmp/gyre', shlex.quote(str(path))).replace('/tmp/stage', shlex.quote(str(tools)))
        gate_path = tools / 'checks.sh'
        gate_path.write_text(gate)
        env = {**os.environ, 'SKIP_WEB_BUILD': '1', 'CARGO_BUILD_JOBS': '2',
               'CARGO_TARGET_DIR': str(execution.store.directory / 'host-target')}
        commands = [(['python3', str(ROOT / 'scripts/dev-cargo-clean.py')], path, 180),
                    (['bash', str(gate_path)], path, 3600),
                    (['cargo', 'test', '--all', '--quiet'], path, 3600),
                    (['npm', 'test', '--', '--no-file-parallelism'], path / 'web', 1800)]
        records = []
        with execution.log.open('ab', buffering=0) as log:
            for index, (command, cwd, timeout) in enumerate(commands):
                execution.current()
                log.write(('\n$ ' + ' '.join(command) + '\n').encode())
                result_path = execution.directory / f'host-step-{index}.exit'
                execution.store.resource_state(host_resource, 'present', {'identity': str(result_path) + '.process.json'})
                child = subprocess.Popen(['bash', str(ROOT / 'scripts/dev-process.sh'), str(result_path), *command],
                                         cwd=cwd, env=env, stdout=log, stderr=subprocess.STDOUT,
                                         start_new_session=True)
                try:
                    code = child.wait(timeout=timeout)
                except subprocess.TimeoutExpired as exc:
                    import signal
                    os.killpg(child.pid, signal.SIGKILL)
                    child.wait()
                    raise Retry('verification timed out; inspect infrastructure') from exc
                finally:
                    if child.poll() is None:
                        import signal
                        os.killpg(child.pid, signal.SIGKILL)
                        child.wait(timeout=30)
                records.append({'command': command, 'exit_code': code})
                if code:
                    if code in (75, 77, 79, 124, 137):
                        raise Retry('verification infrastructure unavailable')
                    if code == 81:
                        observation = {'status': 'baseline_failed', 'base': inp['base'],
                                       'environment': 'host-' + hashlib.sha256(json.dumps(command).encode()).hexdigest(),
                                       'baseline_log': str(execution.log)}
                        prerequisite = baseline_repair(execution, observation)
                        finding = observation | {'id': execution.claim['id'], 'category': 'baseline',
                                                 'source': inp['candidate'], 'prerequisite': prerequisite,
                                                 'log_tail': execution.log.read_text(errors='replace')[-16000:]}
                        return {'passed': False, 'checks': records}, {
                            'repair': finding, 'dependencies': sorted(set(task['data']['dependencies']) | {prerequisite})}, [finding]
                    finding = {'id': execution.claim['id'], 'category': 'verification',
                               'source': inp['candidate'], 'base': inp['base'],
                               'command': command, 'exit_code': code,
                               'artifact': str(execution.log),
                               'log_tail': execution.log.read_text(errors='replace')[-16000:]}
                    return {'passed': False, 'checks': records}, {'repair': finding}, [finding]
        dirty = execution.command('git', 'status', '--porcelain', cwd=path).stdout.splitlines()
        if any('web/dist/' not in line for line in dirty):
            raise RuntimeError('verification changed the source tree')
        branch = f"pipeline/verified/{execution.claim['id']}-{execution.claim['token']}"
        execution.command('git', 'push', 'origin', head + ':refs/heads/' + branch, cwd=path, timeout=180)
        receipt = {'candidate': inp['candidate'], 'base': inp['base'], 'head': head,
                   'tree': tree, 'branch': branch, 'checks': records, 'passed': True}
        (execution.directory / 'verified.json').write_text(json.dumps(receipt))
        return receipt, {'verified': receipt}, []
    finally:
        execution.store.resource_state(host_resource, 'absent')


def publish(execution, task):
    verified = execution.claim['input']['verified']
    head = verified['head']
    repo = execution.store.setting('repository', 'jsell-rh/gyre')
    data = task['data']
    branch = data.get('delivery_branch') or 'pipeline/delivery/' + task['name']
    url = data.get('pr')
    if url:
        observation = json.loads(execution.command('gh', 'pr', 'view', url, '--json',
                                   'state,headRefName,headRefOid,mergeCommit', timeout=30).stdout)
        if observation['state'] == 'OPEN' and not data.get('delivery_branch'):
            branch = observation['headRefName']
        if observation['state'] == 'CLOSED':
            raise Retry('The associated PR was closed. Await an operator decision before publishing or changing code.')
        if observation['state'] == 'MERGED':
            if observation['headRefOid'] != head:
                raise RuntimeError('merged PR does not match verified source')
            sha = observation['mergeCommit']['oid']
            path = checkout(execution, sha)
            tree = execution.command('git', 'rev-parse', 'HEAD^{tree}', cwd=path).stdout.strip()
            if tree != verified['tree']:
                raise RuntimeError('upstream merge tree differs from verified tree')
            for resource in execution.store.db.execute("SELECT name FROM resources WHERE work=? AND kind='merge'", (execution.claim['id'],)).fetchall():
                execution.store.resource_state(resource['name'], 'absent')
            return {'merged': sha, 'pr': url}, {'progress': 'complete', 'delivered': {'sha': sha, 'pr': url,
                     'candidate': head, 'tree': tree, 'generation': task['generation']}}, []
    path = checkout(execution, head)
    shipped = helper('dev-merge-message').message(task['name'],
                                                path / 'specs/tasks' / (task['name'] + '.md'),
                                                verified['candidate'])
    description = execution.directory / 'pr.md'
    description.write_text(shipped + f"\nVerified base: {verified['base']}\nVerified tree: {verified['tree']}\n\n"
                           "Independent review approved the exact candidate. Deterministic guards, full Rust tests, and frontend tests passed. GitHub checks must pass before merge.\n")
    review_receipt = data.get('review') or {}
    if review_receipt.get('checkpoint') and review_receipt.get('record'):
        with description.open('a') as output:
            output.write(f"\nReview record: https://github.com/{repo}/blob/{review_receipt['checkpoint']}/{review_receipt['record']}\n")
    # Stable delivery branch, guarded against unexpected external updates.
    remote = execution.command('git', 'ls-remote', '--heads', 'origin', branch, cwd=path).stdout.split()
    expected = data.get('published_head')
    actual = remote[0] if remote else ''
    if actual != head:
        if actual != (expected or ''):
            raise RuntimeError('delivery branch changed outside recorded publication')
        execution.command('git', 'push', f'--force-with-lease=refs/heads/{branch}:{actual}',
                          'origin', f'{head}:refs/heads/{branch}', cwd=path, timeout=180)
    existing = json.loads(execution.command('gh', 'pr', 'list', '--repo', repo, '--head', branch,
                                            '--state', 'open', '--json', 'url', timeout=30).stdout)
    if existing:
        url = existing[0]['url']
    elif not url:
        url = execution.command('gh', 'pr', 'create', '--repo', repo, '--head', branch, '--base', 'main',
                                '--title', f"feat({task['name']}): {data['title']}",
                                '--body-file', str(description), timeout=60).stdout.strip()
    body_hash = hashlib.sha256(description.read_bytes()).hexdigest()
    if data.get('pr_body_hash') != body_hash:
        execution.command('gh', 'pr', 'edit', url, '--body-file', str(description), timeout=60)
    # Persist the external identity before polling. Discovery replay adopts it.
    with execution.store.transaction():
        execution.store._current(execution.claim['id'], execution.claim['token'])
        current = execution.store.task(task['name'])['data']
        current.update(pr=url, published_head=head, delivery_branch=branch, pr_body_hash=body_hash)
        execution.store.db.execute('UPDATE tasks SET data=? WHERE name=?', (json.dumps(current), task['name']))
        execution.store.event('pull_request', {'url': url, 'head': head}, task['name'], execution.claim['id'])
    latest = execution.command('git', 'ls-remote', 'origin', 'refs/heads/main', cwd=path).stdout.split()[0]
    if latest != verified['base']:
        return {'base_changed': latest}, {'verified': None}, []
    observation = helper('dev-ci').observe(execution.command, url, head, verified['base'], execution.directory)
    if observation['status'] == 'infrastructure':
        for ident in observation.get('runs', []):
            if ident is not None:
                execution.command('gh', 'run', 'rerun', str(ident), '--failed', '--repo', repo,
                                  timeout=30, check=False)
        raise Retry('GitHub infrastructure checks were rerun on the same verified head')
    if observation['status'] == 'baseline_failed':
        prerequisite = baseline_repair(execution, observation)
        finding = observation | {'id': execution.claim['id'], 'category': 'baseline',
                                 'source': head, 'prerequisite': prerequisite,
                                 'log_tail': Path(observation['log']).read_text()[-16000:]}
        return observation, {'dependencies': sorted(set(task['data']['dependencies']) | {prerequisite}),
                             'repair': finding}, [finding]
    if observation['status'] in ('candidate_failed', 'baseline_failed', 'closed'):
        finding = observation | {'id': execution.claim['id'], 'category': 'ci', 'source': head}
        if observation.get('log'):
            finding['log_tail'] = Path(observation.get('candidate_log', observation['log'])).read_text()[-16000:]
        return observation, {'repair': finding, 'candidate': head,
                             'candidate_base': verified['base'], 'review': None, 'verified': None}, [finding]
    if observation['status'] != 'passed':
        raise Retry('GitHub merge gate: ' + observation['status'])
    permit = 'merge-' + execution.claim['id']
    if not execution.store.reserve(permit, execution.claim['id'], execution.claim['token'], 'merge', 1,
                                   {'url': url, 'head': head, 'base': verified['base']}):
        raise Retry('another upstream merge is awaiting confirmation')
    latest = execution.command('git', 'ls-remote', 'origin', 'refs/heads/main', cwd=path).stdout.split()[0]
    if latest != verified['base']:
        execution.store.resource_state(permit, 'absent')
        return {'base_changed': latest}, {'verified': None}, []
    execution.command('gh', 'pr', 'merge', url, '--merge', '--match-head-commit', head,
                      '--subject', f"feat({task['name']}): {data['title']}",
                      '--body-file', str(description), timeout=60)
    # The next observation confirms the authoritative merge and its tree.
    raise Retry('merge requested; awaiting upstream confirmation')


def baseline_repair(execution, observation):
    """Deduplicate a real upstream failure into a scoped prerequisite task."""
    from .catalog import generation_for
    store = execution.store
    key = observation['base'] + ':' + observation['environment']
    excerpt = Path(observation['baseline_log']).read_text()[-24000:]
    body = ('---\ntitle: "Repair verified failure on main ' + observation['base'][:12] + '"\n'
            'spec_ref: "GOAL.md — real implementations and meaningful verification"\n'
            'depends_on: []\nprogress: not-started\ncommits: []\n---\n\n'
            '## Required behavior\n\nReproduce and repair this verified upstream failure. '
            'Implement real production fixes or correct a genuinely broken test setup. '
            'Do not weaken checks, add skips or exemptions, or implement the blocked feature. '
            'Obtain independent review and pass full verification and GitHub checks.\n\n'
            'Base: `' + observation['base'] + '`\n'
            'Environment fingerprint: `' + observation['environment'] + '`\n\n'
            '## Baseline failure\n\n```text\n' + excerpt + '\n```\n')
    generation = generation_for(store, body)
    with store.transaction():
        for task in store.tasks():
            if task['data'].get('baseline_key') == key:
                return task['name']
        number = max((int(task['name'][5:]) for task in store.tasks()), default=0) + 1
        name = f'task-{number:03d}'
        data = metadata(body) | {'baseline_key': key, 'catalog_generation': generation}
        store.db.execute('INSERT INTO tasks VALUES(?,?,?,?,?)', (name, generation, body, json.dumps(data), time.time()))
        store.event('baseline_prerequisite', {'base': observation['base'], 'evidence': observation['baseline_log']}, name)
        return name


def cleanup(execution, task):
    resource = execution.claim['input']['resource']
    record = execution.store.db.execute('SELECT * FROM resources WHERE name=?', (resource,)).fetchone()
    if record['kind'] == 'host':
        identity = json.loads(record['data']).get('identity')
        if identity and Path(identity).exists():
            value = json.loads(Path(identity).read_text())
            if value['boot'] == Path('/proc/sys/kernel/random/boot_id').read_text().strip():
                import signal
                stat = Path(f"/proc/{int(value['pid'])}/stat")
                if stat.exists() and stat.read_text().rsplit(')', 1)[1].split()[19] != value['start']:
                    raise ValueError('host process identity changed; refusing to stop it')
                try:
                    os.killpg(int(value['pid']), signal.SIGKILL)
                except ProcessLookupError:
                    pass
        execution.store.resource_state(resource, 'absent')
        return {'released': resource}, {}, []
    if record['kind'] == 'merge':
        data = json.loads(record['data'])
        observation = json.loads(execution.command('gh', 'pr', 'view', data['url'], '--json',
                                                    'state,headRefOid', timeout=30).stdout)
        if observation['state'] not in ('MERGED', 'CLOSED'):
            raise Retry('merge outcome is unresolved; holding the global merge permit')
        execution.store.resource_state(resource, 'absent')
        return {'merge_observed': observation}, {}, []
    execution.login()
    inventory = gateway.inventory()
    item = next((item for item in inventory if item['name'] == resource), None)
    if item and item['labels'].get('gyre.dev/pipeline') != execution.store.setting('owner'):
        raise ValueError('cleanup resource belongs to another actor')
    updates = {}
    owned = record
    old_work = execution.store.db.execute('SELECT * FROM work WHERE id=?', (owned['work'],)).fetchone()
    old_directory = execution.store.directory / 'attempts' / owned['work'] / str(owned['token'])
    old_directory.mkdir(parents=True, exist_ok=True)
    if any(item['name'] == resource for item in inventory):
        # Crash cleanup must preserve source too. Stop the old process group
        # before capturing its index; its expired token cannot approve work.
        code = """import json,os,pathlib,signal,time
p=pathlib.Path('/tmp/stage/step.exit.process.json')
if p.exists():
 value=json.loads(p.read_text()); pid=int(value['pid']); stat=pathlib.Path(f'/proc/{pid}/stat')
 if stat.exists() and stat.read_text().rsplit(')',1)[1].split()[19]==value['start'] and pathlib.Path('/proc/sys/kernel/random/boot_id').read_text().strip()==value['boot']:
  os.killpg(pid,signal.SIGTERM); time.sleep(1)
  if stat.exists(): os.killpg(pid,signal.SIGKILL)
"""
        try:
            execution.remote(resource, 'python3', '-c', code, timeout=30, check=False)
            raw = execution.remote(resource, 'tail', '-c', '1048576', '/tmp/stage/step.log', timeout=30, check=False)
            (old_directory / 'orphan.log').write_text(raw.stdout)
            try:
                execution.command('python3', str(ROOT / 'scripts/dev-recover.py'), resource,
                                  str(old_directory / 'recovery.patch'), timeout=180, check=False)
            except Exception as exc:
                execution.store.event('orphan_capture_unavailable', {'resource': resource, 'error': str(exc)}, task['name'])
        except Exception as exc:
            execution.store.event('orphan_capture_unavailable', {'resource': resource, 'error': str(exc)}, task['name'])
        execution.store.resource_state(resource, 'deleting')
        result = execution.os('sandbox', 'delete', resource, timeout=180, check=False)
        if result.returncode:
            raise Retry('gateway deletion unavailable')
        if any(item['name'] == resource for item in gateway.inventory()):
            raise Retry('sandbox deletion still pending')
        execution.store.resource_state(resource, 'absent')
    receipt = old_directory / 'recovery.json'
    if (receipt.exists() and old_work and old_work['stage'] == 'implement'
            and old_work['state'] != 'succeeded' and old_work['generation'] == task['generation']
            and not task['data'].get('delivered')):
        old_input = json.loads(old_work['input'])
        if task['data'].get('candidate') == old_input.get('candidate'):
            from .recovery import restore
            # Cleanup has the task claim, so another implementation cannot
            # race this recovery. The Git lease separately fences pushes.
            repository = checkout(execution, json.loads(receipt.read_text())['base'])
            outcome = restore(old_directory, repository,
                              f"pipeline/{old_work['task']}/{old_work['id']}-{json.loads(owned['data']).get('logical_token', owned['token'])}")
            body = execution.command('git', 'show', outcome['head'] + ':specs/tasks/' + task['name'] + '.md', cwd=repository).stdout
            updates = {'candidate': outcome['head'], 'candidate_base': json.loads(receipt.read_text())['base'],
                       'task_override': body, 'review': None, 'verified': None,
                       'repair': {'category': 'checkpoint', 'source': outcome['head'],
                                  'id': execution.claim['id'], 'detail': 'Resume source recovered after worker lease expiry.'}}
    if any(item['name'] == resource for item in gateway.inventory()):
        raise Retry('sandbox deletion still pending')
    execution.store.resource_state(resource, 'absent')
    return {'deleted': resource}, updates, []


HANDLERS = {'triage': triage, 'implement': implement, 'review': review,
            'verify': verify, 'publish': publish, 'cleanup': cleanup}
