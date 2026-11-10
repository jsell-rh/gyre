import tempfile
import unittest
from pipeline.store import Store
from pipeline.cockpit import snapshot


class ReportingTest(unittest.TestCase):
    def test_shared_environment_wait_is_blocked_without_becoming_a_product_dependency(self):
        with tempfile.TemporaryDirectory() as directory:
            store=Store(directory);self.addCleanup(store.close)
            store.put_task('task-001','g','body',{'dependencies':[], 'candidate':'code',
                'repair':{'shared_environment':True,'prerequisite':'ci-repair'}})
            view=snapshot(store)
            self.assertEqual(view['tasks'][0]['state'],'blocked')
            self.assertEqual(view['tasks'][0]['deps'],[])
            self.assertIn('ci-repair',view['tasks'][0]['condition'])
            self.assertEqual(view['eligible'],0)

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

    def test_external_wait_and_inference_failure_are_visible_without_credential_fingerprint(self):
        import time
        with tempfile.TemporaryDirectory() as directory:
            store=Store(directory);self.addCleanup(store.close)
            store.put_task('task-001','g','body',{'candidate':'candidate','published_head':'head',
                'ci':{'head':'head','status':'pending','waiting_since':time.time()-901}})
            store.set_setting('inference_health',{'condition':'InferenceUnavailable: timeout',
                'expires':time.time()+60,'failures':2,'fingerprint':'credential-fingerprint'})
            view=snapshot(store)
            self.assertEqual(view['dispatch']['integration_waiting'],['task-001'])
            self.assertEqual(view['health']['inference']['failures'],2)
            self.assertNotIn('fingerprint',view['health']['inference'])


if __name__ == '__main__':
    unittest.main()
