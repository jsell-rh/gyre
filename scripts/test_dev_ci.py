#!/usr/bin/env python3
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('ci', Path(__file__).with_name('dev-ci.py'))
ci = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ci)


class GitHubChecksTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.head, self.base = 'a' * 40, 'b' * 40
        self.pr = {'state': 'OPEN', 'headRefOid': self.head, 'mergeStateStatus': 'CLEAN', 'statusCheckRollup': []}
        self.calls = []
        self.baseline = []
        self.logs = {}

    def run_cli(self, *args, **kwargs):
        self.calls.append(args)
        if args[:3] == ('gh', 'pr', 'view'):
            value = json.dumps(self.pr)
        elif args[:2] == ('gh', 'api'):
            value = json.dumps({'workflow_runs': self.baseline})
        elif '--log-failed' in args:
            value = self.logs[args[3]]
        else:
            value = json.dumps({'workflowDatabaseId': 5, 'name': 'E2E Tests'})
        return subprocess.CompletedProcess(args, 0, value, '')

    def observe(self):
        return ci.observe(self.run_cli, 'https://github.com/example/repo/pull/1', self.head, self.base, self.temp.name)

    def check(self, conclusion='SUCCESS', status='COMPLETED'):
        return {'status': status, 'conclusion': conclusion, 'name': 'e2e', 'workflowName': 'E2E Tests',
                'detailsUrl': 'https://github.com/example/repo/actions/runs/10/job/100'}

    def test_no_checks_yet_or_pending_or_policy_blocks_publication(self):
        self.assertEqual(self.observe()['status'], 'pending')
        self.pr['statusCheckRollup'] = [self.check(None, 'IN_PROGRESS')]
        self.assertEqual(self.observe()['status'], 'pending')
        self.pr['statusCheckRollup'] = [self.check()]
        self.pr['mergeStateStatus'] = 'BLOCKED'
        self.assertEqual(self.observe()['status'], 'policy_pending')
        self.pr['mergeStateStatus'] = 'CLEAN'
        self.assertEqual(self.observe()['status'], 'passed')
        self.pr['headRefOid'] = self.base
        with self.assertRaisesRegex(RuntimeError, 'head changed'):
            self.observe()

    def test_exact_base_failure_requires_matching_test_defects(self):
        self.pr['statusCheckRollup'] = [self.check('FAILURE')]
        self.baseline = [{'head_sha': self.base, 'workflow_id': 5, 'status': 'completed', 'conclusion': 'failure', 'id': 9, 'run_number': 1}]
        self.logs = {'10': '2026-10-08 tests/e2e/app.spec.js:55:3 › expected real navigation ───\n',
                     '9': '2026-10-07 tests/e2e/app.spec.js:55:3 › expected real navigation ─────\n'}
        self.assertEqual(self.observe()['status'], 'baseline_failed')
        self.logs['10'] += 'tests/e2e/app.spec.js:90:3 › feature regression ───\n'
        self.assertEqual(self.observe()['status'], 'candidate_failed')
        self.assertIn('feature regression', (Path(self.temp.name) / 'github-checks.log').read_text())
        self.baseline[0]['head_sha'] = 'c' * 40
        self.assertEqual(self.observe()['status'], 'candidate_failed')

    def test_cancelled_runner_retries_without_implementation(self):
        self.pr['statusCheckRollup'] = [self.check('CANCELLED')]
        self.assertEqual(self.observe(), {'status': 'infrastructure', 'runs': [10]})
        self.assertEqual(len(self.calls), 1)


if __name__ == '__main__':
    unittest.main()
