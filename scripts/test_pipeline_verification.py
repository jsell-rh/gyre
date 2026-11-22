"""Fail-closed receipts, real process failure/timeout, and shared gateway capacity."""
import importlib.util
import base64
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).parent))
from pipeline.execution import Execution, Retry, Wait, Attention
from pipeline.store import Store
from pipeline.verification import COMMANDS, deadline_error, decode_poll, run, retire, validate_receipt

spec = importlib.util.spec_from_file_location('remote_verify', Path(__file__).with_name('pipeline-verify.py'))
remote = importlib.util.module_from_spec(spec)
spec.loader.exec_module(remote)


class VerificationTests(unittest.TestCase):
    def test_unknown_health_reconnects_without_new_build_or_permanent_hold(self):
        with tempfile.TemporaryDirectory() as directory:
            store=Store(directory);self.addCleanup(store.close)
            store.put_task('task-001','g','body',{})
            store.enqueue('verify','task-001','g',{})
            execution=Execution(store,store.claim('verify','verifier'))
            identity={'candidate':'candidate','head':'head','base':'head','tree':'tree'}
            receipt=identity|{'passed':False,'checks':[{'command':COMMANDS[0],'exit_code':1}]}
            polls=[];launches=[]
            def transport(name,*args,**kwargs):
                if 'launch' in args:launches.append(name)
                if 'poll' not in args:return subprocess.CompletedProcess([],0,'','')
                polls.append(args[args.index('--offset')+1])
                if 1<len(polls)<9:raise RuntimeError('temporary gateway transport outage')
                value={'offset':3 if len(polls)==1 else 6,'log':base64.b64encode(b'abc' if len(polls)==1 else b'def').decode(),
                       'alive':len(polls)==1,'receipt':None if len(polls)==1 else receipt}
                return subprocess.CompletedProcess([],0,'GYRE_VERIFY_POLL '+json.dumps(value),'')
            with patch.object(execution,'login'),patch.object(execution,'wait_ready'),patch.object(execution,'remote',side_effect=transport),patch.object(execution,'os',return_value=subprocess.CompletedProcess([],0,'','')) as commands,patch.object(execution.cancelled,'wait',return_value=False),patch('pipeline.verification.gateway.inventory',return_value=[]),patch('pipeline.runtime.snapshot',return_value=None):
                result=run(execution,Path(directory),identity)
            self.assertFalse(result['passed'])
            self.assertEqual(len(launches),1)
            self.assertEqual(polls,['0']+['3']*8)
            self.assertIn('abcdef',execution.log.read_text())
            self.assertEqual(sum(c.args[:2]==('sandbox','create') for c in commands.call_args_list),1)
            self.assertIsNone(store.setting(store.runtime_hold_key('task-001','verify','g')))

    def test_exhausted_budget_does_not_restart_the_same_build_on_main_advance(self):
        with tempfile.TemporaryDirectory() as directory:
            store=Store(directory);self.addCleanup(store.close)
            store.put_task('task-001','g','body',{})
            work=store.enqueue('verify','task-001','g',{'base':'old'})
            claim=store.claim('verify','verifier')
            error=deadline_error()
            store.retry(work,claim['token'],{'category':error.category,'message':str(error)},attention=error.attention)
            newer=store.enqueue('verify','task-001','g',{'base':'new'})
            self.assertIsNone(store.claim('verify','verifier',ident=newer))
            from pipeline.catalog import helper
            adapter=helper('dispatch-adapter')
            adapter.control(store,{'scope':adapter.scope(store),'key':work,'run_id':f"{work}:{claim['token']}"})
            self.assertIsNotNone(store.claim('verify','verifier',ident=newer))

    def test_cli_diagnostics_do_not_corrupt_framed_poll(self):
        value = {'offset': 0, 'log': '', 'receipt': None, 'alive': True}
        frame = 'GYRE_VERIFY_POLL ' + json.dumps(value)
        self.assertEqual(decode_poll('2026-11-02 WARN tls verification disabled\n' + frame + '\n'), value)
        for invalid in ('diagnostics only', frame + '\n' + frame):
            with self.assertRaises(ValueError):
                decode_poll(invalid)

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

    def test_rejected_receipt_survives_cleanup_with_specific_source_diagnostics(self):
        with tempfile.TemporaryDirectory() as directory:
            s=Store(directory);self.addCleanup(s.close)
            s.put_task('task-001','g','body',{})
            work=s.enqueue('verify','task-001','g',{})
            execution=Execution(s,s.claim('verify','verifier'))
            identity={'candidate':'candidate','head':'head','base':'head','tree':'tree'}
            receipt=identity | {'passed':False,'infrastructure':True,'error_kind':'source_integrity',
                'dirty_paths':['scripts/python-callgraph.py'],'checks':[{'command':c,'exit_code':0} for c in COMMANDS]}
            def transport(name,*args,**kwargs):
                output=''
                if 'poll' in args:
                    output='GYRE_VERIFY_POLL '+json.dumps({'offset':0,'log':'','receipt':receipt,'alive':False})
                return subprocess.CompletedProcess([],0,output,'')
            with patch.object(execution,'login'),patch.object(execution,'wait_ready'),patch.object(execution,'remote',side_effect=transport),patch.object(execution,'os',return_value=subprocess.CompletedProcess([],0,'','')) as commands,patch('pipeline.verification.gateway.inventory',return_value=[]):
                with self.assertRaisesRegex(Attention,'python-callgraph.py'):
                    run(execution,Path(directory),identity)
            raw=execution.directory/'remote-verification-raw.json'
            self.assertEqual(json.loads(raw.read_text()),receipt)
            self.assertEqual(raw.stat().st_mode & 0o777,0o600)
            self.assertFalse((execution.directory/'remote-verification.json').exists())
            self.assertTrue(any(c.args[:2]==('sandbox','delete') for c in commands.call_args_list))

    def test_lost_launch_ack_reuses_the_same_verifier_allocation(self):
        with tempfile.TemporaryDirectory() as directory:
            store=Store(directory);self.addCleanup(store.close)
            store.put_task('task-001','g','body',{})
            store.enqueue('verify','task-001','g',{})
            execution=Execution(store,store.claim('verify','verifier'))
            identity={'candidate':'candidate','head':'head','base':'head','tree':'tree'}
            receipt=identity|{'passed':False,'checks':[{'command':COMMANDS[0],'exit_code':1}]}
            launches=[]
            def transport(name,*args,**kwargs):
                if 'launch' in args:
                    launches.append(name)
                    if len(launches)==1:raise subprocess.TimeoutExpired('lost acknowledgement',30)
                output='GYRE_VERIFY_POLL '+json.dumps({'offset':0,'log':'','receipt':receipt,'alive':False}) if 'poll' in args else ''
                return subprocess.CompletedProcess([],0,output,'')
            with patch.object(execution,'login'),patch.object(execution,'wait_ready'),patch.object(execution.cancelled,'wait',return_value=False),patch.object(execution,'remote',side_effect=transport),patch.object(execution,'os',return_value=subprocess.CompletedProcess([],0,'','')) as calls,patch('pipeline.verification.gateway.inventory',return_value=[]):
                observed=run(execution,Path(directory),identity)
            self.assertFalse(observed['passed'])
            self.assertEqual(len(launches),2)
            self.assertEqual(len(set(launches)),1)
            self.assertEqual(sum(c.args[:2]==('sandbox','create') for c in calls.call_args_list),1)

    def test_source_guard_uses_actual_paths_not_substring_whitelists(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            def git(*args):return subprocess.check_output(['git',*args],cwd=root,text=True).strip()
            git('init','-q');git('config','user.name','Test');git('config','user.email','test@example.com')
            git('-c','core.hooksPath=/dev/null','-c','commit.gpgsign=false','commit','--allow-empty','-qm','source')
            identity={'head':git('rev-parse','HEAD'),'tree':git('rev-parse','HEAD^{tree}')}
            (root/'web/dist').mkdir(parents=True);(root/'web/dist/generated.js').write_text('build output')
            with patch.object(remote,'CHECKOUT',root):
                self.assertIsNone(remote.source_integrity(identity))
                (root/'nested/web/dist').mkdir(parents=True)
                (root/'nested/web/dist/source.js').write_text('unexpected source')
                (root/'scripts').mkdir();(root/'scripts/python-callgraph.py').write_text('leftover script')
                failure=remote.source_integrity(identity)
                self.assertEqual(failure['dirty_paths'],['nested/web/dist/source.js','scripts/python-callgraph.py'])
                self.assertEqual(failure['observed_head'],identity['head'])

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

    def test_transient_infrastructure_retries_in_same_workspace(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            script = root / 'transient.py'
            script.write_text('from pathlib import Path\nimport sys\np=Path("attempt")\n'
                              'n=int(p.read_text()) if p.exists() else 0\np.write_text(str(n+1))\n'
                              'sys.exit(77 if n==0 else 0)\n')
            receipt = root / 'result.json'
            remote.run_checks([([sys.executable, str(script)], directory, 10)], receipt, {}, os.environ.copy(), retry_delays=(0,))
            value = json.loads(receipt.read_text())
            self.assertTrue(value['passed'])
            self.assertEqual(value['checks'][0]['attempts'], 2)
            self.assertEqual((root / 'attempt').read_text(), '2')

    def test_uncertain_create_waits_for_the_same_allocation_instead_of_recreating(self):
        with tempfile.TemporaryDirectory() as directory:
            s=Store(directory);self.addCleanup(s.close);s.put_task('task-001','g','body',{})
            s.set_setting('verification_timeout',14400)
            work=s.enqueue('verify','task-001','g',{})
            execution=Execution(s,s.claim('verify','verifier'))
            identity={'candidate':'candidate','head':'head','base':'head','tree':'tree'}
            name=None;inventory_calls=0
            def invoke(*args,**kwargs):
                nonlocal name
                if args[:2]==('sandbox','create'):
                    self.assertIn('--no-tty',args)
                    name=args[args.index('--name')+1]
                    raise subprocess.TimeoutExpired('lost create acknowledgement',1)
                return subprocess.CompletedProcess(args,0,'','')
            def inventory():
                nonlocal inventory_calls
                inventory_calls+=1
                if inventory_calls in (1,4,5):return []
                return [{'name':name,'phase':'Provisioning' if inventory_calls==2 else 'Ready','labels':{'gyre.dev/pipeline':s.setting('owner')}}]
            def remote(*args,**kwargs):
                value={'offset':0,'log':'','receipt':identity|{'passed':False,'checks':[{'command':COMMANDS[0],'exit_code':1}]},'alive':False}
                return subprocess.CompletedProcess([],0,'GYRE_VERIFY_POLL '+json.dumps(value),'')
            with patch.object(execution,'login'),patch.object(execution,'os',side_effect=invoke) as operations,patch.object(execution,'remote',side_effect=remote),patch.object(execution.cancelled,'wait',return_value=False),patch('pipeline.verification.gateway.inventory',side_effect=inventory):
                receipt=run(execution,Path(directory),identity)
            self.assertFalse(receipt['passed'])
            job=json.loads((execution.directory/'verification-input/verify-job.json').read_text())
            self.assertEqual(job['timeout_seconds'],14400)
            self.assertEqual(sum(c.args[:2]==('sandbox','create') for c in operations.call_args_list),1)
            self.assertIn('acknowledgement timed out',execution.log.read_text())
            self.assertEqual(s.compute_used(),0)

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
