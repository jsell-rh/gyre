#!/usr/bin/env python3
"""Observe GitHub checks on an exact PR head; retain actionable failure evidence."""
import hashlib
import json
import re
from pathlib import Path


def observe(run, url, head, base, directory):
    directory = Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    pr = json.loads(run('gh', 'pr', 'view', url, '--json',
                        'headRefOid,statusCheckRollup,mergeStateStatus,state,mergeCommit', timeout=30).stdout)
    if pr['headRefOid'] != head:
        raise RuntimeError('PR head changed outside the recorded verification')
    if pr['state'] == 'MERGED':
        return {'status': 'merged', 'sha': pr['mergeCommit']['oid']}
    if pr['state'] != 'OPEN':
        return {'status': 'closed'}
    checks = pr.get('statusCheckRollup') or []
    (directory / 'github-checks.json').write_text(json.dumps(pr, indent=2))
    pending, failed, infrastructure = [], [], []
    for check in checks:
        status = check.get('status') or check.get('state')
        conclusion = check.get('conclusion') or check.get('state')
        if status in ('QUEUED', 'IN_PROGRESS', 'PENDING', 'EXPECTED', 'WAITING', 'REQUESTED'):
            pending.append(check)
        elif conclusion in ('CANCELLED', 'TIMED_OUT', 'STALE'):
            infrastructure.append(check)
        elif conclusion not in ('SUCCESS', 'NEUTRAL', 'SKIPPED'):
            failed.append(check)
    if pending or not checks:
        return {'status': 'pending'}
    if infrastructure and not failed:
        ids = sorted({run_id(check) for check in infrastructure})
        return {'status': 'infrastructure', 'runs': ids}
    if not failed:
        # Includes required reviews/rules and late required checks. Never use an
        # admin push to evade GitHub's merge policy.
        return {'status': 'passed' if pr['mergeStateStatus'] == 'CLEAN' else 'policy_pending'}
    repo = re.fullmatch(r'https://github.com/([^/]+/[^/]+)/pull/\d+', url).group(1)
    evidence = []
    baseline_only = True
    baseline_runs = json.loads(run('gh', 'api',
        f'repos/{repo}/actions/runs?head_sha={base}&per_page=100', timeout=30).stdout)['workflow_runs']
    for ident in sorted({run_id(check) for check in failed}):
        if ident is None:
            baseline_only = False
            evidence.append('Failed external status; no GitHub Actions log is available.\n' + json.dumps(failed))
            continue
        metadata = json.loads(run('gh', 'run', 'view', str(ident), '--repo', repo,
                                  '--json', 'workflowDatabaseId,name', timeout=30).stdout)
        candidate_log = run('gh', 'run', 'view', str(ident), '--repo', repo, '--log-failed', timeout=60).stdout
        evidence.append(f'## {metadata["name"]}: run {ident}\n\n{candidate_log[-65536:]}')
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
        evidence.append(f'## Exact upstream base {base}: run {latest["id"]}\n\n{baseline_log[-65536:]}')
    log = directory / 'github-checks.log'
    log.write_text('\n\n'.join(evidence))
    return {'status': 'baseline_failed' if baseline_only else 'candidate_failed',
            'base': base, 'baseline_log': str(log),
            'environment': 'github-' + hashlib.sha256(json.dumps(sorted((item.get('workflowName', ''), item.get('name', ''), item.get('context', '')) for item in failed)).encode()).hexdigest(),
            'log': str(log)}


def run_id(check):
    match = re.search(r'/actions/runs/(\d+)', check.get('detailsUrl') or check.get('targetUrl') or '')
    return int(match.group(1)) if match else None


def failure_signature(log):
    # Playwright's final failed-test list identifies defects independently of
    # run timestamps, durations, runner directories and expected screenshot diffs.
    return sorted(set(re.findall(r'(tests/e2e/[^\n]+?:\d+:\d+\s+›[^\n]+?)(?:\s+─+\s*)?$', log, re.M)))
