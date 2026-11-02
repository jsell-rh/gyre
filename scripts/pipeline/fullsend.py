"""Fullsend runs agents; Gyre owns claims, source receipts and capacity."""
import hashlib
import base64
import re
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import subprocess
import sys
import time
import threading

from .execution import ROOT, Retry, Wait


def binary(store):
    lock = json.loads((ROOT / '.fullsend/toolchain.json').read_text())
    path = Path(os.environ.get('GYRE_FULLSEND', store.directory / 'tools' / ('fullsend-' + lock['version']))).resolve()
    arch = {'x86_64': 'amd64', 'aarch64': 'arm64'}.get(platform.machine())
    expected = lock.get(f'linux_{arch}_binary_sha256')
    if not expected or not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != expected:
        raise Retry('Pinned fullsend binary missing or changed; run scripts/dev-fullsend.py install')
    return path


def forge_token():
    value = os.environ.get('GH_TOKEN') or os.environ.get('GITHUB_TOKEN')
    if value:
        return value
    result = subprocess.run(['gh', 'auth', 'token'], capture_output=True, timeout=15)
    if result.returncode or not result.stdout.strip():
        raise Retry('GitHub credential unavailable; run gh auth login or set GH_TOKEN')
    return result.stdout.decode('utf-8').strip()


def context(path):
    from .store import Store
    from .execution import Execution
    value = json.loads(Path(path).read_text())
    store = Store(value['state'])
    from .credentials import load
    load(store)
    claim = dict(store.db.execute('SELECT * FROM work WHERE id=?', (value['work'],)).fetchone())
    if claim['token'] != value['token']:
        store.close()
        raise RuntimeError('fullsend assignment has been fenced')
    claim['input'] = json.loads(claim['input'])
    execution = Execution(store, claim)
    execution.current()
    execution.branch = value['branch']
    return execution


def adopt(execution, task):
    claim = execution.claim
    previous = json.loads(claim['result']) if isinstance(claim.get('result'), str) else claim.get('result') or {}
    for path in sorted(execution.directory.parent.glob('*/outcome.json'), key=lambda path: int(path.parent.name), reverse=True):
        if previous.get('fresh_model') and path.parent != execution.directory:
            continue
        receipt = json.loads(path.read_text())
        job_path = path.parent / 'bundle/job.json'
        if not job_path.is_file() or not receipt.get('published'):
            continue
        job = json.loads(job_path.read_text())
        if job['stage'] != claim['stage'] or job['task'] != task['name']:
            raise ValueError('published receipt belongs to another assignment')
        if claim['stage'] in ('triage', 'review') and not receipt.get('valid'):
            continue
        execution.branch = job['branch']
        execution.phase('AdoptingPublishedOutcome', head=receipt['head'])
        (execution.directory / 'outcome.json').write_text(json.dumps(receipt))
        return receipt | {'branch': execution.branch}
    return None


def prepare(execution, task, prompt):
    """Build a self-contained checkout and trusted input without copying secrets."""
    claim = execution.claim
    real_cli = shutil.which(os.environ.get('OPENSHELL', 'openshell'))
    if not real_cli:
        raise Retry('OpenShell CLI missing; install it before allocating workers')
    branch = f"pipeline/{task['name']}/{claim['id']}-{claim['token']}"
    execution.branch = branch
    bundle = execution.directory / 'bundle'
    bundle.mkdir(exist_ok=True)
    repo = execution.directory / 'checkout'
    repo_url = execution.store.setting('repo_url', 'https://github.com/jsell-rh/gyre.git')
    if repo.exists():
        shutil.rmtree(repo)
    execution.command('git', 'clone', '--quiet', '--no-checkout', repo_url, str(repo), timeout=300)
    for key, value in (('user.name', 'gyre-pipeline'), ('user.email', 'jsell-rh@users.noreply.github.com'), ('core.hooksPath', '/dev/null')):
        execution.command('git', 'config', key, value, cwd=repo)
    seed = claim['input'].get('candidate') or claim['input']['base']
    execution.command('git', 'fetch', '--quiet', 'origin', seed, claim['input']['base'], cwd=repo, timeout=180)
    execution.command('git', 'checkout', '--quiet', '-b', branch, seed, cwd=repo)
    if claim['stage'] == 'implement':
        execution.command('git', 'merge', '--no-edit', claim['input']['base'], cwd=repo, check=False)
    task_path = repo / 'specs/tasks' / (task['name'] + '.md')
    task_path.parent.mkdir(parents=True, exist_ok=True)
    task_path.write_text(task['body'])
    job = {'stage': claim['stage'], 'task': task['name'], 'branch': branch, 'body': task['body'],
           'input': claim['input'], 'repo_url': repo_url, 'checkout': '/sandbox/workspace/checkout',
           'model': 'gateway/rits/zai-org/glm-5-3', 'engine': 'fullsend'}
    (bundle / 'job.json').write_text(json.dumps(job))
    (bundle / 'push-expected').write_text('')
    (bundle / 'prompt.md').write_text(prompt)
    (bundle / 'bin').mkdir(exist_ok=True)
    for name in ('cargo', 'npm', 'npx'):
        shutil.copy(ROOT / 'scripts/dev-build-command.sh', bundle / 'bin' / name)
    for name in ('pipeline-remote.py', 'dev-checkpoint.py', 'dev-review-guard.py', 'dev-attribution.py'):
        shutil.copyfile(ROOT / 'scripts' / name, bundle / name)
    if claim['stage'] in ('review', 'triage'):
        execution.command(sys.executable, str(bundle / 'dev-review-guard.py'), 'snapshot',
                          str(bundle / 'review-before.json'), '--task', task['name'], cwd=repo)
    repair = task['data'].get('repair') or task['data'].get('review_context') or {}
    evidence = repair.get('baseline_log') if repair.get('category') == 'baseline' else repair.get('candidate_log')
    if evidence:
        path = Path(evidence).resolve()
        if path.is_relative_to(execution.store.directory) and path.is_file():
            (bundle / 'findings.log').write_bytes(path.read_bytes()[-65536:])
    total = 0
    sources = [(Path(p), Path('ci-artifacts') / str(i)) for i, p in enumerate(repair.get('artifact_directories', []))]
    for source, relative in sources:
        source = source.resolve()
        if not source.is_relative_to(execution.store.directory) or not source.is_dir():
            continue
        for path in sorted(source.rglob('*')):
            if not path.is_file() or path.suffix not in ('.png', '.md', '.txt'):
                continue
            if not path.resolve().is_relative_to(execution.store.directory):
                raise ValueError('CI artifact escapes private state')
            if total + path.stat().st_size > 32 * 1024 * 1024:
                continue
            target = bundle / relative / path.relative_to(source)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(path, target)
            total += path.stat().st_size
    retained = list(dict.fromkeys(task['data'].get('retained_artifacts', []) + repair.get('stash_artifacts', [])))
    for i, value in enumerate(retained):
        path = Path(value).resolve()
        if not path.is_relative_to(execution.store.directory) or path.stat().st_size > 16 * 1024 * 1024:
            raise ValueError('resume artifact outside private state or too large')
        shutil.copyfile(path, bundle / f'resume-{i}.patch')
    cfg = execution.directory / 'fullsend'
    shutil.copytree(ROOT / '.fullsend', cfg, dirs_exist_ok=True)
    stage = claim['stage']
    harness = {'base': 'worker.yaml', 'agent': 'agents/' + stage + '.md',
               'role': {'implement': 'coder', 'review': 'review', 'triage': 'triage'}[stage],
               'image': execution.store.setting('fullsend_image', json.loads((ROOT / '.fullsend/toolchain.json').read_text())['worker_image']),
               'sandbox_timeout_seconds': 600,
               'timeout_minutes': max(1, (execution.store.setting('model_timeout', 3600) + 59) // 60),
               'host_files': [{'src': str(p), 'dest': '/tmp/stage/' + str(p.relative_to(bundle))}
                              for p in sorted(bundle.rglob('*')) if p.is_file()],
               'env': {'runner': {'GYRE_FULLSEND_CONTEXT': str(execution.directory / 'fullsend-context.json'),
                                  'GYRE_PIPELINE_ROOT': str(ROOT)},
                       'sandbox': {'CARGO_BUILD_JOBS': '2', 'CARGO_TARGET_DIR': '/tmp/gyre-target',
                                   'CARGO_HOME': '/tmp/cargo', 'RUSTUP_HOME': '/usr/local/rustup',
                                   'PLAYWRIGHT_BROWSERS_PATH': '/usr/local/share/gyre-playwright',
                                   'GYRE_DEV_REAL_CARGO': '/usr/local/cargo/bin/cargo',
                                   'GYRE_DEV_REAL_NPM': '/usr/bin/npm', 'GYRE_DEV_REAL_NPX': '/usr/bin/npx'}}}
    if not harness['image']:
        raise Retry('Set fullsend_image to the pushed digest built from docker/dev-worker/Dockerfile')
    (cfg / 'harness' / (stage + '.yaml')).write_text(json.dumps(harness))
    value = {'state': str(execution.store.directory), 'work': claim['id'], 'token': claim['token'],
             'branch': branch, 'gateway': execution.store.setting('gateway', 'gyre-gyre'), 'real_openshell': str(Path(real_cli).resolve())}
    (execution.directory / 'fullsend-context.json').write_text(json.dumps(value))
    shim = execution.directory / 'bin'
    shim.mkdir(exist_ok=True)
    (shim / 'openshell').unlink(missing_ok=True)
    (shim / 'openshell').symlink_to(ROOT / 'scripts/fullsend-openshell.py')
    return cfg, repo, shim


def capture_evidence(execution, sandbox):
    # Preserve the old bounded evidence format. Never copy agent credentials,
    # symlinks, or arbitrary paths out of the sandbox.
    code = """import base64,io,pathlib,tarfile
root=pathlib.Path('/tmp/stage'); output=io.BytesIO(); total=0
with tarfile.open(fileobj=output,mode='w:gz') as archive:
 for relative in ('outcome.json','verdict.json','capabilities.json','review-evidence'):
  path=root/relative
  paths=path.rglob('*') if path.is_dir() else [path]
  for item in paths:
   if not item.is_file() or item.is_symlink() or not item.resolve().is_relative_to(root): continue
   total+=item.stat().st_size
   if total>64*1024*1024: raise RuntimeError('evidence exceeds capture limit')
   archive.add(item,arcname=str(item.relative_to(root)),recursive=False)
print('GYRE_ARTIFACT '+base64.b64encode(output.getvalue()).decode())
"""
    try:
        result = execution.os('sandbox', 'exec', '-n', sandbox, '--no-login-shell', '--',
                              'python3', '-c', code, timeout=120)
        marker = re.search(r'(?m)^GYRE_ARTIFACT ([A-Za-z0-9+/=]+)$', result.stdout)
        if not marker:
            raise ValueError('sandbox evidence capture incomplete')
        artifact = execution.directory / 'evidence.tar.gz'
        artifact.write_bytes(base64.b64decode(marker[1], validate=True))
        artifact.chmod(0o600)
    except Exception as exc:
        execution.store.event('fullsend_evidence_unavailable', {'error': type(exc).__name__}, execution.claim['task'])


def collect(execution, sandbox, success):
    """Capture a published source receipt before fullsend deletes the sandbox."""
    existing = execution.directory / 'outcome.json'
    if existing.exists():
        return json.loads(existing.read_text())
    execution.current()
    job = execution.directory / 'bundle/job.json'
    if not job.exists():
        raise ValueError('fullsend assignment bundle missing')
    # Re-upload trusted checkpoint code after the model has finished.
    for name in ('pipeline-remote.py', 'dev-checkpoint.py', 'dev-review-guard.py', 'dev-attribution.py', 'job.json'):
        source = execution.directory / 'bundle' / name
        execution.os('sandbox', 'upload', sandbox, str(source), '/tmp/stage/' + name, '--no-git-ignore', timeout=120)
    result = execution.os('sandbox', 'exec', '-n', sandbox, '--no-login-shell', '--workdir', '/sandbox/workspace/checkout',
                          '--', 'python3', '/tmp/stage/pipeline-remote.py', 'finalize', '1' if success else '0',
                          timeout=300, check=False)
    with execution.log.open('a') as output:
        output.write(result.stdout + result.stderr)
    outcome = execution.os('sandbox', 'exec', '-n', sandbox, '--no-login-shell', '--', 'cat',
                           '/tmp/stage/outcome.json', check=False)
    if outcome.returncode:
        raise Retry('fullsend worker did not publish a source receipt')
    value = json.loads(outcome.stdout[outcome.stdout.index('{'):])
    if not value.get('published') or value.get('branch') != execution.branch:
        raise Retry('fullsend source publication incomplete or belongs to another branch')
    if not success:
        value.update(valid=False, agent_exit=1, stream_exit=1)
    capture_evidence(execution, sandbox)
    existing.write_text(json.dumps(value))
    return value


def stream_events(execution, stop):
    """Tail raw Pi transcripts into the existing cockpit event protocol."""
    seen = {}
    with execution.log.open('ab') as output:
        parser = subprocess.Popen(['node', str(ROOT / 'scripts/dev-stream.mjs'), execution.claim['stage'],
                                   str(execution.directory / 'response.txt')],
                                  stdin=subprocess.PIPE, stdout=output, stderr=output)
        try:
            while True:
                for path in sorted((execution.directory / 'runs').glob('*/iteration-*/output.jsonl')):
                    with path.open('rb') as source:
                        source.seek(seen.get(str(path), 0))
                        while True:
                            position = source.tell()
                            line = source.readline()
                            if not line or not line.endswith(b'\n'):
                                source.seek(position)
                                break
                            parser.stdin.write(line)
                        seen[str(path)] = source.tell()
                    parser.stdin.flush()
                if stop.wait(.3):
                    return
        except (BrokenPipeError, OSError):
            output.write(b'Gyre: fullsend stream adapter unavailable; raw transcript retained.\n')
            output.flush()
        finally:
            parser.stdin.close()
            try:
                parser.wait(timeout=10)
            except subprocess.TimeoutExpired:
                parser.kill()
                parser.wait()


def cloud_step(execution, task, prompt):
    receipt = adopt(execution, task)
    if receipt:
        return receipt
    if execution.store.setting('admission_paused', False):
        raise Wait('Model admission paused; qualify fullsend before resuming')
    binary(execution.store)
    execution.login()
    from .inference import check, credential
    check(execution.store)
    outstanding = execution.store.db.execute("SELECT name FROM resources WHERE work=? AND state<>'absent'", (execution.claim['id'],)).fetchall()
    if outstanding:
        raise Wait('Previous attempt still owns resources; cleanup must finish first')
    cfg, repo, shim = prepare(execution, task, prompt)
    key = credential()
    github_key = forge_token()
    env = {**os.environ, 'PATH': str(shim) + os.pathsep + os.environ['PATH'],
           'GYRE_FULLSEND_CONTEXT': str(execution.directory / 'fullsend-context.json'),
           'GYRE_PIPELINE_ROOT': str(ROOT), 'FULLSEND_INFERENCE_GATEWAY_API_KEY': key, 'GH_TOKEN': github_key,
           'OPENSHELL_GATEWAY': execution.store.setting('gateway', 'gyre-gyre'),
           'GYRE_OPEN_GATEWAY': execution.store.setting('gateway', 'gyre-gyre'), 'OPENSHELL_WORKSPACE': 'default'}
    return execute(execution, cfg, repo, shim, env)


def execute(execution, cfg, repo, shim, env, runtime='pi'):
    """Shared transport; production always selects Pi, proofs use scripted fixtures."""
    executable = binary(execution.store)
    execution.phase('Fullsend', engine='fullsend', runtime=runtime,
                    model='fixture' if runtime == 'dummy' else 'gateway/rits/zai-org/glm-5-3')
    command = [str(executable), 'run', execution.claim['stage'], '--runtime', runtime, '--forge', 'github',
               '--fullsend-dir', str(cfg), '--target-repo', str(repo), '--output-dir', str(execution.directory / 'runs')]
    process = None
    finished = threading.Event()
    stream = None
    runner_resource = 'runner-' + execution.claim['id'] + '-' + str(execution.claim['token'])
    try:
        with execution.log.open('a') as log:
            process = subprocess.Popen(command, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            stat = Path(f'/proc/{process.pid}/stat').read_text().rsplit(')', 1)[1].split()[19]
            (execution.directory / 'fullsend-process.json').write_text(json.dumps({'pid': process.pid, 'start': stat,
                'boot': Path('/proc/sys/kernel/random/boot_id').read_text().strip()}))
            execution.store.reserve(runner_resource, execution.claim['id'], execution.claim['token'], 'runner', 10000,
                                    {'identity': str(execution.directory / 'fullsend-process.json')})
            stream = threading.Thread(target=stream_events, args=(execution, finished), daemon=True)
            stream.start()
            deadline = time.monotonic() + execution.store.setting('model_timeout', 3600) + 1800
            while process.poll() is None:
                execution.current()
                if time.monotonic() >= deadline:
                    raise Retry('fullsend execution exceeded assignment deadline')
                execution.cancelled.wait(1)
        result = execution.directory / 'outcome.json'
        if result.exists():
            value = json.loads(result.read_text())
            if process.returncode:
                value.update(valid=False, agent_exit=process.returncode, stream_exit=process.returncode)
                result.write_text(json.dumps(value))
            return value
        raise Retry('fullsend ended without a published source receipt; inspect attempt log')
    finally:
        if process and process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=20)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=10)
        finished.set()
        if stream:
            stream.join(timeout=12)
        execution.store.resource_state(runner_resource, 'absent')
        # This is the trusted input checkout, never the only copy of agent output.
        shutil.rmtree(repo, ignore_errors=True)
        # Fullsend normally deletes; unresolved deletion remains charged for
        # the independent cleanup stage. Never declare absence from exit code.
        from .execution import gateway
        try:
            inventory = {item['name']: item for item in gateway.inventory()}
            for row in execution.store.db.execute("SELECT name FROM resources WHERE work=? AND token=? AND kind='sandbox' AND state<>'absent'", (execution.claim['id'], execution.claim['token'])).fetchall():
                name = row['name']
                if name not in inventory:
                    execution.store.resource_state(name, 'absent')
                    continue
                execution.store.resource_state(name, 'deleting')
                if inventory[name]['phase'].lower() != 'deleting':
                    if inventory[name]['phase'].lower() == 'ready' and not (execution.directory / 'outcome.json').exists():
                        try:
                            collect(execution, name, success=False)
                        except Exception as exc:
                            execution.store.event('fullsend_checkpoint_unavailable',
                                {'resource': name, 'error': type(exc).__name__}, execution.claim['task'])
                    execution.os('sandbox', 'delete', name, check=False, timeout=60)
        except Exception as exc:
            execution.store.event('cleanup_observation_unavailable', {'error': type(exc).__name__}, execution.claim['task'])
