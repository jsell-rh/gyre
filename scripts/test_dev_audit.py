"""Fidelity records must reject hollow claims and replay failing probes."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('audit', Path(__file__).with_name('dev-audit-check.py'))
audit = importlib.util.module_from_spec(spec); spec.loader.exec_module(audit)


class FidelityTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        subprocess.run(['git', 'init', '-q', str(self.root)], check=True)
        for key, value in [('user.name', 'Test'), ('user.email', 'test@example.com'), ('commit.gpgsign', 'false'), ('core.hooksPath', '/dev/null')]:
            subprocess.run(['git', '-C', str(self.root), 'config', key, value], check=True)
        for path, body in {'crates/app/src/lib.rs': 'fn enforce() {}\n', 'specs/tasks/task-002.md': '---\nprogress: needs-revision\n---\nImplement enforcement.\n',
                           'specs/coverage/system/example.md': '| 1 | Enforce | 2 | verified | task-001 | |\n| 2 | Reject | 2 | task-assigned | task-002 | |\n'}.items():
            file = self.root / path; file.parent.mkdir(parents=True, exist_ok=True); file.write_text(body)
        subprocess.run(['git', '-C', str(self.root), 'add', '.'], check=True)
        subprocess.run(['git', '-C', str(self.root), 'commit', '-qm', 'base'], check=True)
        self.previous = Path.cwd(); os.chdir(self.root); self.addCleanup(os.chdir, self.previous)
        self.contract = {'task': 'task-003', 'coverage': 'specs/coverage/system/example.md', 'rows': [{'id': 1, 'tasks': ['task-001']}, {'id': 2, 'tasks': ['task-002']}],
                         'reserved_tasks': ['task-004'], 'code_generation': audit.coverage.code_generation(audit.git, 'HEAD')}
        self.report = {'version': 1, 'task': 'task-003', 'code_generation': self.contract['code_generation'],
                       'findings': [{'row': 1, 'status': 'verified', 'explanation': 'real enforcement', 'production': [{'path': 'crates/app/src/lib.rs', 'symbol': 'enforce'}],
                                     'probes': [{'argv': ['cargo', 'test', 'enforcement'], 'timeout': 30}]},
                                    {'row': 2, 'status': 'gap', 'explanation': 'rejection absent', 'tasks': ['task-002']}]}
        self.path = self.root / 'specs/reviews/audit-task-003.json'; self.path.parent.mkdir(parents=True)

    def validate(self):
        self.path.write_text(json.dumps(self.report))
        return audit.validate(self.contract, self.root)

    def test_verified_claim_requires_production_and_acceptance_evidence(self):
        self.assertEqual(len(self.validate()), 2)
        self.report['findings'][0]['probes'] = []
        with self.assertRaisesRegex(ValueError, 'production entry points'):
            self.validate()
        self.report['findings'][0]['probes'] = [{}]
        self.report['findings'][0]['production'][0]['symbol'] = 'imaginary'
        with self.assertRaisesRegex(ValueError, 'symbol does not exist'):
            self.validate()

    def test_open_gap_cannot_point_to_completed_task(self):
        (self.root / 'specs/tasks/task-002.md').write_text('---\nprogress: complete\n---\n')
        with self.assertRaisesRegex(ValueError, 'claims completion'):
            self.validate()

    def test_acceptance_probe_must_pass_and_match_real_tests(self):
        tool = self.root / 'cargo'; tool.write_text('#!/bin/sh\necho "test result: ok. 0 passed; 0 failed"\n'); tool.chmod(0o755)
        with patch.dict(os.environ, {'PATH': str(self.root) + ':' + os.environ['PATH']}):
            with self.assertRaisesRegex(ValueError, 'no passing Rust tests'):
                audit.replay(self.validate(), self.root)
            tool.write_text('#!/bin/sh\necho "test result: FAILED. 0 passed; 1 failed"\nexit 1\n')
            with self.assertRaisesRegex(ValueError, 'probe failed'):
                audit.replay(self.validate(), self.root)

    def test_unknown_and_duplicate_rows_cannot_satisfy_audit(self):
        self.report['findings'].append(self.report['findings'][0])
        with self.assertRaisesRegex(ValueError, 'every assigned coverage row once'):
            self.validate()


if __name__ == '__main__':
    unittest.main()
