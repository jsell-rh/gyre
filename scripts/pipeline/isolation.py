"""Instance-scoped OpenShell registration and persistent forge provider."""
import hashlib
import fcntl
import json
import os
from pathlib import Path
import subprocess
import time


def github_provider(store):
    return 'gyre-gh-' + hashlib.sha256(store.setting('owner').encode()).hexdigest()[:10]


def isolate_gateway(store):
    """Snapshot registration metadata without sharing refresh tokens or selecting a default.

    OpenShell stores metadata and OIDC tokens under gateways/<local name>.
    `gateway add` also changes active_gateway, so write only the new registration;
    its first login mints its own token using Gyre's existing client credential.
    """
    source = store.setting('gateway', 'gyre-gyre')
    previous = store.setting('dispatcher_gateway_registration')
    if previous and source == previous['alias']:
        return source
    alias = 'gyre-d-' + hashlib.sha256(store.setting('owner').encode()).hexdigest()[:10]
    root = Path(os.environ.get('XDG_CONFIG_HOME', str(Path.home()/'.config')))/'openshell/gateways'
    metadata = json.loads((root/source/'metadata.json').read_text())
    if metadata.get('auth_mode') != 'oidc':
        raise ValueError('Gyre dispatcher requires its own OIDC gateway registration')
    metadata['name'] = alias
    directory = root/alias
    directory.mkdir(mode=0o700, exist_ok=True)
    destination = directory/'metadata.json'
    if destination.exists():
        if json.loads(destination.read_text()) != metadata:
            raise ValueError('instance gateway registration already points at different connection metadata')
    else:
        with destination.open('x') as output:
            destination.chmod(0o600)
            output.write(json.dumps(metadata, indent=2)+'\n')
    return alias


def provider_files(store, directory):
    name = github_provider(store)
    profiles, providers = directory/'profiles', directory/'providers'
    profiles.mkdir(parents=True, exist_ok=True)
    providers.mkdir(parents=True, exist_ok=True)
    template = Path(__file__).resolve().parents[2]/'docker/dev-worker/github.yaml'
    profile = profiles/(name+'.yaml')
    text = template.read_text()
    if not text.startswith('id: gyre-github-custom\n'):
        raise ValueError('unexpected GitHub profile template identity')
    profile.write_text(text.replace('id: gyre-github-custom\n','id: '+name+'\n',1))
    (providers/(name+'.yaml')).write_text(json.dumps({'name':name,'type':name,'credentials':{'GH_TOKEN':'${GH_TOKEN}'}}))
    return name, profile


def ensure_github_provider(store, cli=None):
    from .fullsend import forge_token
    secret=forge_token()
    fingerprint=hashlib.sha256(secret.encode()).hexdigest()
    with (store.directory/'github-provider.lock').open('a') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        if (time.time()-store.setting('dispatcher_provider_at',0)<300 and
                store.setting('dispatcher_provider_fingerprint') == fingerprint):
            return
        name, profile = provider_files(store, store.directory/'dispatcher')
        env = {**os.environ, 'GH_TOKEN':secret, 'RUST_LOG':'error'}
        command = [cli or os.environ.get('OPENSHELL','openshell'),'-g',store.setting('gateway')]
        def call(*args):
            return subprocess.run([*command,*args],env=env,capture_output=True,text=True,timeout=30)
        existing = call('provider','profile','export',name)
        if existing.returncode:
            result = call('provider','profile','import','--file',str(profile))
            if result.returncode:
                raise RuntimeError('could not import the instance GitHub provider profile')
        existing=call('provider','get',name)
        if (existing.returncode and store.setting('dispatcher_provider_fingerprint')==fingerprint
                and 'not found' not in (existing.stdout+existing.stderr).lower()):
            raise RuntimeError('could not observe the instance GitHub provider; refusing an unnecessary credential update')
        if not existing.returncode and store.setting('dispatcher_provider_fingerprint') == fingerprint:
            # Rewriting identical credentials changes the provider generation
            # seen by every attached sandbox. A health check must not do that.
            store.set_setting('dispatcher_provider_at',time.time())
            return
        result = call('provider','create','--name',name,'--type',name,'--credential','GH_TOKEN')
        if result.returncode:
            if 'already exists' not in (result.stdout+result.stderr).lower():
                raise RuntimeError('could not create the instance GitHub provider')
            result = call('provider','update',name,'--credential','GH_TOKEN')
            if result.returncode:
                raise RuntimeError('could not refresh the instance GitHub provider')
        if store.setting('verification_provider') in (None,'github'):
            store.set_setting('verification_provider',name)
        store.set_setting('dispatcher_provider_fingerprint',fingerprint)
        store.set_setting('dispatcher_provider_at',time.time())
