"""Reproduce historical static failures before paying for another repair agent."""
import fcntl
import hashlib
import json
import re

from .execution import Wait


def satisfied(data):
    return (not data.get('assessment_pending') and not (data.get('requirements') or {}).get('errors')
            and bool(data.get('delivered') or data.get('resolved')))


def probe(task):
    data = task['data']
    if not data.get('baseline_key') and not re.fullmatch(
            r'Repair verified failure on main [0-9a-f]{12}', data.get('title') or ''):
        return None
    command = data.get('baseline_probe')
    if command is None:
        for line in task['body'].split('## Shipped')[0].splitlines():
            if line.startswith('GYRE_BASELINE_FAILURE_JSON '):
                try:
                    command = json.loads(line.removeprefix('GYRE_BASELINE_FAILURE_JSON ')).get('probe')
                except ValueError:
                    continue
    # Historical evidence cannot nominate arbitrary host/sandbox commands.
    if (isinstance(command, list) and len(command) == 2 and command[0] == 'bash'
            and isinstance(command[1], str) and re.fullmatch(r'scripts/check-[a-z0-9-]+\.sh', command[1])):
        return command
    return None


def fingerprint(command, output):
    # Worker paths and ANSI escape codes are observations, not failure identity.
    output = re.sub(r'\x1b\[[0-9;]*m', '', output)
    output = re.sub(r'/(?:home|tmp|sandbox)/[^\s:]+', '<path>', output)
    return hashlib.sha256(json.dumps([command, output.strip()], sort_keys=True).encode()).hexdigest()


def cache_key(store, base, command):
    from .execution import ROOT
    image = store.setting('fullsend_image') or json.loads((ROOT / '.fullsend/toolchain.json').read_text())['worker_image']
    return 'baseline-check:' + hashlib.sha256(json.dumps([base, command, image]).encode()).hexdigest()


def cached(store, base, command):
    from .verification import validate_receipt
    receipt = store.setting(cache_key(store,base,command))
    if receipt is not None:
        validate_receipt(receipt, {'candidate':base,'base':base,'head':base}, [command])
    return receipt


def resolution(task, receipt):
    updates = {'baseline_checked': {'base': receipt['base'], 'passed': receipt['passed']}, 'triaged': task['generation']}
    if receipt['passed']:
        updates['resolved'] = {'reason': 'Recorded failure no longer reproduces on upstream main',
                               'base': receipt['base'], 'probe': receipt['probe'], 'receipt': receipt,
                               'generation': task['generation']}
    return updates


def recheck(execution, task):
    from .stages import checkout
    from .verification import run, validate_receipt
    store, inp = execution.store, execution.claim['input']
    command, base = inp['baseline_probe'], inp['base']
    if probe(task) != command:
        raise ValueError('baseline recheck does not match the recorded safe probe')
    key = cache_key(store,base,command)
    with (store.directory / 'baseline-check.lock').open('a') as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise Wait('Another worker is rechecking upstream baseline health; reuse its receipt when complete')
        receipt = store.setting(key)
        if receipt is None:
            path = checkout(execution, base)
            tree = execution.command('git', 'rev-parse', 'HEAD^{tree}', cwd=path).stdout.strip()
            identity = {'candidate': base, 'base': base, 'head': base, 'tree': tree}
            receipt = run(execution, path, identity, probe=command)
            code = receipt['checks'][0]['exit_code']
            if code in (75, 77, 79, 124, 137) or code < 0:
                from .execution import Retry
                raise Retry('Upstream baseline probe infrastructure unavailable; no repair was resolved')
            receipt.update(base=base, probe=command, artifact=str(execution.directory / 'remote-verification.json'))
            store.set_setting(key, receipt)
        validate_receipt(receipt, {'candidate':base, 'base':base, 'head':base}, [command])
        return receipt, resolution(task,receipt), []
