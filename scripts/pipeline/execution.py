"""Shared execution mechanics: heartbeat, transport, logs, and cleanup."""
from contextlib import contextmanager
import base64
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import tarfile
import threading
import time

from .catalog import helper
from .store import Store, StaleClaim

ROOT = Path(__file__).resolve().parents[2]
gateway = helper('dev-gateway-job')


class Retry(RuntimeError):
    def __init__(self, message, fresh_model=False):
        super().__init__(message)
        self.fresh_model = fresh_model


class Wait(Retry):
    """An expected external observation is pending, rather than failing."""
    retry_delay = 30


class Execution:
    def __init__(self, store, claim):
        self.store, self.claim = store, claim
        self.directory = store.directory / 'attempts' / claim['id'] / str(claim['token'])
        self.directory.mkdir(parents=True, exist_ok=True)
        self.directory.chmod(0o700)
        self.log = self.directory / 'output.log'
        self.cancelled = threading.Event()
        self.stop = threading.Event()

    def current(self):
        if self.cancelled.is_set():
            raise StaleClaim(self.claim['id'])
        self.store._current(self.claim['id'], self.claim['token'])

    @contextmanager
    def heartbeat(self):
        def renew():
            connection = Store(self.store.directory)
            try:
                while not self.stop.wait(15):
                    try:
                        connection.heartbeat(self.claim['id'], self.claim['token'])
                    except Exception:
                        self.cancelled.set()
                        return
            finally:
                connection.close()
        thread = threading.Thread(target=renew, daemon=True)
        thread.start()
        try:
            yield self
        finally:
            self.stop.set()
            thread.join(timeout=20)

    def command(self, *args, cwd=None, timeout=120, check=True, input=None, capture=True):
        self.current()
        result = subprocess.run(args, cwd=cwd, input=input, capture_output=capture,
                                text=True, timeout=timeout)
        if check and result.returncode:
            raise RuntimeError((result.stderr or result.stdout or '')[-4000:])
        return result

    def phase(self, name, **detail):
        (self.directory / 'phase.json').write_text(json.dumps({'phase': name, 'at': time.time(), **detail}))
        with self.store.transaction():
            self.store.event('phase', {'phase': name, **detail}, self.claim['task'], self.claim['id'])

    def login(self):
        os.environ.update(OPENSHELL_NO_BROWSER='1', OPENSHELL_GATEWAY_INSECURE='true',
                          OPENSHELL_WORKSPACE='default')
        gateway.login(self.store.directory)

    def os(self, *args, **kwargs):
        check = kwargs.pop('check', True)
        command = (os.environ.get('OPENSHELL', 'openshell'), '-g', 'gyre-gyre', *args)
        result = self.command(*command, check=False, **kwargs)
        diagnostic = (result.stderr or '') + (result.stdout or '')
        if result.returncode and ('cached OIDC token has expired' in diagnostic or
                                  'OIDC token refresh failed' in diagnostic):
            self.login()
            result = self.command(*command, check=False, **kwargs)
        if check and result.returncode:
            raise RuntimeError((result.stderr or result.stdout or '')[-4000:])
        return result

    def remote(self, sandbox, *args, **kwargs):
        return self.os('sandbox', 'exec', '-n', sandbox, '--no-login-shell', '--workdir', '/tmp',
                       '--env', 'HOME=/tmp', '--', *args, **kwargs)

    def wait_ready(self, sandbox):
        delay = 5
        deadline = time.monotonic() + 1800
        while True:
            self.current()
            item = next((item for item in gateway.inventory() if item['name'] == sandbox), None)
            phase = item['phase'].lower() if item else 'unknown'
            if phase == 'ready':
                return
            if phase in ('error', 'failed', 'stopped', 'terminated', 'deleted'):
                self.phase('InfrastructureFailed', sandbox=sandbox, gateway_phase=item['phase'])
                raise Retry(f"sandbox entered terminal infrastructure phase {item['phase']}; purge and retry with backoff")
            if time.monotonic() >= deadline:
                raise Retry('sandbox readiness exceeded autoscaler deadline')
            self.phase('WaitingForInfrastructure', sandbox=sandbox, retry_after=delay)
            self.cancelled.wait(delay)
            delay = min(60, delay * 2)

    def cloud_step(self, task, prompt):
        claim = self.claim
        # A driver can disappear after publication and purge, before the SQL
        # outcome is recorded. Adopt its immutable receipt without new compute.
        for prior in sorted(self.directory.parent.glob('*/outcome.json'), reverse=True):
            previous_failure = json.loads(claim['result']) if isinstance(claim.get('result'), str) else claim.get('result') or {}
            if previous_failure.get('fresh_model') and prior.parent != self.directory:
                continue
            receipt = json.loads(prior.read_text())
            job_path = prior.parent / 'bundle' / 'job.json'
            if not job_path.exists() or not receipt.get('published'):
                continue
            if claim['stage'] in ('review', 'triage') and not receipt.get('valid'):
                # A failed model step is a new assignment on retry. Replaying
                # its failed outcome indefinitely would never make progress.
                continue
            old_job = json.loads(job_path.read_text())
            if old_job['stage'] != claim['stage'] or old_job['task'] != task['name']:
                raise ValueError('published receipt belongs to another assignment')
            self.phase('AdoptingPublishedOutcome', head=receipt['head'])
            self.branch = old_job['branch']
            (self.directory / 'outcome.json').write_text(json.dumps(receipt))
            with self.log.open('a') as output:
                output.write(f'Adopted published outcome from attempt {prior.parent.name}; no model rerun.\n')
            return receipt | {'branch': self.branch}
        previous = self.store.db.execute("SELECT * FROM resources WHERE work=? AND kind='sandbox' AND state IN ('intent','present') ORDER BY updated DESC LIMIT 1", (claim['id'],)).fetchone()
        logical_token = json.loads(previous['data']).get('logical_token', previous['token']) if previous else claim['token']
        sandbox = previous['name'] if previous else 'gp-' + claim['id'][:16]
        if len(sandbox) > 19:
            raise ValueError('sandbox name exceeds gateway limit')
        limit = self.store.setting('slots', 8)
        occupied = self.store.db.execute("SELECT count(*) FROM resources WHERE kind='sandbox' AND state<>'absent'").fetchone()[0]
        if not previous and occupied >= limit:
            self.phase('WaitingForCapacity', reason='global sandbox capacity is occupied')
            raise Retry('global sandbox capacity is occupied')
        self.login()
        providers = self.os('provider', 'list', '--names', timeout=30, check=False)
        missing = {'gyre-enmaas', 'gyre-github-rw'} - set(providers.stdout.splitlines())
        if providers.returncode or missing:
            reason = 'provider check unavailable' if providers.returncode else 'missing providers: ' + ', '.join(sorted(missing))
            self.store.set_setting('admission_condition', reason)
            self.phase('WaitingForConfiguration', reason=reason)
            raise Retry(reason)
        self.store.set_setting('admission_condition', 'Ready')
        inventory = gateway.inventory()
        observed = {item['name'] for item in inventory}
        item = next((item for item in inventory if item['name'] == sandbox), None)
        if item and (item['labels'].get('gyre.dev/work') != claim['id'] or
                     item['labels'].get('gyre.dev/pipeline') != self.store.setting('owner')):
            raise ValueError('sandbox name belongs to another actor')
        # During cutover, old workers still occupy the same compute pool.
        # Account for them without adopting or deleting another actor's pod.
        with self.store.transaction():
            for item in inventory:
                if not any(key in item['labels'] for key in ('gyre.dev/controller', 'gyre.dev/pipeline')):
                    continue
                self.store.db.execute('''INSERT OR IGNORE INTO resources
                    VALUES(?,NULL,NULL,'sandbox','present',?,?)''',
                    (item['name'], json.dumps({'observed_external': True, 'labels': item['labels']}), time.time()))
            for row in self.store.db.execute("SELECT name FROM resources WHERE work IS NULL AND state<>'absent'").fetchall():
                if row['name'] not in observed:
                    self.store.db.execute("UPDATE resources SET state='absent' WHERE name=?", (row['name'],))
        if not self.store.reserve(sandbox, claim['id'], claim['token'], 'sandbox', limit,
                                  {'logical_token': logical_token}):
            self.phase('WaitingForCapacity', reason='global sandbox capacity is occupied')
            raise Retry('global sandbox capacity is occupied')
        branch = f"pipeline/{task['name']}/{claim['id']}-{logical_token}"
        self.branch = branch
        job = {'stage': claim['stage'], 'task': task['name'], 'branch': branch,
               'input': claim['input'], 'body': task['body'],
               'timeout': self.store.setting('model_timeout', 3600),
               'repo_url': self.store.setting('repo_url', 'https://github.com/jsell-rh/gyre.git'),
               'model': self.store.setting('model', 'enmaas-glm-5-3/rits/zai-org/glm-5-3')}
        bundle = self.directory / 'bundle'
        bundle.mkdir(exist_ok=True)
        (bundle / 'job.json').write_text(json.dumps(job))
        (bundle / 'prompt.md').write_text(prompt)
        repair = task['data'].get('repair') or (
            task['data'].get('review_context') if claim['stage'] == 'review' else None) or {}
        evidence_log = repair.get('baseline_log') if repair.get('category') == 'baseline' else repair.get('candidate_log')
        if evidence_log:
            source = Path(evidence_log).resolve()
            if source.is_relative_to(self.store.directory) and source.is_file():
                (bundle / 'findings.log').write_bytes(source.read_bytes()[-65536:])
                with (bundle / 'prompt.md').open('a') as output:
                    output.write('\nCurrent failure evidence is available at /tmp/stage/findings.log. Host paths in findings are provenance and are not accessible inside this sandbox.\n')
        retained_bytes, retained_files = 0, 0
        for directory in repair.get('artifact_directories', []):
            source = Path(directory).resolve()
            if not source.is_relative_to(self.store.directory) or not source.is_dir():
                continue
            for path in sorted(source.rglob('*')):
                if not path.is_file() or path.suffix not in ('.png', '.md', '.txt'):
                    continue
                if not path.resolve().is_relative_to(self.store.directory):
                    raise ValueError('CI artifact escapes private state')
                size = path.stat().st_size
                if retained_files >= 256 or retained_bytes + size > 32 * 1024 * 1024:
                    continue
                target = bundle / 'ci-artifacts' / source.name / path.relative_to(source)
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(path.read_bytes())
                retained_bytes += size
                retained_files += 1
        if retained_files:
            with (bundle / 'prompt.md').open('a') as output:
                output.write(f'\n{retained_files} current CI artifact files are available under /tmp/stage/ci-artifacts. Inspect expected/actual/diff images and error contexts before changing visual baselines.\n')
        artifacts = list(dict.fromkeys((task['data'].get('retained_artifacts') or []) +
                                      (task['data'].get('repair') or {}).get('stash_artifacts', [])))
        for index, value in enumerate(artifacts):
            source = Path(value).resolve()
            if not source.is_relative_to(self.store.directory) or source.stat().st_size > 16 * 1024 * 1024:
                raise ValueError('resume artifact is outside private state or exceeds size limit')
            name = f'resume-{index}.patch'
            (bundle / name).write_bytes(source.read_bytes())
            with (bundle / 'prompt.md').open('a') as output:
                output.write(f'\nRetained source from an interrupted stash is available at /tmp/stage/{name}. Inspect and reconcile it with the assigned checkpoint before implementation.\n')
        names = ('pipeline-remote.py', 'pipeline-attach.py', 'pipeline-transport.py', 'dev-process.sh',
                 'dev-stream.mjs', 'dev-review-guard.py', 'dev-checkpoint.py', 'dev-attribution.py',
                 'dev-build-command.sh')
        for name in names:
            (bundle / name).write_bytes((ROOT / 'scripts' / name).read_bytes())
            (bundle / name).chmod((ROOT / 'scripts' / name).stat().st_mode & 0o777)
        for name in ('models.yml', 'config.yml'):
            (bundle / name).write_bytes((ROOT / 'docker/dev-worker' / name).read_bytes())
        archive = self.directory / 'bundle.tar'
        with tarfile.open(archive, 'w') as tar:
            for path in bundle.iterdir():
                tar.add(path, arcname=path.name)
        allocated = False
        ready = False
        completed = False
        try:
            allocated = any(item['name'] == sandbox for item in inventory)
            if not allocated:
                self.phase('Provisioning', sandbox=sandbox)
                result = self.os('sandbox', 'create', '--name', sandbox, '--from',
                                 self.store.setting('image', 'ghcr.io/jsell-rh/gyre-worker@sha256:a3856204f3b23b3694564ab34fa76d588f2d09ea39060a71967e5ab645c2fa7d'),
                                 '--provider', 'gyre-enmaas', '--provider', 'gyre-github-rw',
                                 '--label', 'gyre.dev/pipeline=' + self.store.setting('owner', 'gyre'),
                                 '--label', 'gyre.dev/work=' + claim['id'],
                                 '--policy', str(ROOT / 'docker/dev-worker/policy.yaml'),
                                 '--detach', '--', 'bash', '-c', 'while true; do sleep 3600; done',
                                 timeout=600, check=False)
                with self.log.open('a') as log:
                    log.write(result.stdout + result.stderr)
                # A failed response may have created compute. Adopt by name.
                allocated = any(item['name'] == sandbox for item in gateway.inventory())
                if not allocated:
                    raise Retry('sandbox creation not observed: ' + (result.stderr or result.stdout)[-2000:])
            self.store.resource_state(sandbox, 'present')
            self.wait_ready(sandbox)
            ready = True
            self.phase('Staging', sandbox=sandbox)
            cli = os.environ.get('OPENSHELL', 'openshell')
            persistent = self.remote(sandbox, 'test', '-f', '/tmp/stage/step.intent', check=False)
            if persistent.returncode != 0:
                with archive.open('rb') as source:
                    result = subprocess.run([cli, '-g', 'gyre-gyre', 'sandbox', 'exec', '-n', sandbox,
                                             '--no-login-shell', '--workdir', '/tmp', '--', 'bash', '-c',
                                             'mkdir -p /tmp/stage && tar -C /tmp/stage -xf -'],
                                            stdin=source, capture_output=True, timeout=120)
                if result.returncode:
                    raise Retry('sandbox staging failed: ' + result.stderr.decode(errors='replace')[-2000:])
            self.phase('Running', sandbox=sandbox)
            offset = 0
            for reconnect in range(20):
                self.current()
                with self.log.open('ab', buffering=0) as log:
                    process = subprocess.Popen([cli, '-g', 'gyre-gyre', 'sandbox', 'exec', '-n', sandbox,
                                                '--no-login-shell', '--workdir', '/tmp', '--',
                                                'python3', '/tmp/stage/pipeline-attach.py', str(offset)],
                                               stdout=subprocess.PIPE, stderr=log, start_new_session=True)
                    attachment_done = threading.Event()
                    def bound_attachment(proc=process, done=attachment_done):
                        deadline = time.monotonic() + job['timeout'] + 1800
                        while not done.wait(1):
                            if self.cancelled.is_set() or time.monotonic() >= deadline:
                                if proc.poll() is None:
                                    try:
                                        os.killpg(proc.pid, signal.SIGTERM)
                                        if not done.wait(5) and proc.poll() is None:
                                            os.killpg(proc.pid, signal.SIGKILL)
                                    except ProcessLookupError:
                                        pass
                                return
                    monitor = threading.Thread(target=bound_attachment, daemon=True)
                    monitor.start()
                    # Read by line so the dashboard sees each event immediately.
                    try:
                        for line in process.stdout:
                            marker = re.fullmatch(rb'GYRE_REMOTE_LOG_OFFSET (\d+)\r?\n', line)
                            if marker:
                                offset = int(marker[1])
                                (self.directory / 'remote.offset').write_text(str(offset))
                            else:
                                log.write(line)
                        process.wait(timeout=30)
                    finally:
                        attachment_done.set()
                        monitor.join(timeout=2)
                outcome = self.remote(sandbox, 'cat', '/tmp/stage/outcome.json', check=False)
                if outcome.returncode == 0:
                    start = re.search(r'(?m)^\s*(?=\{)', outcome.stdout)
                    if not start:
                        raise Retry('remote receipt was not a JSON document')
                    receipt = json.loads(outcome.stdout[start.start():])
                    if receipt.get('published'):
                        (self.directory / 'outcome.json').write_text(json.dumps(receipt))
                        completed = True
                        return receipt | {'branch': branch}
                terminal = self.remote(sandbox, 'cat', '/tmp/stage/step.exit', check=False)
                if terminal.returncode == 0:
                    raise Retry('remote step terminated before publishing its checkpoint')
                self.cancelled.wait(min(60, 5 * 2 ** min(reconnect, 4)))
            raise Retry('remote attachment unavailable after reconnects')
        finally:
            # A new claimant can adopt this exact persistent job. The late
            # driver must not stop, stage over, or delete its resource.
            self.current()
            # Retain logs and source before releasing expensive compute. A
            # separate cleanup reconciler handles crashes and failed deletes.
            self.phase('Deleting', sandbox=sandbox)
            if allocated and ready:
                if not completed:
                    try:
                        code = """import json,os,pathlib,signal,time
p=pathlib.Path('/tmp/stage/step.exit.process.json')
if p.exists():
 value=json.loads(p.read_text()); pid=int(value['pid']); stat=pathlib.Path(f'/proc/{pid}/stat')
 if stat.exists() and stat.read_text().rsplit(')',1)[1].split()[19]==value['start'] and pathlib.Path('/proc/sys/kernel/random/boot_id').read_text().strip()==value['boot']:
  os.killpg(pid,signal.SIGTERM)
  deadline=time.monotonic()+10
  while stat.exists() and time.monotonic()<deadline: time.sleep(.1)
  if stat.exists(): os.killpg(pid,signal.SIGKILL)
"""
                        self.remote(sandbox, 'python3', '-c', code, timeout=30)
                    except Exception as exc:
                        with self.log.open('a') as log:
                            log.write(f'worker stop unavailable before capture: {exc}\n')
                try:
                    code = """import base64,io,pathlib,tarfile
root=pathlib.Path('/tmp/stage'); output=io.BytesIO(); total=0
with tarfile.open(fileobj=output,mode='w:gz') as archive:
 for relative in ('outcome.json','response.txt','verdict.json','capabilities.json','review-evidence'):
  path=root/relative
  paths=path.rglob('*') if path.is_dir() else [path]
  for item in paths:
   if not item.is_file() or item.is_symlink() or not item.resolve().is_relative_to(root): continue
   size=item.stat().st_size; total+=size
   if total>64*1024*1024: raise RuntimeError('evidence exceeds capture limit')
   archive.add(item,arcname=str(item.relative_to(root)),recursive=False)
print('GYRE_ARTIFACT '+base64.b64encode(output.getvalue()).decode())
"""
                    result = self.remote(sandbox, 'python3', '-c', code, timeout=120)
                    marker = re.search(r'(?m)^GYRE_ARTIFACT ([A-Za-z0-9+/=]+)$', result.stdout)
                    if not marker:
                        raise ValueError('sandbox evidence capture was incomplete')
                    artifact = self.directory / 'evidence.tar.gz'
                    artifact.write_bytes(base64.b64decode(marker[1], validate=True))
                    artifact.chmod(0o600)
                except Exception as exc:
                    with self.log.open('a') as log:
                        log.write(f'evidence capture unavailable: {exc}\n')
                # Capture uses its CLI to keep credentials out of artifacts.
                try:
                    subprocess.run(['python3', str(ROOT / 'scripts/dev-recover.py'), sandbox,
                                    str(self.directory / 'recovery.patch')], timeout=180, check=False)
                except Exception as exc:
                    with self.log.open('a') as log:
                        log.write(f'checkpoint capture unavailable: {exc}\n')
            self.store.resource_state(sandbox, 'deleting')
            try:
                self.login()
                result = self.os('sandbox', 'delete', sandbox, timeout=180, check=False)
                if result.returncode == 0 and not any(item['name'] == sandbox for item in gateway.inventory()):
                    self.store.resource_state(sandbox, 'absent')
            except Exception as exc:
                with self.log.open('a') as log:
                    log.write(f'cleanup deferred: {exc}\n')
