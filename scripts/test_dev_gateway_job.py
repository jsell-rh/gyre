"""Inventory must be complete before it can release compute capacity."""
import importlib.util
import json
from pathlib import Path
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('gateway_job', Path(__file__).with_name('dev-gateway-job.py'))
job = importlib.util.module_from_spec(spec); spec.loader.exec_module(job)


class InventoryTest(unittest.TestCase):
    def test_pagination_collects_every_page_and_ownership(self):
        pages = ['2026-10-08 WARN openshell_cli::tls: TLS certificate verification is disabled\n' + json.dumps({'sandboxes': [{'name': 'first', 'labels': {'gyre.dev/controller': 'owner'}, 'phase': 'Pending'}], 'next_page_token': 'next'}),
                 json.dumps({'sandboxes': [{'sandbox': {'metadata': {'name': 'second'}, 'status': {'phase': 'Ready'}}}]})]
        with patch.object(job, 'invoke', side_effect=pages) as cli:
            result = job.inventory()
        self.assertEqual([item['name'] for item in result], ['first', 'second'])
        self.assertEqual(result[0]['labels']['gyre.dev/controller'], 'owner')
        self.assertIn('--page-token', cli.call_args.args[0])

    def test_unknown_schema_or_truncated_page_cannot_free_resources(self):
        for value in ({'unexpected': []}, {'items': [{'name': 'sandbox'}] * 100}, {'items': [None]}):
            with self.assertRaises(ValueError):
                job.decode_page(value)
        with patch.object(job, 'invoke', return_value=json.dumps({'items': [], 'next_page_token': 'same'})):
            with self.assertRaisesRegex(ValueError, 'repeated'):
                job.inventory()


if __name__ == '__main__':
    unittest.main()
