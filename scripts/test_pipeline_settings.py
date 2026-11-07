"""Operator settings must change real admission, persist, and remain isolated."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

from pipeline.settings import configuration, save_configuration
from pipeline.store import Store


class SettingsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.store = Store(Path(self.temp.name) / 'gyre')
        self.addCleanup(self.store.close)
        self.scope = 'gyre/' + self.store.setting('owner')

    def save(self, key, value, **extra):
        return save_configuration(self.store, dict(scope=self.scope, key=key, value=value, **extra))

    def work(self, name):
        self.store.put_task(name, 'g', 'spec', {})
        return self.store.enqueue('implement', name, 'g', {})

    def test_protocol_persists_and_refreshes_browser_cli_changes_without_launching(self):
        cmd = [sys.executable, str(Path(__file__).with_name('dispatch-adapter.py'))]
        args = ['--state', str(self.store.directory)]
        def call(action, data=None):
            result = subprocess.run(cmd + [action] + args, input=json.dumps(data), text=True,
                                    capture_output=True, timeout=15, check=True)
            return json.loads(result.stdout)
        reply = call('settings-save', dict(scope=self.scope, key='slots', value='45', expected='8'))
        self.assertEqual(reply['value'], 45)
        self.store.set_setting('slots', 37)  # Same write path as CLI/browser.
        rows = call('rows')
        self.assertEqual(rows['capacity'], 37)
        self.assertEqual(next(r['value'] for r in rows['settings'] if r['key'] == 'slots'), '37')
        reopened = Store(self.store.directory)
        try:
            self.assertEqual(reopened.setting('slots'), 37)
        finally:
            reopened.close()
        self.assertEqual(self.store.db.execute('SELECT count(*) FROM resources').fetchone()[0], 0)

    def test_edits_preserve_running_claims_and_admission_obeys_new_capacity(self):
        self.save('slots', '3')
        first = self.work('task-001')
        claim = self.store.claim('implement', 'active', concurrency=2, capacity=3)
        self.work('task-002')
        self.save('slots', '0')
        self.assertIsNone(self.store.claim('implement', 'new', concurrency=2, capacity=self.store.setting('slots')))
        row = self.store.db.execute('SELECT state,token,owner FROM work WHERE id=?', (first,)).fetchone()
        self.assertEqual(tuple(row), ('claimed', claim['token'], 'active'))
        self.save('slots', '3')
        self.assertIsNotNone(self.store.claim('implement', 'new', concurrency=2, capacity=self.store.setting('slots')))

    def test_partial_stage_limits_do_not_erase_siblings(self):
        self.save('limit.implement', '3')
        self.save('limit.review', '2')
        self.assertEqual(self.store.setting('limits'), {'implement': 3, 'review': 2})
        self.save('limit.implement', '')
        self.assertEqual(self.store.setting('limits'), {'review': 2})
        self.save('model_timeout', '14400')
        self.assertEqual(self.store.setting('model_timeout'), 14400)

    def test_foreign_scope_readonly_invalid_and_stale_edits_are_rejected(self):
        before = self.store.db.execute('SELECT * FROM settings ORDER BY key').fetchall()
        for key, value in [('slots', '-1'), ('slots', '1.5'), ('slots', '1001'), ('slots', '1e2'),
                           ('slots', '９'), ('limit.review', '0'), ('model_timeout', '5'),
                           ('admission_paused', 'yes'), ('gateway', 'hypershell'), ('ENMAAS_API_KEY', 'secret')]:
            with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                self.save(key, value)
        with self.assertRaises(ValueError):
            save_configuration(self.store, dict(scope='gyre/foreign', key='slots', value='1'))
        with self.assertRaises(ValueError):
            self.save('slots', '1', expected='99')
        self.assertEqual(before, self.store.db.execute('SELECT * FROM settings ORDER BY key').fetchall())
        other = Store(Path(self.temp.name) / 'hypershell')
        try:
            self.save('slots', '45')
            self.assertIsNone(other.setting('slots'))
        finally:
            other.close()

    def test_backoff_settings_affect_future_schedules_only(self):
        ident = self.work('task-001')
        claim = self.store.claim('implement', 'active')
        self.save('retry_base_seconds', '100')
        self.save('retry_max_seconds', '120')
        with patch('pipeline.store.random.uniform', return_value=1):
            before = time.time()
            self.store.retry(ident, claim['token'], {'category': 'infrastructure'})
        scheduled = self.store.db.execute('SELECT retry_at FROM work WHERE id=?', (ident,)).fetchone()[0]
        self.assertGreaterEqual(scheduled, before + 100)
        self.assertLess(scheduled, before + 101)
        self.save('retry_base_seconds', '1')
        self.assertEqual(scheduled, self.store.db.execute('SELECT retry_at FROM work WHERE id=?', (ident,)).fetchone()[0])
        with self.assertRaises(ValueError):
            self.save('retry_max_seconds', '0')
        with self.assertRaises(ValueError):
            self.save('retry_base_seconds', '121')


if __name__ == '__main__':
    unittest.main()
