"""Independent stage handlers. Outcomes become the next stage's discoveries."""
import json
import difflib
import hashlib
import os
from pathlib import Path
import re
import time

from .catalog import contract, helper, metadata, safe_metadata
from .execution import ROOT, Retry, Wait, gateway


def prompt(execution, task):
    role = execution.claim['stage']
    task_path = execution.directory / 'task.md'
    task_path.write_text(task['body'])
    bounded = (task['body'][:16000] if role == 'triage' and safe_metadata(task['body']).get('metadata_error') else
               execution.command('python3', str(ROOT / 'scripts/dev-context.py'), 'task', str(task_path)).stdout)
    findings = [finding['detail'] for finding in execution.store.findings(task['name'])
                if not finding['resolved_by'] and finding['generation'] == task['generation']
                and (finding['category'] != 'baseline' or finding['detail'].get('base') == execution.claim['input'].get('base'))]
    context = task['data'].get('repair') or (task['data'].get('review_context') if role == 'review' else None)
    if context and context not in findings:
        findings.append(context)
    findings = findings[-6:]
    allowance = 18000 // max(1, len(findings))
    findings = [finding | {'log_tail': finding['log_tail'][-allowance:]}
                if finding.get('log_tail') else finding for finding in findings]
    return '\n\n'.join(((ROOT / 'specs/GOAL.md').read_text(),
                        (ROOT / f'specs/prompts/pipeline-{role}.md').read_text(),
                        'Assigned task:\n' + bounded,
                        'Exact assignment:\n' + json.dumps(execution.claim['input']),
                        'Current requirement inputs:\n' + json.dumps((task['data'].get('requirements') or {}).get('components', {})),
                        'Previous assignment outcome:\n' + str(execution.claim.get('result') or 'none')[-4000:],
                        'Durable findings:\n' + json.dumps(findings)))


def triage(execution, task):
    if execution.claim['input'].get('baseline_probe'):
        from .repairs import recheck
        return recheck(execution, task)
    store = execution.store
    data = task['data']
    if execution.claim['input'].get('binding_errors'):
        from .execution import Attention
        error = Attention('Spec input binding needs triage in specs/task-spec-inputs.json: ' + '; '.join(execution.claim['input']['binding_errors']))
        error.category = 'requirements_binding'
        raise error
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
    contract_delta = ''
    try:
        assigned = contract.requirement_parts(task['body'])
        observed = contract.requirement_parts(result['task_body'])
        contract_changed = assigned != observed
        if contract_changed:
            contract_delta = ''.join(difflib.unified_diff(
                ('\n'.join(assigned)).splitlines(True), ('\n'.join(observed)).splitlines(True),
                fromfile='assigned contract', tofile='checkpoint contract'))[:6000]
    except ValueError:
        contract_changed = True
        contract_delta = 'The checkpoint task document could not be parsed.'
    if contract_changed:
        complete = False
        # An implementer cannot approve its own weaker desired requirements.
        # Retain its source for repair while keeping the assigned contract.
        result['task_body'] = task['body']
    repair = task['data'].get('repair') or {}
    # Contract equality is enforced above. A document-only restoration must
    # not require unrelated production edits merely to clear its finding.
    requires_fix = (repair.get('category') in ('review', 'verification', 'ci') and
                    execution.claim['input']['base'] == task['data'].get('candidate_base'))
    if not complete or requires_fix:
        path = checkout(execution, result['head'])
        seed = execution.claim['input'].get('candidate') or execution.claim['input']['base']
        execution.command('git', 'fetch', '--quiet', 'origin', seed, cwd=path, timeout=180)
        changed = execution.command('git', 'diff', '--name-only', seed, result['head'], cwd=path).stdout.splitlines()
        substantive = [name for name in changed if name != f"specs/tasks/{task['name']}.md"
                       and not name.startswith(('web/dist/', 'specs/reviews/'))]
        if not substantive:
            from .repair_budget import check
            check(execution,task,path,result['head'],{'category':'contract','delta':contract_delta} if contract_changed else
                  {'category':'no_source_progress','repair':repair.get('details') or repair.get('detail')})
            if not contract_changed:raise Retry('Implementation did not change source relevant to its unfinished assignment or repair. Resume the same task and address its findings; changing progress or notes alone does not resolve them.', fresh_model=True)
    updates = {'candidate': result['head'], 'candidate_base': result.get('base', execution.claim['input']['base']),
               'branch': result['branch'], 'task_override': result['task_body'],
               'review': None, 'verified': None,
               'review_context': repair if complete else None,
               'repair': None if complete else {'category': 'implementation',
                                                'source': result['head'], 'checkpoint': True,
                                                'id': execution.claim['id'],
                                                'agent_exit': result['agent_exit'],
                                                'detail': 'Continue from this saved source checkpoint. The previous assignment did not complete; do not repeat broad repository exploration or full integration gates.',
                                                'log_tail': execution.log_tail(12000)}}
    if contract_changed:
        updates['repair'].update(category='contract',
            detail='The protected task text changed. The executor restores the assigned task document in the next checkout; preserve the existing production fix. Keep all progress notes under exactly ## Shipped, using ### for round subheadings. Do not rename that heading or add diagnostic top-level sections. Normative changes require separate spec review.',
            log_tail=contract_delta)
    return result, updates, []


def review(execution, task):
    result = execution.cloud_step(task, prompt(execution, task))
    if result.get('agent_exit') != 0 or result.get('stream_exit') != 0:
        from .diagnostics import diagnose
        diagnostic = result.get('diagnostic') or diagnose(execution.directory, result.get('agent_exit'))
        error = Retry(diagnostic['message'] + '; resume saved review evidence on the same candidate')
        error.category, error.diagnostic = diagnostic['category'], diagnostic
        raise error
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
    from .repair_budget import source_findings
    findings,deferred=source_findings(verdict.get('findings'))
    # Generated assets have a mandatory deterministic rebuild/test/publish gate.
    # The original independent verdict remains in outcome.json.
    affirmative=(verdict.get('approved') is True or bool(deferred) and not findings)
    approved = (result['valid'] and affirmative and
                verdict.get('candidate') == execution.claim['input']['candidate'] and
                isinstance(verdict.get('findings'), list) and not findings)
    receipt = {'candidate': execution.claim['input']['candidate'], 'approved': approved,
               'checkpoint': result['head'], 'branch': result['branch'],
               'record': next((name for name in changes if name.startswith('specs/reviews/')), None),
               'artifact': str(execution.directory / 'outcome.json'), 'deferred_generated_findings':deferred}
    if approved:
        return result, {'review': receipt, 'retained_artifacts': None}, []
    finding = {'category': 'review', 'source': execution.claim['input']['candidate'],
               'review_checkpoint': result['head'], 'branch': result['branch'],
               'details': result.get('source_edits') or verdict.get('findings') or ['review did not produce an affirmative exact-source verdict'],
               'artifact': receipt['artifact'], 'id': execution.claim['id']}
    from .repair_budget import check
    check(execution,task,path,assigned,findings or finding['details'])
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
    from .verification import run as remote_verify
    inp = execution.claim['input']
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
        execution.command('git', '-c', 'core.hooksPath=/dev/null', 'commit', '--no-gpg-sign', '-m',
                          f"process({task['name']}): record independent approval", cwd=path)
    approved_candidate = execution.command('git', 'rev-parse', 'HEAD', cwd=path).stdout.strip()
    execution.command('git', 'checkout', '--detach', inp['base'], cwd=path)
    merge = execution.command('git', '-c', 'core.hooksPath=/dev/null', 'merge', '--no-gpg-sign', '--no-ff',
                              '-m', f"verify({task['name']}): reconcile candidate with main",
                              approved_candidate, cwd=path, check=False)
    if merge.returncode:
        generated,remaining=helper('pipeline-artifacts').resolve_generated_conflicts(path,inp['base'])
        if generated:
            execution.store.event('generated_conflicts_resolved',{'paths':generated,'base':inp['base']},task['name'],execution.claim['id'])
            if not remaining:
                execution.command('git','-c','core.hooksPath=/dev/null','commit','--no-gpg-sign','--no-edit',cwd=path)
                merge.returncode=0
    if merge.returncode:
        finding = {'id': execution.claim['id'], 'category': 'rebase', 'source': inp['candidate'],
                   'base': inp['base'], 'details': merge.stdout + merge.stderr}
        return {'passed': False}, {'repair': finding}, [finding]
    head = execution.command('git', 'rev-parse', 'HEAD', cwd=path).stdout.strip()
    tree = execution.command('git', 'rev-parse', 'HEAD^{tree}', cwd=path).stdout.strip()
    remote = remote_verify(execution, path, {'candidate': inp['candidate'], 'base': inp['base'], 'head': head, 'tree': tree})
    records = remote['checks']
    if not remote['passed']:
        failed = next(record for record in records if record['exit_code'])
        code = failed['exit_code']
        if code == 124:
            from .verification import deadline_error
            raise deadline_error()
        if code in (75, 77, 79, 137) or code < 0:
            raise Retry('verification infrastructure unavailable')
        if code == 81:
            observation = static_baseline_observation(execution.log, inp['base'])
            prerequisite = baseline_repair(execution, observation)
            finding = observation | {'id': execution.claim['id'], 'category': 'baseline',
                                     'source': inp['candidate'], 'prerequisite': prerequisite,
                                     'log_tail': execution.log_tail(16000)}
            return {'passed': False, 'checks': records}, {
                'repair': finding, 'dependencies': sorted(set(task['data']['dependencies']) | {prerequisite})}, [finding]
        finding = {'id': execution.claim['id'], 'category': 'verification',
                   'source': inp['candidate'], 'base': inp['base'],
                   'command': failed['command'], 'exit_code': code,
                   'artifact': str(execution.log),
                   'log_tail': execution.log_tail(16000)}
        return {'passed': False, 'checks': records}, {'repair': finding}, [finding]
    branch = f"pipeline/verified/{execution.claim['id']}-{execution.claim['token']}"
    if remote.get('publication'):
        publication=remote['publication']
        actual=execution.command('git','ls-remote','origin','refs/heads/'+branch,cwd=path).stdout.split()
        if not actual or actual[0]!=publication.get('head'):raise Retry('verified artifact branch was not confirmed')
        execution.command('git','fetch','--quiet','origin',branch,cwd=path,timeout=180)
        try:
            helper('pipeline-artifacts').validate(path,{'head':head,'tree':tree},publication,task['name'],branch)
        except ValueError as exc:
            from .execution import Attention
            raise Attention('Verified artifact proof rejected: '+str(exc)) from exc
        head,tree=publication['head'],publication['tree']
        execution.store.event('generated_artifacts_verified',publication|{'tested_head':remote['head']},task['name'],execution.claim['id'])
    else:
        execution.command('git', 'push', 'origin', head + ':refs/heads/' + branch, cwd=path, timeout=180)
    receipt = {'candidate': inp['candidate'], 'base': inp['base'], 'head': head,
               'tree': tree, 'branch': branch, 'checks': records, 'passed': True,
               'engine': 'gateway', 'sandbox': remote['sandbox'],
               'tested_head':remote.get('head'),'artifacts':remote.get('publication')}
    if remote.get('cache'):receipt['cache']=remote['cache']
    (execution.directory / 'verified.json').write_text(json.dumps(receipt))
    return receipt, {'verified': receipt}, []

def publish(execution, task):
    from .ci import publication, read as observe_ci, failure as ci_failure
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
        previous = publication(execution.store,task)
        if previous and previous['head'] != head and previous.get('candidate') == data.get('candidate'):
            # Do not overwrite a known failing published head with another
            # integration merge before its failure reaches implementation.
            observed = observe_ci(execution,task,previous)
            rejected = ci_failure(execution,task,previous,observed)
            if rejected:
                return rejected
            if observed['status'] not in ('passed','policy_pending','artifact_refresh') or observed.get('checks') != 'green':
                raise Wait('Awaiting CI on the recorded published head before changing the branch')
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
        current.update(pr=url, published_head=head, published_verified=verified,
                       delivery_branch=branch, pr_body_hash=body_hash)
        execution.store.db.execute('UPDATE tasks SET data=? WHERE name=?', (json.dumps(current), task['name']))
        execution.store.event('pull_request', {'url': url, 'head': head}, task['name'], execution.claim['id'])
    task = execution.store.task(task['name'])
    observation = observe_ci(execution,task,verified)
    if observation['status'] == 'infrastructure':
        from .ci import retry_infrastructure
        retry_infrastructure(execution,task,verified,observation)
    rejected = ci_failure(execution,task,verified,observation)
    if rejected:
        return rejected
    if observation.get('checks') == 'green':
        latest = execution.command('git', 'ls-remote', 'origin', 'refs/heads/main', cwd=path).stdout.split()[0]
        if latest != verified['base']:
            return {'base_changed': latest}, {'verified': None}, []
    if observation['status'] != 'passed':
        raise Wait('GitHub merge gate: ' + observation['status'])
    permit = 'merge-' + execution.claim['id']
    if not execution.store.reserve(permit, execution.claim['id'], execution.claim['token'], 'merge', 1,
                                   {'url': url, 'head': head, 'base': verified['base']}):
        raise Retry('another upstream merge is awaiting confirmation')
    latest = execution.command('git', 'ls-remote', 'origin', 'refs/heads/main', cwd=path).stdout.split()[0]
    if latest != verified['base']:
        execution.store.resource_state(permit, 'absent')
        return {'base_changed': latest}, {'verified': None}, []
    method = merge_method(execution, repo)
    command = ['gh', 'pr', 'merge', url, method, '--match-head-commit', head]
    if method != '--rebase':
        command += ['--subject', f"feat({task['name']}): {data['title']}",
                    '--body-file', str(description)]
    execution.command(*command, timeout=60)
    # The next observation confirms the authoritative merge and its tree.
    raise Wait('merge requested; awaiting upstream confirmation')


def merge_method(execution, repository):
    """Honor repository merge policy without an administrator bypass."""
    fields = ('allow_merge_commit', 'allow_squash_merge', 'allow_rebase_merge')
    policy = json.loads(execution.command('gh', 'api', 'repos/' + repository, '--jq',
                        '{allow_merge_commit,allow_squash_merge,allow_rebase_merge}', timeout=30).stdout)
    if not isinstance(policy, dict) or any(type(policy.get(field)) is not bool for field in fields):
        raise Retry('repository merge policy observation incomplete')
    for field, method in zip(fields, ('--merge', '--squash', '--rebase')):
        if policy[field]:
            return method
    raise Wait('repository policy currently disables every merge method')


def static_baseline_observation(log, base):
    """Use the gate's reproduced failure identity, never an attempt's path."""
    marker = 'GYRE_BASELINE_FAILURE_JSON '
    records = [line[len(marker):] for line in Path(log).read_text().splitlines()
               if line.startswith(marker)]
    if not records:
        raise Retry('baseline gate exited 81 without reproduced failure evidence')
    try:
        value = json.loads(records[-1])
    except ValueError as exc:
        raise Retry('baseline gate returned malformed failure evidence') from exc
    if (not isinstance(value, dict) or value.get('base') != base or
            not isinstance(value.get('environment'), str) or
            not re.fullmatch(r'[0-9a-f]{64}', value['environment']) or
            not isinstance(value.get('probe'), list) or not value['probe'] or
            not all(isinstance(arg, str) and arg for arg in value['probe'])):
        raise Retry('baseline gate evidence does not identify the verified base and probe')
    from .repairs import fingerprint
    return {'status': 'baseline_failed', 'base': base,
            'environment': 'host-' + value['environment'],
            'probe': value['probe'], 'baseline_log': str(log),
            'failure_signature': fingerprint(value['probe'], str(value.get('log', '')))}


def baseline_repair(execution, observation):
    """Deduplicate a real upstream failure into a scoped prerequisite task."""
    from .catalog import generation_for
    store = execution.store
    from .repairs import fingerprint, probe, satisfied
    signature = observation.get('failure_signature')
    key = observation['base'] + ':' + (signature or observation['environment'])
    excerpt = Path(observation['baseline_log']).read_text()[-24000:]
    shared=observation.get('status')=='environment_failed'
    title=('Repair shared CI environment ' + signature[:12]) if shared else ('Repair verified failure on main ' + observation['base'][:12])
    body = ('---\ntitle: "' + title + '"\n'
            'spec_ref: "GOAL.md — real implementations and meaningful verification"\n'
            'depends_on: []\nprogress: not-started\ncommits: []\n---\n\n'
            '## Required behavior\n\n' + ('Repair this shared CI setup failure. The failed installer ran with the same workflow and dependency inputs as the upstream base; an older green run does not prove the current runner supports them. ' if shared else 'Reproduce and repair this verified upstream failure. ') +
            'Implement real production fixes or correct a genuinely broken test setup. '
            'Do not weaken checks, add skips or exemptions, or implement the blocked feature. '
            'Obtain independent review and pass full verification and GitHub checks.\n\n'
            'Base: `' + observation['base'] + '`\n'
            'Environment fingerprint: `' + observation['environment'] + '`\n\n'
            + ('CI setup inputs: '+json.dumps(observation.get('environment_failure',{}),sort_keys=True)+'\n\n' if shared else '') +
            '## Baseline failure\n\n```text\n' + excerpt + '\n```\n')
    generation = generation_for(store, body)
    with store.transaction():
        for task in store.tasks():
            if satisfied(task['data']):
                continue
            previous_signature = task['data'].get('baseline_failure')
            if signature and previous_signature is None and probe(task) == observation.get('probe'):
                for line in task['body'].split('## Shipped')[0].splitlines():
                    if line.startswith('GYRE_BASELINE_FAILURE_JSON '):
                        try:
                            old = json.loads(line.removeprefix('GYRE_BASELINE_FAILURE_JSON '))
                            previous_signature = fingerprint(old['probe'], str(old.get('log', '')))
                        except (ValueError, KeyError, TypeError):
                            continue
            if (signature and previous_signature == signature) or task['data'].get('baseline_key') == key:
                return task['name']
        number = max((int(task['name'][5:]) for task in store.tasks()), default=0) + 1
        name = f'task-{number:03d}'
        data = metadata(body) | {'baseline_key': key, 'catalog_generation': generation,
                                'baseline_probe': observation.get('probe'), 'baseline_failure': signature}
        if shared:data.update(ci_environment=observation['environment_failure'],triaged=generation)
        store.db.execute('INSERT INTO tasks VALUES(?,?,?,?,?)', (name, generation, body, json.dumps(data), time.time()))
        store.event('baseline_prerequisite', {'base': observation['base'], 'evidence': observation['baseline_log']}, name)
        return name


def merge_observation(execution, url):
    """Distinguish a queued merge from a rejected or lost merge request."""
    from urllib.parse import urlparse
    owner, repository = execution.store.setting('repository', 'jsell-rh/gyre').split('/')
    parsed = urlparse(url)
    number = int(parsed.path.rstrip('/').rsplit('/', 1)[-1])
    if parsed.hostname != 'github.com' or parsed.path.rstrip('/') != f'/{owner}/{repository}/pull/{number}':
        raise Retry('merge URL does not identify a PR in the configured repository')
    query = '''query($owner:String!,$repository:String!,$number:Int!) {
      repository(owner:$owner,name:$repository) { pullRequest(number:$number) {
        state headRefOid mergeCommit { oid }
        autoMergeRequest { enabledAt } mergeQueueEntry { id }
      } }
    }'''
    value = json.loads(execution.command('gh', 'api', 'graphql', '-f', 'query=' + query,
                       '-f', 'owner=' + owner, '-f', 'repository=' + repository,
                       '-F', 'number=' + str(number), timeout=30).stdout)
    if value.get('errors'):
        raise Retry('GitHub merge observation unavailable')
    observation = value['data']['repository']['pullRequest']
    fields = {'state', 'headRefOid', 'mergeCommit', 'autoMergeRequest', 'mergeQueueEntry'}
    if (not observation or not fields.issubset(observation) or
            observation['state'] not in ('OPEN', 'CLOSED', 'MERGED')):
        raise Retry('GitHub merge observation incomplete')
    return observation


def cleanup(execution, task):
    resource = execution.claim['input']['resource']
    record = execution.store.db.execute('SELECT * FROM resources WHERE name=?', (resource,)).fetchone()
    if record['kind'] == 'admission':
        # No external create can precede the atomic transfer to a sandbox
        # reservation. An expired pre-launch admission owns no remote compute.
        execution.store.resource_state(resource, 'absent')
        return {'released_admission': resource}, {}, []
    if record['kind'] in ('host', 'runner'):
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
        observation = merge_observation(execution, data['url'])
        if observation['state'] == 'OPEN' and (
                observation['autoMergeRequest'] is not None or observation['mergeQueueEntry'] is not None):
            raise Wait('queued upstream merge is unresolved; holding the global merge permit')
        # A rejected request (e.g. main advanced during strict merge checks)
        # leaves the PR open without an automatic or queued merge. Keeping
        # its permit forever would prevent every subsequent delivery.
        execution.store.resource_state(resource, 'absent')
        return {'merge_observed': observation}, {}, []
    execution.login()
    if record['kind'] == 'provider':
        # A paginated list cannot establish this provider's absence.
        observed = execution.os('provider', 'get', resource, timeout=30, check=False)
        if observed.returncode == 0:
            execution.os('provider', 'delete', resource, timeout=60)
            observed = execution.os('provider', 'get', resource, timeout=30, check=False)
        if observed.returncode == 0 or 'provider not found' not in (observed.stderr + observed.stdout).lower():
            raise Retry('run-scoped provider deletion not yet confirmed')
        execution.store.resource_state(resource, 'absent')
        return {'deleted_provider': resource}, {}, []
    inventory = gateway.inventory()
    item = next((item for item in inventory if item['name'] == resource), None)
    if item and item['labels'].get('gyre.dev/pipeline') != execution.store.setting('owner'):
        raise ValueError('cleanup resource belongs to another actor')
    if item is None and record['state'] == 'deleting':
        # Source recovery can retry independently after compute is gone.
        execution.store.resource_state(resource, 'absent')
    updates = {}
    owned = record
    old_work = execution.store.db.execute('SELECT * FROM work WHERE id=?', (owned['work'],)).fetchone()
    old_directory = execution.store.directory / 'attempts' / owned['work'] / str(owned['token'])
    old_directory.mkdir(parents=True, exist_ok=True)
    if any(item['name'] == resource for item in inventory):
        # Only implementers own source to salvage. Reviews and exact-tree
        # verification must not spend minutes indexing generated build output.
        # Stop durable agent supervisors before capturing an implementer's index.
        code = """import json,os,pathlib,signal,time,subprocess,sys
for root in pathlib.Path('/sandbox/workspace').glob('.gyre-agent-*'):
 if root.is_dir() and (root/'identity.json').exists():
  subprocess.run([sys.executable,'/tmp/stage/pipeline-agent.py','cancel',str(root)],check=True,timeout=12)
pairs=[pathlib.Path('/tmp/stage/step.exit.process.json')]
pairs.extend(pathlib.Path('/sandbox/workspace').glob('.gyre-agent-*/identity.json'))
pairs.extend(pathlib.Path('/sandbox/workspace').glob('.gyre-agent-*/child.json'))
for p in pairs:
 if not p.exists(): continue
 value=json.loads(p.read_text()); pid=int(value['pid']); stat=pathlib.Path(f'/proc/{pid}/stat')
 def same_process():
  try:
   fields=stat.read_text().rsplit(')',1)[1].split()
   return fields[0]!='Z' and fields[19]==value['start'] and pathlib.Path('/proc/sys/kernel/random/boot_id').read_text().strip()==value['boot']
  except FileNotFoundError: return False
 try:
  if same_process():
   os.killpg(pid,signal.SIGTERM); time.sleep(1)
   if same_process(): os.killpg(pid,signal.SIGKILL)
 except ProcessLookupError: pass
"""
        try:
            if item['phase'].lower() != 'ready' and not any(old_directory.parent.glob('*/remote.offset')):
                raise RuntimeError('No agent was attached; skip unreachable source capture and purge the unready sandbox')
            if old_work and old_work['stage']=='implement':
                execution.os('sandbox','upload',resource,str(ROOT/'scripts/pipeline-agent.py'),'/tmp/stage/pipeline-agent.py','--no-git-ignore',timeout=60)
                stopped=execution.remote(resource, 'python3', '-c', code, timeout=30, check=False)
                if stopped.returncode:raise RuntimeError('owned agent tools did not quiesce; source capture skipped')
            raw = execution.remote(resource, 'tail', '-c', '1048576', '/tmp/stage/step.log', timeout=30, check=False)
            (old_directory / 'orphan.log').write_text(raw.stdout)
            if old_work and old_work['stage']=='implement':
                try:
                    execution.command('env', 'GYRE_DEV_RECOVERY_REPO=' + json.loads(owned['data']).get('repo', '/tmp/gyre'), 'python3', str(ROOT / 'scripts/dev-recover.py'), resource,
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
            and old_work['state'] not in ('succeeded', 'obsolete') and old_work['generation'] == task['generation']
            and not task['data'].get('delivered')):
        old_input = json.loads(old_work['input'])
        if task['data'].get('candidate') == old_input.get('candidate'):
            from .recovery import restore, unchanged_bootstrap
            # Cleanup has the task claim, so another implementation cannot
            # race this recovery. The Git lease separately fences pushes.
            captured_base = json.loads(receipt.read_text())['base']
            repository = checkout(execution, captured_base)
            tree = execution.command('git', 'rev-parse', 'HEAD^{tree}', cwd=repository).stdout.strip()
            # A clone can retain an older main while fetching the assignment's
            # newer base. Only ignore an empty capture on that known ancestry.
            empty = unchanged_bootstrap(old_directory, captured_base, tree)
            ancestor = (empty and old_input.get('base') and execution.command(
                'git', 'merge-base', '--is-ancestor', captured_base, old_input['base'],
                cwd=repository, check=False).returncode == 0)
            if ancestor:
                execution.store.event('empty_bootstrap_capture', {'resource': resource, 'base': captured_base}, task['name'])
                execution.store.resource_state(resource, 'absent')
                return {'deleted': resource, 'unchanged_bootstrap': True}, {}, []
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


from .assessment import run as assess
HANDLERS = {'triage': triage, 'assess': assess, 'implement': implement, 'review': review,
            'verify': verify, 'publish': publish, 'cleanup': cleanup}
from .ci import observe
HANDLERS['observe'] = observe
