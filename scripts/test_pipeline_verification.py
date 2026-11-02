"""Fail-closed receipts, real process failure/timeout, and shared gateway capacity."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).parent))
from pipeline.execution import Execution, Retry, Wait
from pipeline.store import Store
from pipeline.verification import COMMANDS, run, retire, validate_receipt

spec = importlib.util.spec_from_file_location('remote_verify', Path(__file__).with_name('pipeline-verify.py'))
remote = importlib.util.module_from_spec(spec)
spec.loader.exec_module(remote)


class VerificationTests(unittest.TestCase):
    def test_receipt_cannot_skip_checks_or_change_source(self):
        identity = dict(candidate='a', base='b', head='c', tree='d')
        valid = identity | {'passed': True, 'checks': [{'command': c, 'exit_code': 0} for c in COMMANDS]}
        validate_receipt(valid, identity)
        for invalid in (valid | {'head': 'wrong'}, valid | {'checks': valid['checks'][:1]},
                        valid | {'checks': list(reversed(valid['checks']))},
                        valid | {'passed': False}, valid | {'infrastructure': True}):
            with self.subTest(receipt=invalid), self.assertRaises(Retry):
                validate_receipt(invalid, identity)
        failure = identity | {'passed': False, 'checks': [{'command': COMMANDS[0], 'exit_code': 1}]}
        validate_receipt(failure, identity)

    def test_real_failure_stops_later_steps_and_keeps_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            receipt = root / 'result.json'
            sentinel = root / 'must-not-exist'
            commands = [([sys.executable, '-c', 'raise SystemExit(17)'], directory, 10),
                        ([sys.executable, '-c', f'open({str(sentinel)!r}, "w").close()'], directory, 10)]
            remote.run_checks(commands, receipt, {'head': 'exact'}, os.environ.copy())
            value = json.loads(receipt.read_text())
            self.assertFalse(value['passed'])
            self.assertEqual(value['head'], 'exact')
            self.assertEqual(value['checks'][0]['exit_code'], 17)
            self.assertFalse(sentinel.exists())

    def test_real_timeout_kills_check_and_records_infrastructure_code(self):
        with tempfile.TemporaryDirectory() as directory:
            receipt = Path(directory) / 'result.json'
            remote.run_checks([([sys.executable, '-c', 'import time; time.sleep(60)'], directory, .1)],
                              receipt, {}, os.environ.copy())
            self.assertEqual(json.loads(receipt.read_text())['checks'][0]['exit_code'], 124)

    def test_verification_competes_for_same_slots_as_agents(self):
        with tempfile.TemporaryDirectory() as directory:
            s = Store(directory)
            self.addCleanup(s.close)
            s.set_setting('slots', 1)
            body = '---\ntitle: fixture\nprogress: not-started\nspec_ref: fixture\ndepends_on: []\n---\n'
            s.put_task('task-001', 'g', body, {})
            work = s.enqueue('verify', 'task-001', 'g', {'base': 'a'})
            claim = s.claim('verify', 'fixture')
            execution = Execution(s, claim)
            s.reserve('occupied-agent', work, claim['token'], 'sandbox', 1)
            with patch.object(execution, 'login'), patch.object(execution, 'os', side_effect=AssertionError('must not allocate')):
                with self.assertRaises(Wait):
                    run(execution, Path(directory), {})

    def test_failed_delete_stays_charged_and_foreign_sandbox_is_untouched(self):
        with tempfile.TemporaryDirectory() as directory:
            s = Store(directory)
            self.addCleanup(s.close)
            s.put_task('task-001', 'g', 'fixture', {})
            work = s.enqueue('verify', 'task-001', 'g', {})
            claim = s.claim('verify', 'fixture')
            e = Execution(s, claim)
            s.reserve('our-verifier', work, claim['token'], 'sandbox', 1)
            with patch('pipeline.verification.gateway.inventory', return_value=[]), patch.object(e, 'os', return_value=subprocess.CompletedProcess([], 1)):
                retire(e, 'our-verifier')
            self.assertEqual(s.db.execute('SELECT state FROM resources WHERE name=?', ('our-verifier',)).fetchone()[0], 'deleting')
            foreign = [{'name': 'our-verifier', 'labels': {'gyre.dev/pipeline': 'someone-else'}}]
            with patch('pipeline.verification.gateway.inventory', return_value=foreign), patch.object(e, 'os', side_effect=AssertionError('must not delete foreign')):
                with self.assertRaises(RuntimeError):
                    retire(e, 'our-verifier')


if __name__ == '__main__':
    unittest.main()
