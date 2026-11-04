"""Gateway execution of exact-tree deterministic gates; no inference or local builds."""
import base64
import hashlib
import json
import os
import shutil
import subprocess
import time

from .execution import ROOT, Retry, Wait, gateway

COMMANDS = [['bash', '/tmp/stage/checks.sh'], ['cargo', 'test', '--all', '--quiet'],
            ['npm', 'test', '--', '--no-file-parallelism']]


def decode_poll(output):
    frames = [line.removeprefix('GYRE_VERIFY_POLL ') for line in output.splitlines()
              if line.startswith('GYRE_VERIFY_POLL ')]
    if len(frames) != 1:
        raise ValueError('expected one framed verification response')
    return json.loads(frames[0])


def validate_receipt(receipt, identity, commands=None):
    commands = COMMANDS if commands is None else commands
    if not isinstance(receipt, dict) or any(receipt.get(key) != expected for key, expected in identity.items()):
        raise Retry('verification receipt differs from the assigned source')
    if receipt.get('infrastructure'):
        raise Retry('remote verification infrastructure unavailable; retained log has details')
    records = receipt.get('checks')
    if not isinstance(records, list) or not 1 <= len(records) <= len(commands):
        raise Retry('verification receipt lacks its mandatory checks')
    for index, record in enumerate(records):
        if (not isinstance(record, dict) or record.get('command') != commands[index]
                or type(record.get('exit_code')) is not int or (index < len(records) - 1 and record['exit_code'] != 0)):
            raise Retry('verification receipt has invalid or reordered checks')
    passed = len(records) == len(commands) and all(record['exit_code'] == 0 for record in records)
    if type(receipt.get('passed')) is not bool or receipt['passed'] != passed or (not passed and records[-1]['exit_code'] == 0):
        raise Retry('verification receipt has an inconsistent verdict')


def retire(execution, name):
    observed = next((item for item in gateway.inventory() if item['name'] == name), None)
    if observed and observed['labels'].get('gyre.dev/pipeline') != execution.store.setting('owner'):
        raise RuntimeError('refusing to delete a foreign verification sandbox')
    if observed and observed['phase'].lower() == 'ready' and not (execution.directory / 'remote-verification.json').exists():
        try:
            raw = execution.remote(name, 'tail', '-c', '131072', '/tmp/stage/step.log', timeout=10, check=False)
            if raw.returncode == 0:
                (execution.directory / 'failure-tail.log').write_text(raw.stdout)
        except Exception as exc:
            execution.store.event('verification_tail_unavailable', {'resource': name, 'error': type(exc).__name__}, execution.claim['task'])
    execution.store.resource_state(name, 'deleting')
    result = execution.os('sandbox', 'delete', name, timeout=60, check=False)
    if result.returncode:
        return  # Deletion debt stays charged for the cleanup reconciler.
    if not any(item['name'] == name for item in gateway.inventory()):
        execution.store.resource_state(name, 'absent')


def run(execution, path, identity, probe=None):
    if probe is not None:
        import re
        if (not isinstance(probe, list) or len(probe) != 2 or probe[0] != 'bash' or
                not re.fullmatch(r'scripts/check-[a-z0-9-]+\.sh', probe[1])):
            raise ValueError('unsupported upstream baseline probe')
    store = execution.store
    os.environ.setdefault('RUST_LOG', 'error')
    execution.login()
    name = 'gyre-v-' + hashlib.sha256((execution.claim['id'] + ':' + str(execution.claim['token'])).encode()).hexdigest()[:12]
    if not store.reserve(name, execution.claim['id'], execution.claim['token'], 'sandbox', store.setting('slots', 8),
                         {'repo': '/sandbox/workspace/checkout', 'engine': 'verification'}):
        raise Wait('remote verification is waiting for shared sandbox capacity')
    try:
        image = store.setting('fullsend_image', json.loads((ROOT / '.fullsend/toolchain.json').read_text())['worker_image'])
        bundle = execution.directory / 'verification-input'
        bundle.mkdir(exist_ok=True)
        for source in ('pipeline-verify.py', 'dev-static-gate.py', 'check-rustfmt-diff.py', 'check-clippy-diff.py'):
            shutil.copyfile(ROOT / 'scripts' / source, bundle / source)
        gate = (ROOT / 'scripts/dev-check.sh').read_text().replace('/tmp/gyre', '/sandbox/workspace/checkout')
        (bundle / 'checks.sh').write_text(gate)
        job = identity | {'image': image, 'repo_url': store.setting('repo_url', 'https://github.com/jsell-rh/gyre.git')}
        if probe is not None:
            job['probe'] = probe
        (bundle / 'verify-job.json').write_text(json.dumps(job))
        if identity['head'] != identity['base']:
            execution.command('git', 'bundle', 'create', str(bundle / 'candidate.bundle'), 'HEAD', '^' + identity['base'], cwd=path, timeout=300)
        execution.phase('Provisioning', sandbox=name, engine='verification')
        # The reservation precedes create, so even an uncertain response is
        # owned and charged. Readiness polling retains the same allocation.
        try:
            created = execution.os('sandbox', 'create', '--name', name, '--from', image,
                                  '--policy', str(ROOT / '.fullsend/policies/worker.yaml'),
                                  '--provider', store.setting('verification_provider', 'github'), '--no-auto-providers',
                                  '--cpu', '2', '--memory', store.setting('verification_memory', '8Gi'),
                                  '--label', 'gyre.dev/pipeline=' + store.setting('owner'),
                                  '--label', 'gyre.dev/work=' + execution.claim['id'],
                                  '--detach', '--', 'sleep', 'infinity', timeout=180, check=False)
        except subprocess.TimeoutExpired:
            created = None
        observed = next((v for v in gateway.inventory() if v['name'] == name), None)
        if observed and observed['labels'].get('gyre.dev/pipeline') != store.setting('owner'):
            raise RuntimeError('verification sandbox ownership differs')
        if not observed and (created is None or created.returncode):
            raise Retry('verification sandbox creation was not confirmed')
        execution.wait_ready(name)
        execution.remote(name, 'mkdir', '-p', '/tmp/stage', timeout=30)
        for source in sorted(bundle.iterdir()):
            execution.os('sandbox', 'upload', name, str(source), '/tmp/stage/' + source.name, '--no-git-ignore', timeout=180)
        execution.remote(name, 'python3', '/tmp/stage/pipeline-verify.py', 'launch', timeout=30)
        execution.phase('Verifying', sandbox=name, engine='verification')
        offset, failures = 0, 0
        deadline = time.monotonic() + store.setting('verification_timeout', 10800)
        while time.monotonic() < deadline:
            execution.current()
            try:
                result = execution.remote(name, 'python3', '/tmp/stage/pipeline-verify.py', 'poll', '--offset', str(offset), timeout=30)
                value = decode_poll(result.stdout)
                data = base64.b64decode(value['log'], validate=True)
                if value['offset'] != offset + len(data):
                    raise ValueError('verification log offset mismatch')
                with execution.log.open('ab') as log:
                    log.write(data)
                offset = value['offset']
                (execution.directory / 'remote.offset').write_text(str(offset))
                failures = 0
                if value['receipt'] is not None:
                    receipt = value['receipt']
                    validate_receipt(receipt, identity, [probe] if probe is not None else None)
                    receipt['sandbox'] = name
                    (execution.directory / 'remote-verification.json').write_text(json.dumps(receipt))
                    return receipt
                if not value.get('alive') and not data:
                    raise Retry('remote verifier exited without a complete receipt')
            except (subprocess.TimeoutExpired, RuntimeError) as exc:
                if isinstance(exc, Retry):
                    raise
                failures += 1
                if failures >= 6:
                    raise Retry('remote verification transport unavailable') from exc
            except (ValueError, KeyError, TypeError) as exc:
                failures += 1
                if failures >= 6:
                    raise Retry('invalid remote verification protocol response') from exc
            execution.cancelled.wait(min(30, 2 ** failures) if failures else 2)
        raise Retry('remote verification exceeded its bounded deadline')
    finally:
        try:
            retire(execution, name)
        except Exception as exc:
            store.event('cleanup_observation_unavailable', {'resource': name, 'error': type(exc).__name__}, execution.claim['task'])
