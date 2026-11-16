"""Avoid paying for a review of source already known to require a rebase."""
from .execution import Retry
from .repair_budget import source_findings


def check(execution, task):
    latest = execution.store.setting('observed_main')
    candidate = execution.claim['input']['candidate']
    if not latest or latest == candidate:
        return None
    from .stages import checkout
    path = checkout(execution, candidate)
    execution.command('git', 'fetch', '--quiet', 'origin', latest, cwd=path, timeout=180)
    merged = execution.command('git', 'merge-tree', '--write-tree', '--name-only', '-z', latest, candidate,
                               cwd=path, check=False, timeout=180)
    if merged.returncode == 0:
        return None
    if merged.returncode != 1:
        raise Retry('Review preflight could not determine mergeability; preserve source and retry infrastructure')
    # NUL framing preserves paths containing spaces, quotes or newlines.
    paths = []
    for name in merged.stdout.split('\0')[1:]:
        if not name:
            break
        paths.append(name)
    if not paths:
        raise Retry('Review preflight could not read conflict paths; no model or code repair was authorized')
    summary = 'specs/coverage/SUMMARY.md'
    summary_only = not any(name.startswith('specs/coverage/') and name != summary for name in paths)
    source, _ = source_findings([{'file': name, 'detail': 'merge conflict'} for name in paths
                                if name != summary or not summary_only])
    if not summary_only and summary in paths:
        source.append({'file': summary, 'detail': 'coverage matrices require a source merge'})
    if not source:
        return None  # The verifier already owns deterministic web/dist recovery.
    finding = {'id': execution.claim['id'], 'category': 'rebase', 'source': candidate,
               'base': latest, 'files': [item['file'] for item in source],
               'details': 'Rebase the saved source against ' + latest + '. Resolve conflicts in: '
                          + ', '.join(item['file'] for item in source)
                          + '. Preserve existing behavior, then obtain fresh independent review and verification.'}
    execution.store.event('review_deferred_for_rebase', finding, task['name'], execution.claim['id'])
    return {'preflight': 'source_conflicts', 'candidate': candidate, 'base': latest}, {
        'repair': finding, 'review': None, 'verified': None}, [finding]
