"""Repair task progress markers on saved source, without another model round."""
import json
from .catalog import contract, safe_metadata
from .execution import ROOT


def normalize(execution, task):
    repair = task['data'].get('repair') or {}
    candidate = task['data'].get('candidate')
    if (repair.get('category') != 'contract' or repair.get('source') != candidate
            or not repair.get('checkpoint') or repair.get('agent_exit') != 0):
        return None
    # Require the original successful agent's published receipt. A diagnostic
    # string alone cannot promote an interrupted or failed run into review.
    receipt = None
    root = execution.store.directory / 'attempts' / str(repair.get('id', ''))
    for path in root.glob('*/outcome.json'):
        result = json.loads(path.read_text())
        if (result.get('head') == candidate and result.get('published')
                and result.get('valid') is True and result.get('agent_exit') == 0
                and result.get('stream_exit') == 0):
            receipt = result
            break
    if receipt is None:
        return None
    from .stages import checkout
    path = checkout(execution, candidate)
    document = path / 'specs/tasks' / (task['name'] + '.md')
    observed = document.read_text()
    restored = contract.restore_checkbox_markers(task['body'], observed)
    if (restored == observed or contract.requirement_parts(restored) != contract.requirement_parts(task['body'])
            or safe_metadata(restored)['progress'] not in ('ready-for-review', 'complete')):
        return None
    document.write_text(restored)
    branch = f"pipeline/{task['name']}/{execution.claim['id']}-{execution.claim['token']}"
    execution.command('git', 'config', 'user.name', 'gyre-pipeline', cwd=path)
    execution.command('git', 'config', 'user.email', 'jsell-rh@users.noreply.github.com', cwd=path)
    execution.command('git', 'checkout', '-B', branch, cwd=path)
    execution.command('git', 'add', '--', str(document), cwd=path)
    execution.command('git', '-c', 'core.hooksPath=/dev/null', 'commit', '--no-gpg-sign', '-m',
                      f"process({task['name']}): restore assigned acceptance markers", cwd=path)
    # Detached CI/shared clones may not have origin/main yet. Attribution
    # requires that exact tracking ref rather than an ambient local branch.
    execution.command('git', 'fetch', '--quiet', 'origin', '+refs/heads/main:refs/remotes/origin/main', cwd=path, timeout=180)
    # Include any unlisted source commits in normal attribution bookkeeping.
    execution.command('python3', str(ROOT / 'scripts/dev-attribution.py'), task['name'], '--commit', cwd=path)
    head = execution.command('git', 'rev-parse', 'HEAD', cwd=path).stdout.strip()
    execution.command('git', 'push', 'origin', f'HEAD:refs/heads/{branch}', cwd=path, timeout=180)
    result = receipt | {'head': head, 'tree': execution.command('git', 'rev-parse', 'HEAD^{tree}', cwd=path).stdout.strip(),
                        'branch': branch, 'task_body': document.read_text(), 'normalized_from': candidate}
    (execution.directory / 'outcome.json').write_text(json.dumps(result))
    execution.store.event('contract_checkpoint_normalized', {'candidate': candidate, 'head': head},
                          task['name'], execution.claim['id'])
    return result
