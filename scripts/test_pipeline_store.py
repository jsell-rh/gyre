import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest

from pipeline.store import Store, StaleClaim


class StoreTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.store = Store(self.temp.name)
        self.addCleanup(self.store.close)
        self.store.put_task('task-001', 'g1', 'requirements', {'state': 'implement'})

    def work(self, stage='implement', inputs=None):
        return self.store.enqueue(stage, 'task-001', 'g1', inputs or {'seed': 'abc'})

    def test_independent_processes_claim_work_once(self):
        ident = self.work()
        command = [sys.executable, '-c', '''
import json,sys
from pipeline.store import Store
s=Store(sys.argv[1]); c=s.claim('implement',sys.argv[2],concurrency=8)
print(json.dumps(c['id'] if c else None))
''', self.temp.name]
        processes = [subprocess.Popen(command + [str(i)], cwd=Path(__file__).parent,
                                      stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
                     for i in range(8)]
        results = []
        for process in processes:
            stdout, stderr = process.communicate(timeout=15)
            self.assertEqual(process.returncode, 0, stderr)
            results.append(json.loads(stdout))
        self.assertEqual([value for value in results if value], [ident])

    def test_expired_worker_cannot_finish_after_reclaim(self):
        ident = self.work()
        first = self.store.claim('implement', 'first')
        self.store.db.execute('UPDATE work SET expires=? WHERE id=?', (time.time() - 1, ident))
        second = self.store.claim('implement', 'second')
        self.assertGreater(second['token'], first['token'])
        with self.assertRaises(StaleClaim):
            self.store.finish(ident, first['token'], {'candidate': 'wrong'}, {'candidate': 'wrong'})
        self.store.finish(ident, second['token'], {'candidate': 'right'}, {'candidate': 'right'})
        self.assertEqual(self.store.tasks()[0]['data']['candidate'], 'right')

    def test_requirement_change_fences_inflight_result(self):
        ident = self.work()
        claim = self.store.claim('implement', 'worker')
        self.store.put_task('task-001', 'g2', 'new requirements', {'state': 'triage'})
        with self.assertRaises(StaleClaim):
            self.store.finish(ident, claim['token'], {}, {'state': 'merged'})
        self.assertEqual(self.store.tasks()[0]['data']['state'], 'triage')

    def test_replay_and_durable_findings(self):
        ident = self.work('review')
        claim = self.store.claim('review', 'reviewer')
        self.store.finish(ident, claim['token'], {'verdict': 'repair'}, {'state': 'implement'},
                          [{'category': 'code', 'source': 'abc', 'command': ['test'],
                            'exit_code': 1, 'artifact': 'failure.log'}])
        self.assertEqual(self.work('review'), ident)
        self.assertIsNone(self.store.claim('review', 'another'))
        reopened = Store(self.temp.name)
        try:
            row = reopened.db.execute('SELECT * FROM findings').fetchone()
            self.assertEqual(json.loads(row['detail'])['exit_code'], 1)
            self.assertEqual(row['source'], 'abc')
        finally:
            reopened.close()

    def test_resource_budget_counts_expired_claim_resources_until_deleted(self):
        ident = self.work()
        claim = self.store.claim('implement', 'one')
        self.assertTrue(self.store.reserve('sandbox-one', ident, claim['token'], 'sandbox', 1))
        self.store.db.execute('UPDATE work SET expires=? WHERE id=?', (time.time() - 1, ident))
        other = self.store.claim('implement', 'two')
        self.assertFalse(self.store.reserve('sandbox-two', ident, other['token'], 'sandbox', 1))
        self.store.resource_state('sandbox-one', 'deleting')
        self.assertFalse(self.store.reserve('sandbox-two', ident, other['token'], 'sandbox', 1))
        self.store.resource_state('sandbox-one', 'absent')
        self.assertTrue(self.store.reserve('sandbox-two', ident, other['token'], 'sandbox', 1))

    def test_retry_retains_same_work_and_respects_backoff(self):
        ident = self.work()
        claim = self.store.claim('implement', 'one')
        self.store.retry(ident, claim['token'], {'category': 'infrastructure'})
        self.assertEqual(self.work(), ident)
        self.assertIsNone(self.store.claim('implement', 'two'))
        row = self.store.db.execute('SELECT retry_at,failures FROM work WHERE id=?', (ident,)).fetchone()
        self.assertEqual(row['failures'], 1)
        self.assertGreater(row['retry_at'], time.time())

    def test_new_claim_adopts_existing_compute_without_another_reservation(self):
        ident = self.work()
        old = self.store.claim('implement', 'one')
        self.assertTrue(self.store.reserve('same-sandbox', ident, old['token'], 'sandbox', 1,
                                          {'logical_token': old['token']}))
        self.store.db.execute('UPDATE work SET expires=? WHERE id=?', (time.time() - 1, ident))
        new = self.store.claim('implement', 'two')
        self.assertTrue(self.store.reserve('same-sandbox', ident, new['token'], 'sandbox', 1))
        resource = self.store.db.execute('SELECT * FROM resources').fetchone()
        self.assertEqual(resource['token'], new['token'])
        self.assertEqual(json.loads(resource['data'])['logical_token'], old['token'])
        with self.assertRaises(StaleClaim):
            self.store.reserve('same-sandbox', ident, old['token'], 'sandbox', 1)

    def test_main_movement_does_not_reset_implementation_backoff(self):
        first = self.work(inputs={'base': 'old', 'candidate': None, 'repair': None})
        claim = self.store.claim('implement', 'one')
        self.store.retry(first, claim['token'], {'message': 'capacity unavailable'})
        before = self.store.db.execute('SELECT retry_at FROM work WHERE id=?', (first,)).fetchone()[0]
        second = self.work(inputs={'base': 'new', 'candidate': None, 'repair': None})
        self.assertEqual(first, second)
        row = self.store.db.execute('SELECT input,retry_at FROM work WHERE id=?', (first,)).fetchone()
        self.assertEqual(row['retry_at'], before)
        self.assertEqual(json.loads(row['input'])['base'], 'new')
        self.assertIsNone(self.store.claim('implement', 'two'))

    def test_deleted_reservation_can_be_recreated_but_still_obeys_capacity(self):
        ident = self.work()
        claim = self.store.claim('implement', 'one')
        self.assertTrue(self.store.reserve('same', ident, claim['token'], 'sandbox', 1))
        self.store.resource_state('same', 'absent')
        self.assertTrue(self.store.reserve('other', ident, claim['token'], 'sandbox', 1))
        self.assertFalse(self.store.reserve('same', ident, claim['token'], 'sandbox', 1))
        self.store.resource_state('other', 'absent')
        self.assertTrue(self.store.reserve('same', ident, claim['token'], 'sandbox', 1))
        self.assertEqual(self.store.db.execute("SELECT state FROM resources WHERE name='same'").fetchone()[0], 'intent')


if __name__ == '__main__':
    unittest.main()
