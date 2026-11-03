"""Shared execution mechanics: heartbeat, transport, logs, and cleanup."""
from contextlib import contextmanager
import json
import os
from pathlib import Path
import subprocess
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
                          OPENSHELL_WORKSPACE='default',
                          GYRE_OPEN_GATEWAY=self.store.setting('gateway', 'gyre-gyre'))
        gateway.login(self.store.directory)

    def os(self, *args, **kwargs):
        check = kwargs.pop('check', True)
        command = (os.environ.get('OPENSHELL', 'openshell'), '-g', self.store.setting('gateway', 'gyre-gyre'), *args)
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
        from .fullsend import cloud_step
        return cloud_step(self, task, prompt)
