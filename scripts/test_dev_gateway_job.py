"""Inventory must be complete before it can release compute capacity."""
import importlib.util
import json
from pathlib import Path
import unittest
import tempfile
import subprocess
import sys
import os
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


class AuthenticationTest(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name)

    def test_proactive_refresh_is_shared_instead_of_repeated_on_every_operation(self):
        with patch.object(job,'invoke',return_value='ok') as cli:
            job.login(self.root);job.login(self.root)
        self.assertEqual(sum(c.args[0][:2]==['gateway','login'] for c in cli.call_args_list),1)
        self.assertFalse(job.auth_status(self.root)['using_cached'])

    def test_refresh_failure_retries_with_backoff_while_usable_cached_auth_keeps_work_running(self):
        def invoke(args,**kwargs):
            if args[:2]==['gateway','login']:raise RuntimeError('SECRET_SENTINEL token service unavailable')
            return '[]'
        with patch.object(job,'invoke',side_effect=invoke) as cli:
            job.login(self.root);job.login(self.root)
        value=job.auth_status(self.root)
        self.assertTrue(value['using_cached']);self.assertEqual(value['failures'],1)
        self.assertEqual(sum(c.args[0][:2]==['gateway','login'] for c in cli.call_args_list),1)
        self.assertNotIn('SECRET_SENTINEL',json.dumps(value))

    def test_known_expired_token_never_falls_back_to_cache_or_hammers_token_service(self):
        with patch.object(job,'invoke',side_effect=RuntimeError('mint unavailable')) as cli:
            with self.assertRaisesRegex(RuntimeError,'refresh failed'):job.login(self.root,force=True)
            with self.assertRaisesRegex(RuntimeError,'backing off'):job.login(self.root,force=True)
        self.assertEqual(cli.call_count,1)
        self.assertFalse(job.auth_status(self.root)['using_cached'])

    def test_recovered_token_service_clears_degraded_state_after_backoff(self):
        with patch.object(job,'invoke',side_effect=lambda args,**kw: (_ for _ in ()).throw(RuntimeError()) if args[:2]==['gateway','login'] else '[]'):
            job.login(self.root)
        value=job.auth_status(self.root);value['retry_at']=0
        job.auth_path(self.root,job.gateway_name()).write_text(json.dumps(value))
        with patch.object(job,'invoke',return_value='ok'):job.login(self.root)
        self.assertFalse(job.auth_status(self.root)['using_cached'])
        self.assertEqual(job.auth_status(self.root)['failures'],0)

    def test_concurrent_processes_mint_once_under_the_same_private_lock(self):
        backend=self.root/'openshell'
        backend.write_text('''#!/usr/bin/python3
import os,sys,pathlib
p=pathlib.Path(os.environ['AUTH_TEST_CALLS'])
with p.open('a') as f:f.write(' '.join(sys.argv[1:])+'\\n')
print('[]')
''');backend.chmod(0o700)
        env={**os.environ,'OPENSHELL':str(backend),'GYRE_OPEN_GATEWAY':'private-test',
             'AUTH_TEST_CALLS':str(self.root/'calls')}
        script="from pathlib import Path;from pipeline.catalog import helper;import sys;helper('dev-gateway-job').login(Path(sys.argv[1]))"
        processes=[subprocess.Popen([sys.executable,'-c',script,str(self.root)],cwd=Path(__file__).parent,
                    env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True) for _ in range(8)]
        for p in processes:
            _,error=p.communicate(timeout=15);self.assertEqual(p.returncode,0,error)
        self.assertEqual(sum(line.startswith('gateway login') for line in (self.root/'calls').read_text().splitlines()),1)


if __name__ == '__main__':
    unittest.main()
