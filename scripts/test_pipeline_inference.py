import tempfile
import unittest
import urllib.error
from unittest.mock import patch

from pipeline.execution import Execution, Retry
from pipeline.inference import check
from pipeline.store import Store


class InferenceAdmissionTest(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.store = Store(directory.name)
        self.addCleanup(self.store.close)

    def test_rejected_credential_is_cached_without_gateway_updates(self):
        error = urllib.error.HTTPError('https://example.invalid', 401, 'unauthorized', None, None)
        with patch('pipeline.inference.credential', return_value='test-key'), patch('pipeline.inference.probe', side_effect=error) as probe, patch('pipeline.inference.configure') as configure:
            for _ in range(10):
                with self.assertRaisesRegex(Retry, 'HTTP 401'):
                    check(self.store)
        self.assertEqual(probe.call_count, 1)
        configure.assert_not_called()
        self.assertEqual(self.store.setting('inference_health')['failures'], 1)
        self.assertIn('HTTP 401', self.store.setting('admission_condition'))

    def test_rotated_credential_bypasses_failed_cache_and_syncs_gateway(self):
        error = urllib.error.HTTPError('https://example.invalid', 401, 'unauthorized', None, None)
        with patch('pipeline.inference.credential', side_effect=['expired-key', 'new-key', 'new-key']), patch('pipeline.inference.probe', side_effect=[error, None]) as probe, patch('pipeline.inference.configure') as configure:
            with self.assertRaises(Retry):
                check(self.store)
            check(self.store)
            check(self.store)
        self.assertEqual(probe.call_count, 2)
        configure.assert_called_once_with('new-key')
        self.assertEqual(self.store.setting('admission_condition'), 'Ready')

    def test_binary_credential_does_not_reach_http(self):
        with patch('pipeline.inference.credential', side_effect=UnicodeDecodeError('utf-8', b'\xff', 0, 1, 'bad credential')), patch('pipeline.inference.probe') as probe:
            with self.assertRaises(Retry):
                check(self.store)
        probe.assert_not_called()

    def test_unhealthy_inference_cannot_allocate_a_sandbox(self):
        import subprocess
        self.store.put_task('task-001', 'g', 'body', {})
        self.store.enqueue('implement', 'task-001', 'g', {})
        execution = Execution(self.store, self.store.claim('implement', 'worker'))
        providers = subprocess.CompletedProcess([], 0, 'gyre-enmaas\ngyre-github-rw\n', '')
        with patch.object(execution, 'login'), patch.object(execution, 'os', return_value=providers), patch('pipeline.inference.check', side_effect=Retry('InferenceUnavailable')), patch.object(self.store, 'reserve') as reserve:
            with self.assertRaisesRegex(Retry, 'InferenceUnavailable'):
                execution.cloud_step(self.store.task('task-001'), 'assignment')
        reserve.assert_not_called()


if __name__ == '__main__':
    unittest.main()
