#!/usr/bin/python3
"""Claim-aware boundary for fullsend's real OpenShell subprocesses."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from pipeline.fullsend import context, collect
from pipeline.execution import gateway


def option(args, name):
    for i, value in enumerate(args):
        if value == name and i + 1 < len(args):
            return args[i + 1]
        if value.startswith(name + '='):
            return value.split('=', 1)[1]
    return None


def main():
    execution = context(os.environ['GYRE_FULLSEND_CONTEXT'])
    store = execution.store
    config = json.loads((execution.directory / 'fullsend-context.json').read_text())
    registration = store.setting('dispatcher_gateway_registration') or {}
    bound_gateway = (registration['alias'] if config['gateway'] == registration.get('source') else config['gateway'])
    args = sys.argv[1:]
    # Controller hooks also call this shim through Execution.os, which already
    # supplies the gateway. Consume matching selectors rather than duplicating
    # the global flag or allowing a hook to target another gateway.
    while args and args[0] in ('-g', '--gateway', '--workspace'):
        if len(args) < 2:
            raise ValueError('missing OpenShell scope value')
        expected = {'default'} if args[0] == '--workspace' else {config['gateway'], bound_gateway}
        if args[1] not in expected:
            raise ValueError('OpenShell scope differs from the assigned gateway')
        args = args[2:]
    env = {**os.environ, 'OPENSHELL': config['real_openshell'], 'OPENSHELL_GATEWAY': bound_gateway, 'RUST_LOG': 'error'}
    # Refresh the private alias even for a worker started before the cutover.
    # Changing a shim process's environment cannot retarget its parent or HyperShell.
    os.environ['GYRE_OPEN_GATEWAY'] = bound_gateway
    def refresh(force=False):
        previous=os.environ.get('OPENSHELL');os.environ['OPENSHELL']=config['real_openshell']
        try:gateway.login(store.directory,force=force,max_age=120)
        finally:
            if previous is None:os.environ.pop('OPENSHELL',None)
            else:os.environ['OPENSHELL']=previous
    def invoke(arguments, **kwargs):
        result=subprocess.run([config['real_openshell'], '-g', bound_gateway, *arguments], env=env, **kwargs)
        if kwargs.get('capture_output') and result.returncode and any(marker in ((result.stderr or '')+(result.stdout or '')) for marker in ('cached OIDC token has expired','OIDC token refresh failed','Unauthenticated','token is expired')):
            refresh(force=True)
            result=subprocess.run([config['real_openshell'], '-g', bound_gateway, *arguments], env=env, **kwargs)
        return result
    from pipeline.isolation import github_provider, ensure_github_provider
    persistent=github_provider(store)
    shared_create=['provider','create','--name',persistent,'--type',persistent,'--credential','GH_TOKEN']
    shared_update=['provider','update',persistent,'--credential','GH_TOKEN']
    if args in (shared_create,shared_update):
        ensure_github_provider(store,cli=config['real_openshell'])
        print('Instance GitHub provider ready (unchanged credentials are retained)')
        store.close()
        return 0
    from pipeline.agent_transport import pi_command, stream
    command=pi_command(args) if config.get('durable_agent') else None
    if command:
        name=option(args,'--name')
        if not name:raise ValueError('agent transport requires its assigned sandbox')
        row=store.db.execute('SELECT * FROM resources WHERE name=?',(name,)).fetchone()
        if not row or row['work']!=execution.claim['id'] or row['token']!=execution.claim['token']:
            raise ValueError('agent transport sandbox belongs to another claim')
        timeout=min(int(option(args,'--timeout') or 14400),store.setting('model_timeout',14400))
        return stream(execution,name,command,timeout,invoke)
    provider_create = args[:2] == ['provider', 'create']
    provider_delete = args[:2] == ['provider', 'delete']
    provider_name = option(args, '--name') if provider_create else (args[2] if provider_delete and len(args) > 2 else None)
    owned_provider = bool(provider_name and provider_name.startswith('inference-gateway-'))
    if provider_delete and owned_provider:
        record = store.db.execute('SELECT * FROM resources WHERE name=?', (provider_name,)).fetchone()
        if not record or record['work'] != execution.claim['id'] or record['token'] != execution.claim['token']:
            raise ValueError('refuse deletion of another claim’s run-scoped provider')
    if provider_create and owned_provider:
        if not store.reserve(provider_name, execution.claim['id'], execution.claim['token'], 'provider', 100000, {'engine': 'fullsend'}):
            raise RuntimeError('provider admission unavailable')
    create = args[:2] == ['sandbox', 'create']
    delete = args[:2] == ['sandbox', 'delete']
    name = option(args, '--name') or (args[2] if delete and len(args) > 2 else None)
    if create:
        if not name or not re.fullmatch(r'fs-[a-z0-9]{3}-[a-f0-9]{12}', name):
            raise ValueError('unexpected fullsend sandbox identity')
        execution.current()
        if store.setting(f"cancel:{execution.claim['id']}:{execution.claim['token']}", False):
            raise RuntimeError('assignment canceled before sandbox allocation')
        old = store.db.execute('SELECT * FROM resources WHERE name=?', (name,)).fetchone()
        if old and old['state'] == 'absent':
            raise RuntimeError('fullsend attempted to recreate a deleted sandbox in the same assignment; retry through the controller')
        if old and old['work']==execution.claim['id'] and old['token']==execution.claim['token']:
            observed=invoke(['sandbox','get',name,'--output','json'],capture_output=True,text=True,timeout=30)
            if observed.returncode==0:
                start=re.search(r'(?m)^\s*(?=\{)',observed.stdout)
                value=json.loads(observed.stdout[start.start():]) if start else {}
                labels=value.get('labels',{})
                if labels.get('gyre.dev/pipeline')!=store.setting('owner') or labels.get('gyre.dev/work')!=execution.claim['id']:
                    raise ValueError('existing sandbox has different ownership labels')
                phase=value.get('phase','').lower()
                if phase in ('ready','pending','provisioning','creating'):
                    # Fullsend retries create after a readiness timeout. Adopt
                    # the same allocation and let its next readiness poll wait;
                    # a duplicate create would fail immediately and discard it.
                    print('Created sandbox: '+name)
                    store.close()
                    return 0
                raise RuntimeError('existing sandbox reached terminal infrastructure state; cleanup before retry')
        if not store.reserve(name, execution.claim['id'], execution.claim['token'], 'sandbox', store.setting('slots', 8),
                             {'engine': 'fullsend', 'logical_token': execution.claim['token'], 'repo': '/sandbox/workspace/checkout'}):
            raise RuntimeError('global sandbox capacity is occupied')
        from pipeline.compute import assigned_profile
        budget = assigned_profile(store, execution.claim)
        # Never inherit the gateway's 1 GiB / half-CPU defaults for build work.
        for flag in ('--cpu','--memory'):
            while flag in args:
                index=args.index(flag); del args[index:index+2]
            args=[a for a in args if not a.startswith(flag+'=')]
        position=args.index('--') if '--' in args else len(args)
        args[position:position]=['--cpu',budget['cpu'],'--memory',budget['memory']]
        record=store.db.execute('SELECT data,state FROM resources WHERE name=?',(name,)).fetchone()
        store.resource_state(name,record['state'],json.loads(record['data']) | {'compute':budget})
        args = [arg for arg in args if arg != '--keep']
        separator = args.index('--') if '--' in args else len(args)
        args[separator:separator] = ['--label', 'gyre.dev/pipeline=' + store.setting('owner'),
                                    '--label', 'gyre.dev/work=' + execution.claim['id']]
        execution.phase('Provisioning', sandbox=name, engine='fullsend')
    if delete:
        # Fullsend retries readiness internally. Keep a pending autoscaler
        # allocation across those polls; the outer reconciler owns terminal
        # cleanup once the bounded fullsend attempt ends.
        observed = invoke(['sandbox', 'get', name, '--output', 'json'], capture_output=True, text=True, timeout=30)
        if observed.returncode == 0:
            start = re.search(r'(?m)^\s*(?=\{)', observed.stdout)
            value = json.loads(observed.stdout[start.start():]) if start else {}
            if value.get('phase', '').lower() in ('pending', 'provisioning', 'creating'):
                print('gyre: waiting for existing infrastructure; retry without recreating sandbox', file=sys.stderr)
                return 75
        row = store.db.execute('SELECT * FROM resources WHERE name=?', (name,)).fetchone()
        if not row or row['work'] != execution.claim['id'] or row['token'] != execution.claim['token']:
            raise ValueError('refuse deletion of an unowned fullsend sandbox')
        # On cancellation/failed inference the normal validator may not run.
        # Preserve an explicitly incomplete checkpoint before deleting compute.
        if execution.claim['stage']=='implement' and not (execution.directory/'outcome.json').exists():
            try:
                collect(execution, name, success=False)
            except Exception as exc:
                store.event('fullsend_checkpoint_unavailable', {'resource': name, 'error': type(exc).__name__}, execution.claim['task'])
                # Keep existing recovery format for the independent cleanup stage.
                recover_env = {**env, 'GYRE_DEV_RECOVERY_REPO': '/sandbox/workspace/checkout'}
                subprocess.run([sys.executable, str(Path(__file__).with_name('dev-recover.py')), name,
                                str(execution.directory / 'recovery.patch')], env=recover_env,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=180)
        store.resource_state(name, 'deleting')
    # Streaming exec/upload/download must inherit stdin/stdout. Retry bounded
    # control-plane calls on the precise CLI token-expiry diagnostic only.
    control = args and args[0] in ('gateway', 'provider', 'profile', 'settings') or create or delete or args[:2] in (['sandbox', 'get'], ['sandbox', 'list'])
    if control:
        result = invoke(args, capture_output=True, text=True, timeout=600 if create else 180)
        output = result.stdout + result.stderr
        if 'cached OIDC token has expired' in output or 'OIDC token refresh failed' in output:
            previous = os.environ.get('OPENSHELL')
            os.environ['OPENSHELL'] = config['real_openshell']
            try:
                gateway.login(store.directory,force=True)
            finally:
                if previous is None: os.environ.pop('OPENSHELL', None)
                else: os.environ['OPENSHELL'] = previous
            if result.returncode:
                result = invoke(args, capture_output=True, text=True, timeout=600 if create else 180)
            else:
                # A successful mutation must never be replayed. Remove only
                # the CLI's cached-token warning after refreshing future calls.
                result.stderr = '\n'.join(line for line in result.stderr.splitlines()
                    if 'OIDC token refresh failed' not in line)
        sys.stdout.write(result.stdout)
        sys.stderr.write(result.stderr)
    else:
        # Upload/bootstrap exec replaces this process to preserve streams. Refresh
        # before that boundary rather than discovering expiry after allocation and
        # discarding a perfectly good sandbox. The shared login lock coalesces it.
        import time
        if time.time()-gateway.auth_status(store.directory,bound_gateway).get('refreshed_at',0)>120:
            refresh()
        # Fullsend's deadline must kill the actual streaming CLI, not a
        # Python parent whose orphan keeps its stdout pipe open forever.
        store.close()
        os.execvpe(config['real_openshell'],
                   [config['real_openshell'], '-g', bound_gateway, *args], env)
    if provider_create and owned_provider and result.returncode == 0:
        store.resource_state(provider_name, 'present')
    if provider_delete and owned_provider and result.returncode == 0:
        # Retain until an authoritative provider inventory confirms absence.
        store.resource_state(provider_name, 'deleting')
    if create and result.returncode == 0:
        store.resource_state(name, 'present')
    if delete and result.returncode == 0:
        # Delete can be asynchronous; release only on authoritative absence.
        observed = invoke(['sandbox', 'list', '--output', 'json'], capture_output=True, text=True, timeout=30)
        if observed.returncode == 0:
            start = re.search(r'(?m)^\s*(?=[\[{])', observed.stdout)
            if start:
                items, following = gateway.decode_page(json.loads(observed.stdout[start.start():]))
                if not following and not any(item['name'] == name for item in items):
                    store.resource_state(name, 'absent')
    store.close()
    return result.returncode


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except Exception as exc:
        print('gyre fullsend adapter:', type(exc).__name__, str(exc), file=sys.stderr)
        raise SystemExit(75)
