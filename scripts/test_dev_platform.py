"""dev-platform bootstrap provisions over real REST and is idempotent.

The provisioning logic runs against a local HTTP server that implements the
platform's provisioning surface (workspaces, repos, users with API keys,
workspace membership, token introspection), verifying that bootstrap
registers pipeline settings once and reuses existing records instead of
duplicating them.
"""
import importlib.util
import json
import re
import tempfile
import threading
import unittest
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

SPEC = importlib.util.spec_from_file_location("dev_platform", Path(__file__).with_name("dev-platform.py"))
bootstrap = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bootstrap)

from pipeline.store import Store


class StubServer:
    """In-process stand-in for the platform's provisioning REST surface."""

    def __init__(self):
        self.admin_token = 'admin-token'
        self.workspaces = {}
        self.repos = {}
        self.users = {}       # username -> {'id', 'key'}
        self.keys = {}        # raw key -> username
        self.memberships = []
        self.posts = []
        outer = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def _send(self, status, body):
                payload = json.dumps(body).encode() if body is not None else b''
                self.send_response(status)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)

            def _body(self):
                length = int(self.headers.get('Content-Length') or 0)
                return json.loads(self.rfile.read(length)) if length else None

            def _authorized(self):
                token = (self.headers.get('Authorization') or '').removeprefix('Bearer ')
                return token == outer.admin_token or token in outer.keys

            def do_GET(self):
                if self.path == '/api/v1/version':
                    return self._send(200, {'name': 'stub'})
                if not self._authorized():
                    return self._send(401, {'error': 'unauthorized'})
                if self.path == '/api/v1/auth/token-info':
                    token = (self.headers.get('Authorization') or '').removeprefix('Bearer ')
                    username = outer.keys.get(token)
                    if not username:
                        return self._send(401, {'error': 'unknown token'})
                    return self._send(200, {'subject': username, 'roles': ['Developer'],
                                            'token_kind': 'api_key'})
                match = re.fullmatch(r'/api/v1/workspaces/([^/]+)', self.path)
                if match:
                    record = outer.workspaces.get(match.group(1))
                    return self._send(200, record) if record else self._send(404, {'error': 'not found'})
                if self.path == '/api/v1/workspaces':
                    return self._send(200, list(outer.workspaces.values()))
                match = re.fullmatch(r'/api/v1/repos/([^/]+)', self.path)
                if match:
                    record = outer.repos.get(match.group(1))
                    return self._send(200, record) if record else self._send(404, {'error': 'not found'})
                return self._send(404, {'error': 'no route'})

            def do_POST(self):
                if not self._authorized():
                    return self._send(401, {'error': 'unauthorized'})
                body = self._body()
                outer.posts.append(self.path)
                if self.path == '/api/v1/workspaces':
                    record = dict(body) | {'id': uuid.uuid4().hex}
                    outer.workspaces[record['id']] = record
                    return self._send(201, record)
                if self.path == '/api/v1/repos':
                    record = dict(body) | {'id': uuid.uuid4().hex}
                    outer.repos[record['id']] = record
                    return self._send(201, record)
                if self.path == '/api/v1/users':
                    if body['username'] in outer.users:
                        return self._send(409, {'error': 'user exists'})
                    key = 'gyre_' + uuid.uuid4().hex
                    record = {'user': {'id': uuid.uuid4().hex, 'username': body['username']},
                              'api_key': {'key': key, 'name': 'bootstrap'}}
                    outer.users[body['username']] = record['user']
                    outer.keys[key] = body['username']
                    return self._send(201, record)
                match = re.fullmatch(r'/api/v1/workspaces/([^/]+)/members', self.path)
                if match:
                    if any(item['workspace'] == match.group(1) and item['user'] == body['user_id']
                           for item in outer.memberships):
                        return self._send(409, {'error': 'already a member'})
                    outer.memberships.append({'workspace': match.group(1), 'user': body['user_id'],
                                              'role': body['role']})
                    return self._send(201, {})
                return self._send(404, {'error': 'no route'})

        self.server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.endpoint = f'http://127.0.0.1:{self.server.server_address[1]}'
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    def start(self):
        self.thread.start()
        return self

    def stop(self):
        self.server.shutdown()
        self.server.server_close()


class BootstrapTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.stub = StubServer().start()
        self.addCleanup(self.stub.stop)
        self.store = Store(Path(self.temp.name) / 'state')
        self.addCleanup(self.store.close)

    def test_provision_creates_context_and_registers_settings(self):
        provisioned = bootstrap.provision(self.stub.endpoint, self.stub.admin_token, self.store)
        bootstrap.register(self.store, self.stub.endpoint, provisioned)
        self.assertEqual(self.store.setting('platform_endpoint'), self.stub.endpoint)
        self.assertEqual(self.store.setting('platform_token'), provisioned['token'])
        self.assertEqual(self.store.setting('platform_workspace'), provisioned['workspace'])
        self.assertEqual(self.store.setting('platform_repo'), provisioned['repo'])
        self.assertEqual(len(self.stub.workspaces), 1)
        self.assertEqual(len(self.stub.repos), 1)
        self.assertEqual(list(self.stub.users), ['gyre-dev-bridge'])
        # The service principal holds the Developer role on the workspace.
        self.assertEqual(self.stub.memberships,
                         [{'workspace': provisioned['workspace'],
                           'user': provisioned['principal'], 'role': 'Developer'}])

    def test_repeated_provision_reuses_records_without_duplicates(self):
        first = bootstrap.provision(self.stub.endpoint, self.stub.admin_token, self.store)
        bootstrap.register(self.store, self.stub.endpoint, first)
        posts_after_first = list(self.stub.posts)
        second = bootstrap.provision(self.stub.endpoint, self.stub.admin_token, self.store)
        bootstrap.register(self.store, self.stub.endpoint, second)
        self.assertEqual(second['workspace'], first['workspace'])
        self.assertEqual(second['repo'], first['repo'])
        self.assertEqual(second['token'], first['token'])
        self.assertEqual(self.stub.posts, posts_after_first)
        self.assertEqual(len(self.stub.workspaces), 1)
        self.assertEqual(len(self.stub.users), 1)

    def test_lost_token_provisions_a_fresh_service_principal(self):
        first = bootstrap.provision(self.stub.endpoint, self.stub.admin_token, self.store)
        bootstrap.register(self.store, self.stub.endpoint, first)
        # The pipeline store loses the token (or it was revoked): the
        # existing username collides, so the next free identity is used.
        self.store.set_setting('platform_token', 'gyre_revoked')
        second = bootstrap.provision(self.stub.endpoint, self.stub.admin_token, self.store)
        self.assertNotEqual(second['token'], 'gyre_revoked')
        self.assertIn('gyre-dev-bridge-2', self.stub.users)
        self.assertEqual(second['principal'], self.stub.users['gyre-dev-bridge-2']['id'])

    def test_health_check_requires_a_serving_server(self):
        self.assertTrue(bootstrap.healthy(self.stub.endpoint, self.stub.admin_token))
        self.assertFalse(bootstrap.healthy('http://127.0.0.1:1', self.stub.admin_token))


if __name__ == '__main__':
    unittest.main()
