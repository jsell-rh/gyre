#!/usr/bin/env python3
"""Bootstrap the Gyre platform as the development task ledger (dogfooding stage 1).

Builds (SKIP_WEB_BUILD=1) and starts a gyre-server instance on a dedicated
port with its own SQLite data directory, provisions the development context
(workspace and repo records) plus a Developer-role service principal with an
API token, and registers the endpoint and token as pipeline settings so
stage processes read them (the same store.setting mechanism the gateway
setting uses). The bridge then upserts every specs/tasks/*.md task into the
platform through the authenticated REST surface and consults it as the
authoritative dependency-sequencing gate for implement admission.

Idempotent: a healthy running instance with valid settings is left
untouched, and existing workspace/repo/user records are reused.

Usage:
  python3 scripts/dev-platform.py [--state DIR] [--port N] start
  python3 scripts/dev-platform.py [--state DIR] stop
  python3 scripts/dev-platform.py [--state DIR] status
"""
import argparse
import json
import os
import secrets
import signal
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_PORT = 3210
TENANT = 'default'  # the global bootstrap token's tenant (system_principal_tenant)
WORKSPACE_NAME = 'Gyre Development'
WORKSPACE_SLUG = 'gyre-dev'
REPO_NAME = 'gyre'
PRINCIPAL = 'gyre-dev-bridge'
HEALTH_TIMEOUT = 180
HTTP_TIMEOUT = 15


def request(method, url, token, body=None, timeout=HTTP_TIMEOUT):
    request = urllib.request.Request(url, method=method)
    if token:
        request.add_header('Authorization', 'Bearer ' + token)
    payload = None
    if body is not None:
        payload = json.dumps(body).encode()
        request.add_header('Content-Type', 'application/json')
    try:
        with urllib.request.urlopen(request, payload, timeout=timeout) as response:
            raw = response.read()
            return response.status, (json.loads(raw) if raw else None)
    except urllib.error.HTTPError as exc:
        raw = exc.read()
        try:
            return exc.code, (json.loads(raw) if raw else None)
        except ValueError:
            return exc.code, None
    except (urllib.error.URLError, TimeoutError, OSError) as exc:
        return None, str(exc)


def paths(state):
    platform = state / 'platform'
    return {'root': platform, 'db': platform / 'gyre.db', 'repos': platform / 'repos',
            'log': platform / 'server.log', 'pid': platform / 'server.pid',
            'admin': platform / 'admin-token'}


def read_admin_token(path):
    if path.exists():
        value = path.read_text().strip()
        if value:
            return value
    value = secrets.token_hex(32)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(value + '\n')
    path.chmod(0o600)
    return value


def healthy(endpoint, token):
    status, _ = request('GET', endpoint + '/api/v1/version', token)
    return status == 200


def server_pid(pid_file):
    if not pid_file.exists():
        return None
    try:
        pid = int(pid_file.read_text().strip())
        os.kill(pid, 0)
        return pid
    except (ValueError, ProcessLookupError, PermissionError):
        pid_file.unlink(missing_ok=True)
        return None


def build():
    """Idempotently build the server binary (the committed web/dist is reused)."""
    cargo = os.environ.get('GYRE_CARGO', 'cargo')
    environment = dict(os.environ, SKIP_WEB_BUILD='1')
    subprocess.run([cargo, 'build', '-p', 'gyre-server'], cwd=ROOT, env=environment, check=True)
    binary = Path(os.environ.get('GYRE_TARGET_DIR', ROOT / 'target')) / 'debug' / 'gyre-server'
    if not binary.exists():
        raise SystemExit(f'build produced no server binary at {binary}')
    return binary


def start_server(args, path, admin_token):
    path['root'].mkdir(parents=True, exist_ok=True)
    path['repos'].mkdir(parents=True, exist_ok=True)
    binary = build()
    environment = dict(os.environ,
                       GYRE_PORT=str(args.port),
                       GYRE_AUTH_TOKEN=admin_token,
                       GYRE_DATABASE_URL='sqlite://' + str(path['db']),
                       GYRE_REPOS_PATH=str(path['repos']),
                       GYRE_BASE_URL=f'http://127.0.0.1:{args.port}',
                       GYRE_SNAPSHOT_PATH=str(path['root'] / 'snapshots'))
    log = path['log'].open('ab')
    process = subprocess.Popen([str(binary)], cwd=ROOT, env=environment,
                               stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    path['pid'].write_text(str(process.pid) + '\n')
    deadline = time.time() + HEALTH_TIMEOUT
    while time.time() < deadline:
        if healthy(f'http://127.0.0.1:{args.port}', admin_token):
            return process.pid
        if process.poll() is not None:
            raise SystemExit(f'server exited early; see {path["log"]}')
        time.sleep(0.5)
    process.terminate()
    raise SystemExit(f'server did not become healthy within {HEALTH_TIMEOUT}s; see {path["log"]}')


def provision(endpoint, admin_token, store):
    """Provision the development context and service principal over real REST."""
    def api(method, path_url, body=None):
        return request(method, endpoint + path_url, admin_token, body)

    # Workspace: reuse the registered one, or an existing slug match, or create.
    workspace = store.setting('platform_workspace')
    if workspace:
        status, record = request('GET', f'{endpoint}/api/v1/workspaces/{workspace}', admin_token)
        if status != 200:
            workspace = None
    if not workspace:
        status, record = api('GET', '/api/v1/workspaces')
        record = next((item for item in (record or []) if item.get('slug') == WORKSPACE_SLUG), None) \
            if status == 200 else None
        if record is None:
            status, record = api('POST', '/api/v1/workspaces',
                                 {'name': WORKSPACE_NAME, 'slug': WORKSPACE_SLUG,
                                  'description': "Development task ledger and sequencing authority "
                                                 "for Gyre's own development (dogfooding progression stage 1)."})
            if status != 201:
                raise SystemExit(f'workspace provisioning failed: {status} {record}')
        workspace = record['id']

    # Repo record representing the Gyre development repository.
    repo = store.setting('platform_repo')
    if repo:
        status, _ = request('GET', f'{endpoint}/api/v1/repos/{repo}', admin_token)
        if status != 200:
            repo = None
    if not repo:
        status, record = api('POST', '/api/v1/repos',
                             {'workspace_id': workspace, 'name': REPO_NAME,
                              'description': 'The Gyre repository developed through the pipeline bridge.'})
        if status != 201:
            raise SystemExit(f'repo provisioning failed: {status} {record}')
        repo = record['id']

    # Service principal with the Developer role and an API token. The raw
    # key is returned exactly once at creation; it is persisted as a
    # pipeline setting (the store directory is 0700).
    token = store.setting('platform_token')
    if token:
        status, record = request('GET', endpoint + '/api/v1/auth/token-info', token)
        if status == 200 and 'Developer' in (record or {}).get('roles', []):
            return {'workspace': workspace, 'repo': repo, 'token': token, 'principal': 'existing'}
    user = None
    for suffix in ['', '-2', '-3', '-4', '-5']:
        username = PRINCIPAL + suffix
        status, record = api('POST', '/api/v1/users',
                             {'username': username, 'tenant_id': TENANT, 'roles': ['Developer']})
        if status == 201:
            user, token = record['user'], record['api_key']['key']
            break
        if status != 409:
            raise SystemExit(f'service principal provisioning failed: {status} {record}')
    if user is None:
        raise SystemExit(f'cannot provision a service principal: {PRINCIPAL} exists without a valid token')
    # Workspace membership (Developer role) so workspace-scoped reads pass
    # the real membership ABAC check. 409 means a previous run invited them.
    status, _ = api('POST', f'/api/v1/workspaces/{workspace}/members',
                    {'user_id': user['id'], 'role': 'Developer'})
    if status not in (201, 409):
        raise SystemExit(f'membership provisioning failed: {status}')
    return {'workspace': workspace, 'repo': repo, 'token': token, 'principal': user['id']}


def register(store, endpoint, provisioned):
    store.set_setting('platform_endpoint', endpoint)
    store.set_setting('platform_token', provisioned['token'])
    store.set_setting('platform_workspace', provisioned['workspace'])
    store.set_setting('platform_repo', provisioned['repo'])
    store.event('platform_bootstrap', {'endpoint': endpoint, 'workspace': provisioned['workspace'],
                                       'repo': provisioned['repo'], 'principal': provisioned['principal']})


def start(args):
    from pipeline.store import Store
    state = args.state
    state.mkdir(parents=True, exist_ok=True)
    path = paths(state)
    endpoint = f'http://127.0.0.1:{args.port}'
    admin_token = read_admin_token(path['admin'])
    store = Store(state)
    try:
        if healthy(endpoint, admin_token):
            # A server (from this or a previous invocation) is already serving.
            provisioned = provision(endpoint, admin_token, store)
            register(store, endpoint, provisioned)
            print(f'platform already running at {endpoint}; settings refreshed')
            return
        pid = start_server(args, path, admin_token)
        provisioned = provision(endpoint, admin_token, store)
        register(store, endpoint, provisioned)
        print(f'platform started (pid {pid}) at {endpoint}')
        print(f'data directory: {path["root"]}')
        print('pipeline settings registered: platform_endpoint, platform_token, '
              'platform_workspace, platform_repo')
        print('next: run discovery (dev-pipeline.py discover implement) to sync the task ledger')
    finally:
        store.close()


def stop(args):
    path = paths(args.state)
    pid = server_pid(path['pid'])
    if pid is None:
        print('no running platform server recorded')
        return
    os.kill(pid, signal.SIGTERM)
    deadline = time.time() + 30
    while time.time() < deadline:
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            break
        time.sleep(0.5)
    else:
        os.kill(pid, signal.SIGKILL)
    path['pid'].unlink(missing_ok=True)
    print(f'platform server (pid {pid}) stopped; implement admission now fails closed')


def status(args):
    from pipeline.store import Store
    store = Store(args.state)
    try:
        path = paths(args.state)
        endpoint = store.setting('platform_endpoint')
        print(f'settings endpoint: {endpoint or "not registered"}')
        pid = server_pid(path['pid'])
        print(f'recorded server pid: {pid or "none"}')
        if endpoint:
            admin_token = read_admin_token(path['admin'])
            print(f'server healthy: {healthy(endpoint, admin_token)}')
            token = store.setting('platform_token')
            if token:
                code, record = request('GET', endpoint + '/api/v1/auth/token-info', token)
                print(f'service token valid: {code == 200 and "Developer" in (record or {}).get("roles", [])}')
            workspace = store.setting('platform_workspace')
            if workspace:
                code, record = request('GET', f'{endpoint}/api/v1/workspaces/{workspace}/tasks', token)
                print(f'ledger records: {len(record) if code == 200 else f"listing failed: {code}"}')
    finally:
        store.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--state', type=Path,
                        default=Path(os.environ.get('GYRE_PIPELINE_STATE', ROOT / '.gyre-pipeline')))
    parser.add_argument('--port', type=int, default=int(os.environ.get('GYRE_DEV_PLATFORM_PORT', DEFAULT_PORT)))
    commands = parser.add_subparsers(dest='command', required=True)
    for name in ('start', 'stop', 'status'):
        commands.add_parser(name)
    args = parser.parse_args()
    args.state = args.state.resolve()
    {'start': start, 'stop': stop, 'status': status}[args.command](args)


if __name__ == '__main__':
    main()
