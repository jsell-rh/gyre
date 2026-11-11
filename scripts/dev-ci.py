#!/usr/bin/env python3
"""Observe GitHub checks on an exact PR head; retain actionable failure evidence."""
import hashlib
from datetime import datetime
import json
import re
import subprocess
from pathlib import Path


def check_sets(pr):
    pending, failed, infrastructure = [], [], []
    for check in pr.get('statusCheckRollup') or []:
        status = check.get('status') or check.get('state')
        conclusion = check.get('conclusion') or check.get('state')
        if status in ('QUEUED', 'IN_PROGRESS', 'PENDING', 'EXPECTED', 'WAITING', 'REQUESTED'):
            pending.append(check)
        elif conclusion in ('CANCELLED', 'TIMED_OUT', 'STALE', 'ERROR'):
            infrastructure.append(check)
        elif conclusion in ('FAILURE','FAILED'):
            failed.append(check)
        elif conclusion not in ('SUCCESS', 'NEUTRAL', 'SKIPPED'):
            pending.append(check)  # Missing/unknown conclusions are not defects.
    return pending, failed, infrastructure


def reconcile_checks(run, url, head, pr):
    """Resolve contradictory Actions statuses using source-bound terminal evidence.

    A conclusion alone never overrides a pending check. Require its completed
    job and completed current workflow attempt, on this exact PR head.
    """
    repo = re.fullmatch(r'https://github.com/([^/]+/[^/]+)/pull/\d+', url).group(1)
    checks, evidence, workflows = [], [], {}
    terminal = {'SUCCESS','FAILURE','NEUTRAL','SKIPPED','CANCELLED','TIMED_OUT','STALE','ERROR'}
    for original in pr.get('statusCheckRollup') or []:
        check = dict(original)
        match = re.fullmatch(r'https://github.com/' + re.escape(repo) +
                            r'/actions/runs/(\d+)/job/(\d+)', check.get('detailsUrl') or '')
        if (match and check.get('status') != 'COMPLETED' and
                check.get('conclusion') in terminal and check.get('completedAt')):
            ident, job_id = map(int,match.groups())
            if ident not in workflows:
                workflows[ident] = json.loads(run('gh','api',f'repos/{repo}/actions/runs/{ident}',timeout=30).stdout)
            workflow = workflows[ident]
            job = json.loads(run('gh','api',f'repos/{repo}/actions/jobs/{job_id}',timeout=30).stdout)
            if (workflow.get('id') == ident and workflow.get('head_sha') == head
                    and workflow.get('status') == 'completed' and workflow.get('conclusion')
                    and job.get('id') == job_id and job.get('run_id') == ident
                    and job.get('head_sha') == head and job.get('name') == check.get('name')
                    and job.get('run_attempt') == workflow.get('run_attempt')
                    and isinstance(job.get('run_attempt'),int)
                    and job.get('completed_at') == check['completedAt']
                    and (job.get('conclusion') or '').upper() == check['conclusion']):
                check['status'] = 'COMPLETED'
                evidence.append({'check':check['name'],'run':ident,'job':job_id,
                                 'head':head,'attempt':job['run_attempt'],'conclusion':check['conclusion']})
        checks.append(check)
    return pr | {'statusCheckRollup':checks}, evidence


def merge_conflict(pr):
    return (pr.get('state') == 'OPEN' and pr.get('mergeStateStatus') == 'DIRTY'
            and pr.get('mergeable') == 'CONFLICTING'
            and bool(re.fullmatch(r'[a-f0-9]{40}',pr.get('baseRefOid') or '')))


def setup_failure(run, repo, head, base, failed, logs):
    """Recognize source-independent setup failures, using terminal Actions evidence.

    An old green base run does not describe today's hosted runner. Never group
    product-test failures or candidates that changed the relevant CI inputs.
    """
    workflows, platforms = set(), set()
    for check in failed:
        ident=run_id(check)
        match=re.search(r'Error:\s*(?:ERROR:\s*)?Playwright does not support chromium on ([\w.-]+)',logs.get(ident,''))
        job_match=re.fullmatch(r'https://github.com/'+re.escape(repo)+r'/actions/runs/(\d+)/job/(\d+)',check.get('detailsUrl') or '')
        if not match or not job_match:return None
        workflow=json.loads(run('gh','api',f'repos/{repo}/actions/runs/{ident}','--jq','{id,head_sha,status,conclusion,run_attempt,path}',timeout=30).stdout)
        job=json.loads(run('gh','api',f'repos/{repo}/actions/jobs/{job_match[2]}','--jq','{id,name,head_sha,run_id,status,conclusion,run_attempt,steps}',timeout=30).stdout)
        if (workflow.get('id')!=ident or job.get('id')!=int(job_match[2]) or job.get('name')!=check.get('name')
                or workflow.get('head_sha')!=head or job.get('head_sha')!=head or job.get('run_id')!=ident
                or workflow.get('status')!='completed' or job.get('status')!='completed'
                or workflow.get('conclusion')!='failure' or job.get('conclusion')!='failure'
                or workflow.get('run_attempt')!=job.get('run_attempt')
                or not isinstance(job.get('run_attempt'),int)):
            raise RuntimeError('CI setup evidence changed; observe the current workflow attempt again')
        steps=[s['name'] for s in job.get('steps',[]) if s.get('conclusion')=='failure']
        path=workflow.get('path','')
        if steps!=['Install Playwright browsers'] or not re.fullmatch(r'\.github/workflows/[\w.-]+\.ya?ml',path):return None
        workflows.add(path);platforms.add(match[1])
    paths=sorted(workflows | {'web/package.json','web/package-lock.json','.npmrc','web/.npmrc'})
    cache={}
    def inputs(ref):
        result={}
        for path in paths:
            key=(ref,path)
            if key not in cache:
                response=run('gh','api',f'repos/{repo}/contents/{path}?ref={ref}','--jq','{sha,type,status}',timeout=30,check=False)
                value=json.loads(response.stdout)
                if response.returncode and str(value.get('status'))=='404' and path.endswith('.npmrc'):
                    cache[key]=None
                elif response.returncode or value.get('type')!='file' or not re.fullmatch(r'[a-f0-9]{40}',value.get('sha','')):
                    raise RuntimeError('Cannot establish exact CI configuration identity')
                else:cache[key]=value['sha']
            result[path]=cache[key]
        return result
    original=inputs(head)
    if original!=inputs(base):return None
    main=json.loads(run('gh','api',f'repos/{repo}/git/ref/heads/main',timeout=30).stdout).get('object',{}).get('sha','')
    if not re.fullmatch(r'[a-f0-9]{40}',main):raise RuntimeError('Cannot establish current main for CI recovery')
    fingerprint=hashlib.sha256(json.dumps([repo,'playwright-unsupported-platform',sorted(platforms),original],sort_keys=True).encode()).hexdigest()
    return {'status':'environment_refresh' if inputs(main)!=original else 'environment_failed',
            'failure_signature':fingerprint,'environment_failure':{'kind':'playwright-unsupported-platform',
                'platforms':sorted(platforms),'inputs':original},'target_base':main}


def transient_setup_failure(run, repo, head, base, failed, logs):
    """Only exact-head, terminal setup jobs with transport errors qualify.

    Product steps and changed workflow definitions always retain normal failure
    handling. A DNS-looking line elsewhere in a workflow is not evidence.
    """
    evidence=[]
    for check in failed:
        match=re.fullmatch(r'https://github.com/'+re.escape(repo)+r'/actions/runs/(\d+)/job/(\d+)',check.get('detailsUrl') or '')
        if not match:return None
        ident,job_id=map(int,match.groups())
        workflow=json.loads(run('gh','api',f'repos/{repo}/actions/runs/{ident}',timeout=30).stdout)
        job=json.loads(run('gh','api',f'repos/{repo}/actions/jobs/{job_id}',timeout=30).stdout)
        if (workflow.get('id')!=ident or workflow.get('head_sha')!=head or job.get('head_sha')!=head or job.get('id')!=job_id
                or job.get('run_id')!=ident or job.get('name')!=check.get('name')
                or workflow.get('status')!='completed' or job.get('status')!='completed'
                or job.get('conclusion')!='failure' or workflow.get('conclusion')!='failure'
                or not isinstance(job.get('run_attempt'),int) or job['run_attempt']!=workflow.get('run_attempt')):
            raise RuntimeError('CI infrastructure evidence changed; observe the current attempt again')
        steps=[x['name'] for x in job.get('steps',[]) if x.get('conclusion')=='failure']
        if not steps or any(x not in {'Install mold linker','Install dependencies','Install web dependencies',
                'Install Playwright browsers','Checkout','Set up job'} for x in steps):return None
        for step in steps:
            def belongs(line):
                if line.startswith(job['name']+'\t'+step+'\t'):return True
                # gh sometimes labels valid job logs UNKNOWN STEP. In that
                # case require the timestamp to fall inside the exact failed
                # setup step's authoritative API interval.
                if not line.startswith(job['name']+'\tUNKNOWN STEP\t'):return False
                stamp=re.search(r'\d{4}-\d\d-\d\dT[\d:.]+Z',line)
                metadata=next(s for s in job['steps'] if s['name']==step)
                try:
                    return (datetime.fromisoformat(metadata['started_at'])<=datetime.fromisoformat(stamp[0])
                            <=datetime.fromisoformat(metadata['completed_at']))
                except (KeyError,ValueError,TypeError):return False
            lines='\n'.join(line for line in logs.get(ident,'').splitlines() if belongs(line))
            if not re.search(r'Temporary failure resolving|Could not resolve host|Could not resolve proxy|'
                             r'EAI_AGAIN|Connection timed out|TLS handshake timeout|HTTP[^\n]*\b50[234]\b',lines,re.I):return None
        path=workflow.get('path','').split('@',1)[0]
        if not re.fullmatch(r'\.github/workflows/[\w.-]+\.ya?ml',path):return None
        inputs=[]
        for ref in (head,base):
            value=json.loads(run('gh','api',f'repos/{repo}/contents/{path}?ref={ref}',timeout=30).stdout)
            if value.get('type')!='file' or not re.fullmatch(r'[a-f0-9]{40}',value.get('sha','')):return None
            inputs.append(value['sha'])
        if inputs[0]!=inputs[1]:return None
        evidence.append({'run':ident,'attempt':job['run_attempt'],'job':job_id,'steps':steps})
    return {'status':'infrastructure','infrastructure_evidence':evidence,'detail':'Transient setup transport failure; retry the same head, without implementation.'}


def observe(run, url, head, base, directory):
    directory = Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    pr = json.loads(run('gh', 'pr', 'view', url, '--json',
                        'headRefOid,statusCheckRollup,mergeStateStatus,state,mergeCommit,baseRefOid,mergeable', timeout=30).stdout)
    if pr['headRefOid'] != head:
        raise RuntimeError('PR head changed outside the recorded verification')
    if pr['state'] == 'MERGED':
        return {'status': 'merged', 'sha': pr['mergeCommit']['oid']}
    if pr['state'] != 'OPEN':
        return {'status': 'closed'}
    checks = pr.get('statusCheckRollup') or []
    (directory / 'github-checks.json').write_text(json.dumps(pr, indent=2))
    # Conflicts require source repair even when CI is green, absent, or still
    # running. Recheck both refs and conflict state before handing off.
    if merge_conflict(pr):
        return {'status':'merge_conflict','head':head,'base':pr['baseRefOid'],
                'verified_base':base,'pr':url,'merge_state':'DIRTY',
                'check_summary':[{key:check.get(key) for key in ('name','status','conclusion','context','state')} for check in checks],
                'detail':'The published PR conflicts with its current target branch. Resolve the merge against current main, preserving the retained implementation. Inspect all unmerged paths, resolve source conflicts, regenerate conflicting build artifacts from the resolved source, and obtain fresh independent review and verification.'}
    pr, reconciled = reconcile_checks(run,url,head,pr)
    (directory / 'github-check-reconciliation.json').write_text(json.dumps(reconciled,indent=2))
    pending, failed, infrastructure = check_sets(pr)
    if (pending and not failed) or not checks:
        return {'status': 'pending'}
    if infrastructure and not failed:
        ids = sorted({run_id(check) for check in infrastructure}, key=str)
        return {'status': 'infrastructure', 'runs': ids}
    if not failed:
        # Includes required reviews/rules and late required checks. Never use an
        # admin push to evade GitHub's merge policy.
        return {'status': 'passed' if pr['mergeStateStatus'] == 'CLEAN' else 'policy_pending',
                'checks': 'green', 'merge_state': pr['mergeStateStatus']}
    repo = re.fullmatch(r'https://github.com/([^/]+/[^/]+)/pull/\d+', url).group(1)
    candidate_evidence, baseline_evidence = [], []
    logs={}
    artifact_directories = []
    baseline_only = True
    baseline_runs = json.loads(run('gh', 'api',
        f'repos/{repo}/actions/runs?head_sha={base}&per_page=100', timeout=30).stdout)['workflow_runs']
    for ident in sorted({run_id(check) for check in failed}):
        if ident is None:
            baseline_only = False
            candidate_evidence.append('Failed external status; no GitHub Actions log is available.\n' + json.dumps(failed))
            continue
        metadata = json.loads(run('gh', 'run', 'view', str(ident), '--repo', repo,
                                  '--json', 'workflowDatabaseId,name', timeout=30).stdout)
        try:
            fetched = run('gh', 'run', 'view', str(ident), '--repo', repo, '--log-failed', timeout=60, check=False)
            candidate_log = fetched.stdout if fetched.returncode == 0 else ''
        except (OSError, subprocess.TimeoutExpired):
            candidate_log = ''
        if not candidate_log:
            # A completed failed job can coexist with running sibling jobs.
            # Logs may be unavailable until its workflow ends; that cannot
            # turn a confirmed failure back into indefinite pending.
            candidate_log = 'Completed failed check; logs not yet available.\n' + json.dumps([
                {key:check.get(key) for key in ('name','conclusion','detailsUrl','targetUrl')}
                for check in failed if run_id(check) == ident])
        (directory / f'github-run-{ident}.log').write_text(candidate_log)
        logs[ident]=candidate_log
        allowance = max(1024, 60000 // (2 * len(failed)))
        candidate_evidence.append(f'## Current PR head {head}: {metadata["name"]}, run {ident}\n\n{candidate_log[-allowance:]}')
        if metadata['name'] == 'E2E Tests':
            artifacts = directory / f'github-run-{ident}-artifacts'
            try:
                downloaded = run('gh', 'run', 'download', str(ident), '--repo', repo,
                                 '--name', 'playwright-screenshots', '--dir', str(artifacts),
                                 timeout=90, check=False)
                if downloaded.returncode == 0 and artifacts.exists():
                    artifact_directories.append(str(artifacts))
            except (OSError, subprocess.TimeoutExpired):
                # Optional screenshots must not suppress known check failures.
                pass
        matching = [item for item in baseline_runs if item['head_sha'] == base and
                    item['workflow_id'] == metadata['workflowDatabaseId'] and item['status'] == 'completed']
        latest = max(matching, key=lambda item: item['run_number'], default=None)
        if not latest or latest['conclusion'] != 'failure':
            baseline_only = False
            continue
        baseline_log = run('gh', 'run', 'view', str(latest['id']), '--repo', repo, '--log-failed', timeout=60).stdout
        signature = failure_signature(candidate_log)
        if not signature or signature != failure_signature(baseline_log):
            baseline_only = False
        (directory / f'github-run-{latest["id"]}.log').write_text(baseline_log)
        baseline_evidence.append(f'## Exact upstream base {base}: run {latest["id"]}\n\n{baseline_log[-allowance:]}')
    log = directory / 'github-checks.log'
    candidate_path, baseline_path = directory / 'github-candidate.log', directory / 'github-baseline.log'
    candidate_path.write_text('\n\n'.join(candidate_evidence))
    baseline_path.write_text('\n\n'.join(baseline_evidence))
    log.write_text('\n\n'.join(candidate_evidence + baseline_evidence))
    result={'status': 'baseline_failed' if baseline_only else 'candidate_failed',
            'base': base, 'baseline_log': str(baseline_path), 'candidate_log': str(candidate_path),
            'pr': url, 'head': head, 'runs': sorted({run_id(check) for check in failed}, key=str),
            'artifact_directories': artifact_directories,
            'environment': 'github-' + hashlib.sha256(json.dumps(sorted((item.get('workflowName', ''), item.get('name', ''), item.get('context', '')) for item in failed)).encode()).hexdigest(),
            'log': str(log)}
    transient=transient_setup_failure(run,repo,head,base,failed,logs) if any(re.search(r'Temporary failure resolving|Could not resolve|EAI_AGAIN|Connection timed out|TLS handshake timeout|HTTP[^\n]*\b50[234]\b',v,re.I) for v in logs.values()) else None
    if transient:
        result.update(transient)
        return result
    setup=setup_failure(run,repo,head,base,failed,logs)
    if setup:
        result.update(setup,baseline_log=str(candidate_path))
    return result


def run_id(check):
    match = re.search(r'/actions/runs/(\d+)', check.get('detailsUrl') or check.get('targetUrl') or '')
    return int(match.group(1)) if match else None


def failure_signature(log):
    # Playwright's final failed-test list identifies defects independently of
    # run timestamps, durations, runner directories and expected screenshot diffs.
    return sorted(set(re.findall(r'(tests/e2e/[^\n]+?:\d+:\d+\s+›[^\n]+?)(?:\s+─+\s*)?$', log, re.M)))
