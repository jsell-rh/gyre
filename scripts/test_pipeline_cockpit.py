import tempfile
import unittest
from pipeline.store import Store
from pipeline.cockpit import snapshot


class ReportingTest(unittest.TestCase):
    def test_pending_github_checks_are_not_an_infrastructure_failure_queue(self):
        with tempfile.TemporaryDirectory() as directory:
            store = Store(directory)
            self.addCleanup(store.close)
            store.put_task('task-001', 'g', 'body', {'dependencies': [], 'triaged': 'g',
                'candidate': 'candidate', 'pr': 'https://github.com/jsell-rh/gyre/pull/1'})
            work = store.enqueue('publish', 'task-001', 'g', {'candidate': 'candidate'})
            claim = store.claim('publish', 'publisher')
            store.retry(work, claim['token'], {'message': 'GitHub merge gate: pending'})
            view = snapshot(store)
            self.assertEqual(view['tasks'][0]['state'], 'published')
            self.assertEqual(view['counts'].get('deferred', 0), 0)
            self.assertEqual(view['tasks'][0]['condition'], 'GitHub merge gate: pending')
            self.assertFalse(view['resources']['inventory_ready'])
            store.event('gateway_inventory', {'sandboxes': 0})
            self.assertTrue(snapshot(store)['resources']['inventory_ready'])


if __name__ == '__main__':
    unittest.main()
