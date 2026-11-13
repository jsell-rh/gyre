"""Gateway execution of exact-tree deterministic gates; no inference or local builds."""
import base64
import hashlib
import json
import os
import shutil
import subprocess
import time

from .execution import ROOT, Retry, Wait, Attention, gateway

COMMANDS = [['bash', '/tmp/stage/checks.sh'], ['cargo', 'test', '--all', '--quiet'],
            ['npm', 'test', '--', '--no-file-parallelism']]


POLICY_FILES=('pipeline-verify.py','dev-static-gate.py','check-rustfmt-diff.py',
              'check-clippy-diff.py','verifier-script-overlay.py','pipeline-artifacts.py',
              'pipeline-build-env.py','pipeline-cache.py','dev-cargo-clean.py','pipeline-usage.py')


def policy(store, bundle=None, image=None):
    """Hash the actual uploaded verifier, rather than a later host revision."""
    image=image or store.setting('fullsend_image',json.loads((ROOT/'.fullsend/toolchain.json').read_text())['worker_image'])
    if '@sha256:' not in image:return None
    files={name:hashlib.sha256(((bundle/name) if bundle else (ROOT/'scripts'/name)).read_bytes()).hexdigest() for name in POLICY_FILES}
    gate=(bundle/'checks.sh').read_bytes() if bundle else (ROOT/'scripts/dev-check.sh').read_text().replace('/tmp/gyre','/sandbox/workspace/checkout').encode()
    files['checks.sh']=hashlib.sha256(gate).hexdigest()
    files['worker.yaml']=hashlib.sha256(((bundle/'worker-policy.yaml') if bundle else ROOT/'.fullsend/policies/worker.yaml').read_bytes()).hexdigest()
    return hashlib.sha256(json.dumps({'image':image,'commands':COMMANDS,'files':files},sort_keys=True).encode()).hexdigest()


def deadline_error():
    error=Attention('Verification exhausted its deadline; inspect retained logs or raise the budget before retrying the same build')
    error.category='verification_timeout'
    return error


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
    if receipt.get('error_kind') == 'source_integrity' or receipt.get('error') == 'verification changed its assigned source':
        paths=receipt.get('dirty_paths') or []
        error=Attention('Verification source integrity failed; retained receipt lists changed paths: '+', '.join(str(path) for path in paths)[:1000])
        error.category='verification_source_integrity'
        error.diagnostic={key:receipt.get(key) for key in ('dirty_paths','dirty_path_count','observed_head','observed_tree')}
        raise error
    if receipt.get('infrastructure'):
        raise Retry('remote verification infrastructure unavailable; inspect remote-verification-raw.json and output.log')
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


def prepare_bundle(execution, path, identity, destination):
    destination.unlink(missing_ok=True)
    if identity['head']==identity['base']:return False
    if execution.command('git','rev-parse','HEAD',cwd=path).stdout.strip()!=identity['head']:
        raise ValueError('candidate bundle checkout differs from assigned head')
    ancestor=execution.command('git','merge-base','--is-ancestor',identity['head'],identity['base'],cwd=path,check=False)
    if ancestor.returncode==0:
        return False  # Fetching base already supplies this exact historical head.
    if ancestor.returncode!=1:raise Retry('Cannot establish candidate bundle ancestry')
    execution.command('git','bundle','create',str(destination),'HEAD','^'+identity['base'],cwd=path,timeout=300)
    return True


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
        for source in ('pipeline-verify.py', 'dev-static-gate.py', 'check-rustfmt-diff.py', 'check-clippy-diff.py', 'verifier-script-overlay.py', 'pipeline-artifacts.py', 'pipeline-usage.py', 'pipeline-build-env.py', 'pipeline-cache.py', 'dev-cargo-clean.py'):
            shutil.copyfile(ROOT / 'scripts' / source, bundle / source)
        gate = (ROOT / 'scripts/dev-check.sh').read_text().replace('/tmp/gyre', '/sandbox/workspace/checkout')
        (bundle / 'checks.sh').write_text(gate)
        (bundle/'worker-policy.yaml').write_bytes((ROOT/'.fullsend/policies/worker.yaml').read_bytes())
        assigned_policy=policy(store,bundle,image)
        timeout=store.setting('verification_timeout',14400)
        job = identity | {'image': image, 'timeout_seconds':timeout, 'repo_url': store.setting('repo_url', 'https://github.com/jsell-rh/gyre.git'),
                          'cache_enabled':store.setting('build_cache_enabled',True),'task':execution.claim['task'],'publication_branch':f"pipeline/verified/{execution.claim['id']}-{execution.claim['token']}"}
        if probe is not None:
            job['probe'] = probe
        (bundle / 'verify-job.json').write_text(json.dumps(job))
        prepare_bundle(execution,path,identity,bundle/'candidate.bundle')
        execution.phase('Provisioning', sandbox=name, engine='verification')
        # The reservation precedes create, so even an uncertain response is
        # owned and charged. Readiness polling retains the same allocation.
        from .compute import assigned_profile
        budget=assigned_profile(store,execution.claim)
        resources=['--cpu',budget['cpu'],'--memory',budget['memory']]
        record=store.db.execute('SELECT data,state FROM resources WHERE name=?',(name,)).fetchone()
        store.resource_state(name,record['state'],json.loads(record['data']) | {'compute':budget})
        try:
            created = execution.os('sandbox', 'create', '--name', name, '--from', image,
                                  '--policy', str(bundle / 'worker-policy.yaml'),
                                  '--provider', store.setting('verification_provider', 'github'), '--no-auto-providers',
                                  *resources,
                                  '--label', 'gyre.dev/pipeline=' + store.setting('owner'),
                                  '--label', 'gyre.dev/work=' + execution.claim['id'],
                                  '--no-tty', '--detach', '--', 'sleep', 'infinity', timeout=180, check=False)
        except subprocess.TimeoutExpired:
            created = None
        if created is not None:
            with execution.log.open('a') as output:
                output.write('Verification sandbox create (exit '+str(created.returncode)+'):\n'+created.stdout+created.stderr+'\n')
        else:
            with execution.log.open('a') as output:output.write('Verification sandbox create acknowledgement timed out; checking the same allocation.\n')
        observed = next((v for v in gateway.inventory() if v['name'] == name), None)
        if observed and observed['labels'].get('gyre.dev/pipeline') != store.setting('owner'):
            raise RuntimeError('verification sandbox ownership differs')
        if not observed and created is not None and created.returncode and 'Created sandbox:' not in (created.stdout+created.stderr):
            import re
            message=created.stdout+created.stderr
            gated=bool(re.search(r'timed out waiting\W+for gated workload Pod',message))
            error=Retry('Gateway timed out waiting for the verifier workload Pod; retry with backoff' if gated else
                        'verification sandbox creation was rejected; retained output.log has the CLI diagnostic')
            error.category='sandbox_provisioning'
            error.diagnostic={'kind':'gated_pod_timeout' if gated else 'creation_rejected','artifact':str(execution.log)}
            raise error
        # A timed-out acknowledgement or an accepted asynchronous creation is
        # uncertain, not absent. Keep this reservation/name while it becomes
        # visible/Ready instead of deleting an allocation the autoscaler needs.

        execution.wait_ready(name)
        from .runtime import ensure_workspace
        ensure_workspace(execution,name)
        execution.remote(name, 'mkdir', '-p', '/tmp/stage', timeout=30)
        for source in sorted(bundle.iterdir()):
            execution.os('sandbox', 'upload', name, str(source), '/tmp/stage/' + source.name, '--no-git-ignore', timeout=180)
        execution.phase('Verifying', sandbox=name, engine='verification')
        offset, failures, launched = 0, 0, False
        cache_handled=False
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            execution.current()
            try:
                if not launched:
                    execution.remote(name,'python3','/tmp/stage/pipeline-verify.py','launch',timeout=30)
                    launched=True
                result = execution.remote(name, 'python3', '/tmp/stage/pipeline-verify.py', 'poll', '--offset', str(offset), timeout=30)
                value = decode_poll(result.stdout)
                if value.get('cache_request') and not cache_handled:
                    from . import build_cache
                    build_cache.restore(execution,name,value['cache_request'],identity['base'])
                    cache_handled=True
                if value.get('usage'):
                    (execution.directory/'remote-usage.json').write_text(json.dumps(value['usage']))
                    from .compute import from_usage
                    oom=from_usage(execution,name,value['usage'])
                    if oom:raise oom
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
                    # Preserve even a rejected/invalid receipt before validation
                    # and before finally retires the sandbox. This is evidence,
                    # never an accepted verification receipt.
                    raw=execution.directory / 'remote-verification-raw.json'
                    temporary=raw.with_suffix('.tmp')
                    temporary.write_text(json.dumps(receipt));temporary.chmod(0o600);temporary.replace(raw)
                    validate_receipt(receipt, identity, [probe] if probe is not None else None)
                    if receipt.get('passed') and probe is None and not isinstance(receipt.get('publication'),dict):
                        raise Retry('verification receipt lacks the rebuilt artifact publication')
                    receipt['policy'] = assigned_policy
                    receipt['sandbox'] = name
                    (execution.directory / 'remote-verification.json').write_text(json.dumps(receipt))
                    if receipt.get('passed') and probe is None:
                        from . import build_cache
                        build_cache.save(execution,name,receipt)
                    return receipt
                if not value.get('alive') and not data:
                    raise Retry('remote verifier exited without a complete receipt')
            except (subprocess.TimeoutExpired, RuntimeError) as exc:
                (execution.directory/'transport-error.json').write_text(json.dumps({'kind':type(exc).__name__,'message':str(exc)[-2000:],'at':time.time()}))
                if isinstance(exc, Retry):
                    raise
                failures += 1
                # Keep the detached verifier and its build cache across RPC
                # loss. Restart only an authoritatively stopped, owned sandbox.
                if failures >= 2:
                    from . import runtime
                    def invoke(arguments, **kwargs):
                        return execution.os(*arguments,check=False,**{('capture' if k=='capture_output' else k):v for k,v in kwargs.items() if k!='text'})
                    value=runtime.snapshot(execution,name,invoke)
                    phase=(value or {}).get('phase','').lower()
                    if phase=='stopped':
                        oom=runtime.failure(execution,name)
                        if oom:raise oom
                        if runtime.restart(execution,name,invoke):
                            execution.remote(name,'mkdir','-p','/tmp/stage',timeout=30)
                            for source in sorted(bundle.iterdir()):
                                execution.os('sandbox','upload',name,str(source),'/tmp/stage/'+source.name,'--no-git-ignore',timeout=180)
                            execution.remote(name,'python3','/tmp/stage/pipeline-verify.py','launch','--resume-after-stop',timeout=30)
                            offset=0;failures=0;cache_handled=False
                    elif phase in ('error','failed','deleted','deleting'):
                        error=Attention('Verifier sandbox entered terminal phase '+phase);error.category='sandbox_stopped';raise error
                    elif failures>=6 and phase!='ready':
                        error=Attention('Cannot establish verifier health; retaining diagnostics instead of repeating builds blindly');error.category='gateway_transport';raise error

            except (ValueError, KeyError, TypeError) as exc:
                failures += 1
                if failures >= 6:
                    raise Attention('invalid remote verification protocol response; inspect retained diagnostics') from exc
            execution.cancelled.wait(min(30, 2 ** failures) if failures else 2)
        raise deadline_error()
    finally:
        try:
            retire(execution, name)
        except Exception as exc:
            store.event('cleanup_observation_unavailable', {'resource': name, 'error': type(exc).__name__}, execution.claim['task'])
