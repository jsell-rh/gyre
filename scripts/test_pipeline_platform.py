"""The platform bridge must consult the platform, not just mirror to it.

These tests run the bridge against a real local HTTP server that implements
the platform's task REST surface (authenticated reads, Developer-role writes,
and the platform's legal task-status machine). The mirror probe is the load
bearing case: with the local pipeline store marking a dependency satisfied
while the platform reports it not done, implement admission must be refused
- the base pipeline, which never consults the platform, admits it.
"""
import json
import re
import tempfile
import threading
import unittest
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from pipeline import platform
from pipeline.catalog import sync_checkout
from pipeline.eligibility import eligible
from pipeline.store import Store


class StubPlatform:
    """In-process HTTP stand-in for the platform's task coordination surface.

    Models the server-side contract: bearer-token authentication, the
    Developer role required for task writes, and the task status machine
    (gyre-domain Task::transition_status - done and cancelled are terminal).
    """

    # The platform's legal task status transitions.
    TRANSITIONS = {
        'backlog': ('in_progress', 'cancelled'),
        'in_progress': ('review', 'blocked', 'cancelled'),
        'review': ('done', 'in_progress', 'cancelled'),
        'blocked': ('in_progress', 'cancelled'),
        'done': (),
        'cancelled': (),
    }

    def __init__(self):
        self.records = {}
        self.created = []
        self.transitions = []
        self.tokens = {'developer-token': 'Developer', 'viewer-token': 'ReadOnly'}
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

            def _role(self):
                token = (self.headers.get('Authorization') or '').removeprefix('Bearer ')
                return outer.tokens.get(token)

            def do_GET(self):
                if self.path == '/api/v1/version':
                    return self._send(200, {'name': 'stub'})
                role = self._role()
                if role is None:
                    return self._send(401, {'error': 'missing or unknown token'})
                if self.path == '/api/v1/workspaces/ws-1/tasks':
                    return self._send(200, list(outer.records.values()))
                match = re.fullmatch(r'/api/v1/tasks/([^/]+)', self.path)
                if match:
                    record = outer.records.get(match.group(1))
                    return self._send(200, record) if record else self._send(404, {'error': 'not found'})
                return self._send(404, {'error': 'no route'})

            def do_POST(self):
                if self._role() != 'Developer':
                    return self._send(403, {'error': 'role may not write tasks'})
                if self.path == '/api/v1/tasks':
                    body = self._body()
                    if not body.get('title'):
                        return self._send(400, {'error': 'title required'})
                    record = dict(body) | {'id': uuid.uuid4().hex, 'status': 'backlog'}
                    outer.records[record['id']] = record
                    outer.created.append(body)
                    return self._send(201, record)
                return self._send(404, {'error': 'no route'})

            def do_PUT(self):
                if self._role() != 'Developer':
                    return self._send(403, {'error': 'role may not write tasks'})
                body = self._body()
                status_match = re.fullmatch(r'/api/v1/tasks/([^/]+)/status', self.path)
                if status_match:
                    record = outer.records.get(status_match.group(1))
                    if not record:
                        return self._send(404, {'error': 'not found'})
                    target = (body or {}).get('status')
                    if target not in outer.TRANSITIONS.get(record['status'], ()):
                        return self._send(400, {'error': f'illegal transition {record["status"]} to {target}'})
                    outer.transitions.append((record['id'], record['status'], target))
                    record['status'] = target
                    return self._send(200, record)
                match = re.fullmatch(r'/api/v1/tasks/([^/]+)', self.path)
                if match:
                    record = outer.records.get(match.group(1))
                    if not record:
                        return self._send(404, {'error': 'not found'})
                    for key in ('title', 'description', 'priority', 'labels'):
                        if key in (body or {}):
                            record[key] = body[key]
                    return self._send(200, record)
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

    def status_of(self, platform_id):
        return self.records[platform_id]['status']

    def by_title(self, title):
        return next(record for record in self.records.values() if record['title'] == title)


class PlatformBridgeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.store = Store(Path(self.temp.name) / 'state')
        self.addCleanup(self.store.close)
        self.stub = StubPlatform().start()
        self.addCleanup(self.stub.stop)
        self.configure(token='developer-token')

    def configure(self, token):
        platform.invalidate()
        self.store.set_setting('platform_endpoint', self.stub.endpoint)
        self.store.set_setting('platform_token', token)
        self.store.set_setting('platform_workspace', 'ws-1')
        self.store.set_setting('platform_repo', 'repo-1')

    def task(self, name, **data):
        self.store.put_task(name, 'g', 'body', {'dependencies': [], 'triaged': 'g', **data})

    def implement_work(self, name):
        """Enqueue implement work exactly as discovery would and return the row."""
        ident = self.store.enqueue('implement', name, 'g',
                                   {'base': 'main', 'candidate': None, 'repair': None})
        row = dict(self.store.db.execute('SELECT * FROM work WHERE id=?', (ident,)).fetchone())
        row['input'] = json.loads(row['input'])
        return row

    def mapping(self):
        return self.store.setting('platform_tasks', {})

    def events(self, kind):
        return [json.loads(row['detail']) for row in
                self.store.db.execute('SELECT detail FROM events WHERE kind=?', (kind,))]

    def sync(self):
        platform.invalidate()
        return platform.sync(self.store)

    def test_sync_registers_every_task_with_resolved_edges_and_is_idempotent(self):
        self.task('task-001', title='First', spec_ref='gap-1', delivered={'sha': 'a'})
        self.task('task-002', title='Second', spec_ref='gap-2', dependencies=['task-001'])
        summary = self.sync()
        self.assertEqual(summary['registered'], 2)
        self.assertEqual(len(self.stub.records), 2)
        first, second = self.stub.by_title('First'), self.stub.by_title('Second')
        self.assertEqual(first['spec_ref'], 'gap-1')
        self.assertEqual(first['order'], 1)
        self.assertEqual(second['depends_on'], [first['id']])
        # Tasks are registered untyped so the mechanical scheduler never
        # spawns agents for development ledger records.
        for body in self.stub.created:
            self.assertNotIn('task_type', body)
        self.assertEqual(first['labels'], ['dev-ledger'])
        # A delivered dependency is reconciled to done on the platform.
        self.assertEqual(self.stub.status_of(first['id']), 'done')
        # Re-running sync creates no duplicate records.
        again = self.sync()
        self.assertEqual(again['registered'], 0)
        self.assertEqual(len(self.stub.records), 2)

    def test_lifecycle_write_back_is_observable_on_the_platform(self):
        self.task('task-003', title='Third')
        self.sync()
        claim = {'stage': 'implement', 'task': 'task-003'}
        platform.admitted(self.store, claim)
        record = self.stub.by_title('Third')
        self.assertEqual(self.stub.status_of(record['id']), 'in_progress')
        platform.completed(self.store, claim)
        # done is reached through the platform's legal status machine.
        self.assertEqual(self.stub.status_of(record['id']), 'done')
        self.assertIn((record['id'], 'in_progress', 'review'), self.stub.transitions)
        self.assertIn((record['id'], 'review', 'done'), self.stub.transitions)

    def test_structural_block_marks_the_record_blocked(self):
        self.task('task-004', title='Fourth')
        self.sync()
        claim = {'stage': 'implement', 'task': 'task-004'}
        platform.admitted(self.store, claim)
        platform.blocked(self.store, claim)
        self.assertEqual(self.stub.status_of(self.stub.by_title('Fourth')['id']), 'blocked')

    def test_mirror_probe_platform_not_done_refuses_admission_local_state_alone_admits(self):
        # The local pipeline store marks the dependency satisfied, but its
        # platform record is not done: admission must be refused. This is
        # exactly the fabricated-completion defect the probe exists to catch -
        # a write-only mirror (or the base pipeline with no platform at all)
        # would admit the dependent task here.
        self.task('task-005', title='Fifth')
        self.task('task-006', title='Sixth', dependencies=['task-005'])
        self.sync()
        dependency = self.stub.by_title('Fifth')
        self.assertEqual(self.stub.status_of(dependency['id']), 'backlog')
        # The local store now marks the dependency satisfied (delivered)
        # without the platform record having transitioned.
        self.store.put_task('task-005', 'g', 'body',
                            {'dependencies': [], 'triaged': 'g', 'delivered': {'sha': 'b'}})
        work = self.implement_work('task-006')
        self.assertFalse(eligible(self.store, work))
        claim = self.store.claim('implement', 'worker',
                                 eligible=lambda item: eligible(self.store, item))
        self.assertIsNone(claim)
        # Once the platform reports the dependency done, admission proceeds.
        self.assertTrue(platform.completed(self.store, {'task': 'task-005'}))
        platform.invalidate()
        self.assertTrue(eligible(self.store, work))
        claim = self.store.claim('implement', 'worker',
                                 eligible=lambda item: eligible(self.store, item))
        self.assertEqual(claim['task'], 'task-006')

    def test_unmapped_dependency_fails_closed(self):
        # The dependency exists and is satisfied locally, but the platform
        # has no record for it (sync never registered it): the platform
        # cannot vouch for it, so admission is refused.
        self.task('task-007', title='Seventh', dependencies=['task-005'])
        self.task('task-005', title='Fifth', delivered={'sha': 'b'})
        platform.invalidate()
        work = self.implement_work('task-007')
        self.assertFalse(eligible(self.store, work))

    def test_fail_closed_records_durable_event_and_blocks_new_admission(self):
        self.store.set_setting('platform_endpoint', 'http://127.0.0.1:1')
        platform.invalidate()
        self.task('task-008', title='Eighth')
        work = self.implement_work('task-008')
        self.assertFalse(eligible(self.store, work))
        failures = self.events('platform_unavailable')
        self.assertTrue(failures)
        self.assertIn('fails closed', failures[0]['effect'])
        # A second evaluation in the same process stays failed without
        # spamming duplicate durable events.
        self.assertFalse(eligible(self.store, work))
        self.assertEqual(len(self.events('platform_unavailable')), 1)

    def test_fail_closed_leaves_claimed_in_flight_work_untouched(self):
        self.store.set_setting('platform_endpoint', 'http://127.0.0.1:1')
        platform.invalidate()
        self.task('task-009', title='Ninth')
        self.implement_work('task-009')
        claim = self.store.claim('implement', 'worker')
        self.assertEqual(claim['task'], 'task-009')
        # The post-execution eligibility recheck sees claimed work: the
        # platform being unreachable must not fence in-flight work.
        self.assertTrue(eligible(self.store, claim))

    def test_unconfigured_bridge_is_inactive(self):
        store = Store(Path(self.temp.name) / 'other')
        self.addCleanup(store.close)
        store.put_task('task-010', 'g', 'body', {'dependencies': [], 'triaged': 'g'})
        work = store.enqueue('implement', 'task-010', 'g',
                             {'base': 'main', 'candidate': None, 'repair': None})
        self.assertIsNone(platform.sync(store))
        self.assertTrue(platform.dependencies_satisfied(store, []))
        self.assertTrue(eligible(store, dict(store.db.execute(
            'SELECT * FROM work WHERE id=?', (work,)).fetchone()) | {'input': '{}'}))

    def test_write_without_developer_role_is_refused_and_recorded(self):
        # Registration with a token that lacks the Developer role is refused
        # by the platform's authorization and recorded as a sync failure.
        self.configure(token='viewer-token')
        self.task('task-011', title='Eleventh')
        summary = self.sync()
        self.assertEqual(summary['registered'], 0)
        self.assertEqual(len(self.stub.records), 0)
        self.assertTrue(self.events('platform_sync_failed'))
        # Task transitions through the bridge path are equally refused.
        self.configure(token='developer-token')
        self.task('task-012', title='Twelfth')
        self.sync()
        self.configure(token='viewer-token')
        platform.invalidate()
        self.assertFalse(platform.completed(self.store, {'task': 'task-012'}))
        self.assertTrue(self.events('platform_transition_failed'))

    def test_reopened_work_gets_a_fresh_record_and_the_terminal_one_is_retired(self):
        self.task('task-013', title='Thirteenth', delivered={'sha': 'c'})
        self.sync()
        old = self.stub.by_title('Thirteenth')
        self.assertEqual(self.stub.status_of(old['id']), 'done')
        # Requirements changed: the task reopens at a new generation with
        # the delivery cleared. A terminal record cannot represent that.
        self.store.put_task('task-013', 'g2', 'body', {'dependencies': [], 'triaged': 'g2'})
        self.sync()
        self.assertEqual(old['labels'], ['dev-ledger-superseded'])
        entry = self.mapping()['task-013']
        self.assertNotEqual(entry['id'], old['id'])
        self.assertEqual(self.stub.status_of(entry['id']), 'backlog')

    def test_operator_reset_of_a_non_terminal_record_is_respected(self):
        # A structurally held task is blocked on the platform. An operator
        # resetting the platform record through the platform's own surface
        # is authoritative: the bridge pushed that transition once and does
        # not silently push it back while the local state is unchanged.
        self.task('task-014', title='Fourteenth')
        self.sync()
        claim = {'stage': 'implement', 'task': 'task-014'}
        platform.admitted(self.store, claim)
        self.store.set_setting(self.store.runtime_hold_key('task-014', 'implement', 'g'),
                               {'category': 'repair_stalled', 'message': 'held'})
        self.sync()
        record = self.stub.by_title('Fourteenth')
        self.assertEqual(self.stub.status_of(record['id']), 'blocked')
        # Operator reset via PUT /api/v1/tasks/{id}/status (a legal
        # blocked -> in_progress transition on the platform).
        record['status'] = 'in_progress'
        self.sync()
        self.assertEqual(self.stub.status_of(record['id']), 'in_progress')

    def test_sync_checkout_runs_the_platform_sync(self):
        root = Path(self.temp.name) / 'checkout'
        (root / 'specs/system').mkdir(parents=True)
        (root / 'specs/tasks').mkdir(parents=True)
        (root / 'specs/GOAL.md').write_text('real code')
        body = ('---\ntitle: Synced\nspec_ref: behavior.md\ndepends_on: []\nprogress: ready\n'
                'commits: []\n---\n\n## Required behavior\nImplement the spec.\n')
        (root / 'specs/tasks/task-015.md').write_text(body)
        sync_checkout(self.store, root, 'main')
        self.assertEqual(len(self.stub.records), 1)
        self.assertEqual(self.stub.by_title('Synced')['spec_ref'], 'behavior.md')


if __name__ == '__main__':
    unittest.main()
