"""Exercise crash windows and cleanup against the real ledger and handlers."""
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
from pipeline.store import Store
from pipeline.execution import Execution
from pipeline.stages import cleanup, implement, prompt

spec = importlib.util.spec_from_file_location('pipeline_cli', Path(__file__).with_name('dev-pipeline.py'))
cli = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cli)


class CrashTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.store = Store(self.temp.name)
        self.addCleanup(self.store.close)
        self.store.put_task('task-001', 'g', 'body', {'dependencies': []})
        self.work = self.store.enqueue('implement', 'task-001', 'g', {})
        self.claim = self.store.claim('implement', 'actor')

    def test_maximum_memory_attention_cannot_be_bypassed_by_checkpoint_or_new_main(self):
        from pipeline.execution import Attention
        execution=Execution(self.store,self.claim)
        (execution.directory/'recovery.json').write_text('{}')
        def exhausted(*_):
            error=Attention('OOM at maximum tier');error.category='sandbox_oom';raise error
        with patch.dict(cli.HANDLERS,{'implement':exhausted}):
            cli.run_claim(self.store,self.claim)
        self.assertEqual(self.store.db.execute('SELECT state FROM work WHERE id=?',(self.work,)).fetchone()[0],'attention')
        next_work=self.store.enqueue('implement','task-001','g',{'base':'advanced-main'})
        self.assertIsNone(self.store.claim('implement','cannot-bypass',ident=next_work))
        self.assertTrue(self.store.runtime_held({'task':'task-001','stage':'implement','generation':'g'}))

    def test_inventory_cannot_release_inflight_creation(self):
        self.store.reserve('inflight', self.work, self.claim['token'], 'sandbox', 1)
        with patch.object(cli.gateway if hasattr(cli, 'gateway') else __import__('pipeline.execution', fromlist=['gateway']).gateway, 'login'), patch('pipeline.execution.gateway.inventory', return_value=[]):
            cli.discover_cleanup(self.store)
        self.assertEqual(self.store.db.execute('SELECT state FROM resources').fetchone()[0], 'intent')
        self.assertFalse(self.store.reserve('duplicate', self.work, self.claim['token'], 'sandbox', 1))

    def test_crash_after_publication_adopts_receipt_without_gateway(self):
        prior = self.store.directory / 'attempts' / self.work / '0'
        (prior / 'bundle').mkdir(parents=True)
        (prior / 'bundle/job.json').write_text(json.dumps({'stage': 'implement', 'task': 'task-001', 'branch': 'private'}))
        (prior / 'outcome.json').write_text(json.dumps({'published': True, 'head': 'source', 'valid': True}))
        execution = Execution(self.store, self.claim)
        with patch.object(execution, 'login', side_effect=AssertionError('must not allocate')):
            result = execution.cloud_step(self.store.task('task-001'), '')
        self.assertEqual(result['head'], 'source')
        self.assertEqual(json.loads((execution.directory / 'outcome.json').read_text())['head'], 'source')
        self.assertEqual(self.store.db.execute('SELECT count(*) FROM resources').fetchone()[0], 0)

    def test_truncated_checkpoint_task_restores_contract_without_discarding_candidate(self):
        body='---\ntitle: Real task\nspec_ref: fixture\ndepends_on: []\nprogress: not-started\n---\n\nRequired behavior.\n'
        self.store.put_task('task-001','g',body,{'candidate':'saved-source',
                            'task_override':'---\ntitle: Real task\n'})
        observed=[]
        def handler(execution,task):
            observed.append(task)
            return {},{},[]
        with patch.dict(cli.HANDLERS,{'implement':handler}), patch.object(cli,'eligible',return_value=True):
            cli.run_claim(self.store,self.claim)
        self.assertEqual(observed[0]['body'],body)
        self.assertEqual(observed[0]['data']['candidate'],'saved-source')
        self.assertEqual(self.store.db.execute("SELECT count(*) FROM events WHERE kind='checkpoint_contract_restored'").fetchone()[0],1)

    def test_verifier_safety_failure_stops_retries_and_releases_lane_until_operator_retry(self):
        from pipeline.execution import Attention
        self.store.finish(self.work,self.claim['token'],{})
        self.store.put_task('task-001','g','body',{'candidate':'source','dependencies':[]})
        work=self.store.enqueue('verify','task-001','g',{'candidate':'source','base':'main'})
        claim=self.store.claim('verify','verifier')
        with patch.dict(cli.HANDLERS,{'verify':lambda *_: (_ for _ in ()).throw(Attention('source changed'))}):
            cli.run_claim(self.store,claim)
        self.assertEqual(self.store.db.execute('SELECT state FROM work WHERE id=?',(work,)).fetchone()[0],'attention')
        self.assertIsNone(self.store.integration_lane())
        new=self.store.enqueue('verify','task-001','g',{'candidate':'source','base':'advanced-main'})
        self.assertIsNone(self.store.claim('verify','cannot-bypass',ident=new))
        self.store.put_task('task-002','g','body',{'candidate':'other'})
        self.store.enqueue('verify','task-002','g',{'candidate':'other'})
        other=self.store.claim('verify','next-task')
        self.assertEqual(other['task'],'task-002')
        self.store.finish(other['id'],other['token'],{}, {'delivered':{'sha':'merged'}})
        with self.store.transaction():
            self.store.db.execute("UPDATE work SET state='ready',retry_at=0 WHERE id=?",(work,))
        self.assertIsNotNone(self.store.claim('verify','operator-retry',ident=work))

    def test_adopted_partial_implementation_without_current_log_is_recorded_once(self):
        from pipeline.fullsend import adopt
        # A checkpoint exists, but no process has run in this resumed attempt.
        prior=self.store.directory/'attempts'/self.work/'0';(prior/'bundle').mkdir(parents=True)
        (prior/'bundle/job.json').write_text(json.dumps({'stage':'implement','task':'task-001','branch':'private'}))
        body='---\ntitle: Task\nprogress: not-started\ndepends_on: []\nspec_ref: fixture\n---\nRequired behavior.\n'
        self.store.put_task('task-001','g',body,{'dependencies':[]})
        outcome={'published':True,'head':'retained-source','valid':False,'agent_exit':1,'branch':'private','task_body':body}
        (prior/'outcome.json').write_text(json.dumps(outcome))
        execution=Execution(self.store,self.claim)
        result=adopt(execution,self.store.task('task-001'))
        self.assertEqual(result['head'],'retained-source')
        self.assertIn('Adopted published checkpoint',execution.log_tail())
        self.assertEqual(json.loads((execution.directory/'adopted-from.json').read_text())['attempt'],0)
        # Tail absence is diagnostic, never loss of a recovered source receipt.
        execution.log.unlink()
        self.assertEqual(execution.log_tail(),'')

    def test_terminal_readiness_does_not_hold_dead_compute(self):
        from pipeline.execution import Retry
        execution = Execution(self.store, self.claim)
        with patch('pipeline.execution.gateway.inventory', return_value=[{'name': 'pod', 'phase': 'Error'}]), patch.object(execution.cancelled, 'wait', side_effect=AssertionError('terminal pods must not wait')):
            with self.assertRaisesRegex(Retry, 'terminal infrastructure phase Error'):
                execution.wait_ready('pod')
        self.assertEqual(json.loads((execution.directory / 'phase.json').read_text())['phase'], 'InfrastructureFailed')

    def test_pending_readiness_keeps_its_pod_and_backs_off(self):
        execution = Execution(self.store, self.claim)
        inventory = [[{'name': 'pod', 'phase': phase}] for phase in ('Pending', 'Provisioning', 'Ready')]
        with patch('pipeline.execution.gateway.inventory', side_effect=inventory), patch.object(execution.cancelled, 'wait', return_value=False) as wait:
            execution.wait_ready('pod')
        self.assertEqual([call.args[0] for call in wait.call_args_list], [5, 10])

    def test_pending_allocation_survives_old_thirty_minute_window(self):
        execution=Execution(self.store,self.claim)
        inventory=[[{'name':'pod','phase':'Pending'}],[{'name':'pod','phase':'Ready'}]]
        with patch('pipeline.execution.time.monotonic',side_effect=[0,1801]),patch('pipeline.execution.gateway.inventory',side_effect=inventory),patch.object(execution.cancelled,'wait',return_value=False) as wait:
            execution.wait_ready('pod')
        self.assertEqual(wait.call_count,1)

    def test_readiness_inventory_outage_keeps_the_same_allocation(self):
        execution=Execution(self.store,self.claim)
        responses=[RuntimeError('temporary gateway outage'),[{'name':'pod','phase':'Pending'}],[{'name':'pod','phase':'Ready'}]]
        with patch('pipeline.execution.gateway.inventory',side_effect=responses),patch.object(execution.cancelled,'wait',return_value=False) as wait:
            execution.wait_ready('pod')
        self.assertEqual([c.args[0] for c in wait.call_args_list],[5,10])
        phases=[json.loads(r['detail']) for r in self.store.db.execute("SELECT detail FROM events WHERE kind='phase'")]
        self.assertEqual([p['phase'] for p in phases],['WaitingForGateway','WaitingForInfrastructure'])
        self.assertTrue(all(p['sandbox']=='pod' for p in phases))

    def test_failed_capture_still_purges_expensive_compute(self):
        self.store.reserve('pod', self.work, self.claim['token'], 'sandbox', 1)
        self.store.finish(self.work, self.claim['token'], {})
        ident = self.store.enqueue('cleanup', 'task-001', 'g', {'resource': 'pod'})
        claim = self.store.claim('cleanup', 'cleaner')
        execution = Execution(self.store, claim)
        item = {'name': 'pod', 'phase': 'Ready', 'labels': {'gyre.dev/pipeline': self.store.setting('owner')}}
        with patch.object(execution, 'login'), patch('pipeline.stages.gateway.inventory', side_effect=[[item], [], []]), patch.object(execution, 'remote', side_effect=subprocess.TimeoutExpired('capture', 30)), patch.object(execution, 'os', return_value=subprocess.CompletedProcess([], 0, '', '')) as delete:
            result, _, _ = cleanup(execution, self.store.task('task-001'))
        self.assertEqual(result['deleted'], 'pod')
        delete.assert_any_call('sandbox', 'delete', 'pod', timeout=180, check=False)
        self.assertEqual(self.store.db.execute("SELECT state FROM resources WHERE name='pod'").fetchone()[0], 'absent')

    def test_unstarted_error_is_purged_without_unreachable_capture(self):
        self.store.reserve('pod', self.work, self.claim['token'], 'sandbox', 1)
        self.store.finish(self.work, self.claim['token'], {})
        self.store.enqueue('cleanup', 'task-001', 'g', {'resource': 'pod'})
        execution = Execution(self.store, self.store.claim('cleanup', 'cleaner'))
        item = {'name': 'pod', 'phase': 'Error', 'labels': {'gyre.dev/pipeline': self.store.setting('owner')}}
        with patch.object(execution, 'login'), patch('pipeline.stages.gateway.inventory', side_effect=[[item], [], []]), patch.object(execution, 'remote', side_effect=AssertionError('no source was staged')) as remote, patch.object(execution, 'os', return_value=subprocess.CompletedProcess([], 0, '', '')) as delete:
            cleanup(execution, self.store.task('task-001'))
        remote.assert_not_called()
        delete.assert_called_once_with('sandbox', 'delete', 'pod', timeout=180, check=False)
        self.assertEqual(self.store.db.execute("SELECT state FROM resources WHERE name='pod'").fetchone()[0], 'absent')

    def test_error_after_an_earlier_attachment_still_attempts_source_capture(self):
        self.store.reserve('pod', self.work, self.claim['token'], 'sandbox', 1)
        earlier = self.store.directory / 'attempts' / self.work / '0'
        earlier.mkdir(parents=True)
        (earlier / 'remote.offset').write_text('100')
        self.store.finish(self.work, self.claim['token'], {})
        self.store.enqueue('cleanup', 'task-001', 'g', {'resource': 'pod'})
        execution = Execution(self.store, self.store.claim('cleanup', 'cleaner'))
        item = {'name': 'pod', 'phase': 'Error', 'labels': {'gyre.dev/pipeline': self.store.setting('owner')}}
        with patch.object(execution, 'login'), patch('pipeline.stages.gateway.inventory', side_effect=[[item], [], []]), patch.object(execution, 'remote', side_effect=subprocess.TimeoutExpired('capture', 30)) as remote, patch.object(execution, 'os', return_value=subprocess.CompletedProcess([], 0, '', '')):
            cleanup(execution, self.store.task('task-001'))
        remote.assert_called_once()

    def test_deleted_compute_is_released_even_if_source_recovery_retries(self):
        self.store.reserve('gone', self.work, self.claim['token'], 'sandbox', 1)
        self.store.resource_state('gone', 'deleting')
        self.store.retry(self.work, self.claim['token'], {'message': 'bootstrap failed'})
        original = self.store.directory / 'attempts' / self.work / str(self.claim['token'])
        original.mkdir(parents=True)
        (original / 'recovery.json').write_text(json.dumps({'base': 'base'}))
        self.store.enqueue('cleanup', 'task-001', 'g', {'resource': 'gone'})
        execution = Execution(self.store, self.store.claim('cleanup', 'cleaner'))
        with patch.object(execution, 'login'), patch('pipeline.stages.gateway.inventory', return_value=[]), patch('pipeline.stages.checkout', return_value=self.store.directory), patch.object(execution, 'command', return_value=subprocess.CompletedProcess([], 0, 'tree\n', '')), patch('pipeline.recovery.restore', side_effect=ValueError('incomplete receipt')):
            with self.assertRaisesRegex(ValueError, 'incomplete receipt'):
                cleanup(execution, self.store.task('task-001'))
        self.assertEqual(self.store.db.execute("SELECT state FROM resources WHERE name='gone'").fetchone()[0], 'absent')
        self.assertTrue((original / 'recovery.json').exists())
        self.assertIsNone(self.store.task('task-001')['data'].get('candidate'))

    def merge_cleanup(self, observation):
        self.store.reserve('merge-old', self.work, self.claim['token'], 'merge', 1,
                           {'url': 'https://github.com/jsell-rh/gyre/pull/633'})
        self.store.db.execute("UPDATE work SET state='obsolete',token=token+1 WHERE id=?", (self.work,))
        self.store.enqueue('cleanup', 'task-001', 'g', {'resource': 'merge-old'})
        claim = self.store.claim('cleanup', 'cleaner')
        execution = Execution(self.store, claim)
        result = {'data': {'repository': {'pullRequest': observation}}}
        with patch.object(execution, 'command', return_value=subprocess.CompletedProcess([], 0, json.dumps(result), '')):
            return cleanup(execution, self.store.task('task-001'))

    def test_rejected_merge_does_not_permanently_block_other_deliveries(self):
        self.merge_cleanup({'state': 'OPEN', 'headRefOid': 'head', 'mergeCommit': None,
                            'autoMergeRequest': None, 'mergeQueueEntry': None})
        self.store.put_task('task-002', 'g', 'body', {})
        self.store.enqueue('publish', 'task-002', 'g', {})
        claim = self.store.claim('publish', 'next-publisher')
        self.assertTrue(self.store.reserve('merge-new', claim['id'], claim['token'], 'merge', 1))

    def test_merge_method_honors_repository_policy(self):
        from pipeline.stages import merge_method
        from pipeline.execution import Retry, Wait
        execution = Execution(self.store, self.claim)
        fields = ('allow_merge_commit', 'allow_squash_merge', 'allow_rebase_merge')
        for index, expected in enumerate(('--merge', '--squash', '--rebase')):
            policy = dict.fromkeys(fields, False)
            policy[fields[index]] = True
            with patch.object(execution, 'command', return_value=subprocess.CompletedProcess([], 0, json.dumps(policy), '')):
                self.assertEqual(merge_method(execution, 'jsell-rh/gyre'), expected)
        for policy, error in (({}, Retry), (dict.fromkeys(fields, False), Wait)):
            with patch.object(execution, 'command', return_value=subprocess.CompletedProcess([], 0, json.dumps(policy), '')):
                with self.assertRaises(error):
                    merge_method(execution, 'jsell-rh/gyre')

    def test_queued_merge_retains_its_global_permit(self):
        from pipeline.execution import Wait
        with self.assertRaises(Wait):
            self.merge_cleanup({'state': 'OPEN', 'headRefOid': 'head', 'mergeCommit': None,
                                'autoMergeRequest': None, 'mergeQueueEntry': {'id': 'entry'}})
        self.assertEqual(self.store.db.execute("SELECT state FROM resources WHERE name='merge-old'").fetchone()[0], 'intent')

    def test_unknown_merge_observation_retains_its_global_permit(self):
        from pipeline.execution import Retry
        with self.assertRaises(Retry):
            self.merge_cleanup({'state': 'OPEN', 'headRefOid': 'head'})
        self.assertEqual(self.store.db.execute("SELECT state FROM resources WHERE name='merge-old'").fetchone()[0], 'intent')

    def test_implementer_cannot_weaken_its_assigned_contract(self):
        body = '---\ntitle: Thing\nspec_ref: behavior.md\ndepends_on: []\nprogress: not-started\n---\n\n## Required behavior\nReject foreign tenants.\n'
        self.store.put_task('task-001', 'g', body, {'dependencies': []})
        execution = Execution(self.store, self.claim)
        execution.claim['input'] = {'base': 'base'}
        execution.log.write_text('agent completed')
        weakened = body.replace('not-started', 'ready-for-review').replace('Reject foreign tenants.', 'Allow every tenant.')
        receipt = {'task_body': weakened, 'valid': True, 'head': 'candidate', 'base': 'base', 'branch': 'private', 'agent_exit': 0}
        # This unit isolates contract rejection; source-progress and Git
        # ancestry are exercised against real repositories in efficiency tests.
        with patch('pipeline.stages.prompt', return_value='assignment'), patch.object(execution, 'cloud_step', return_value=receipt), \
                patch('pipeline.stages.checkout',return_value=self.store.directory), \
                patch.object(execution,'command',return_value=subprocess.CompletedProcess([],0,'source.rs\n','')):
            result, updates, _ = implement(execution, self.store.task('task-001'))
        self.assertEqual(result['task_body'], body)
        self.assertEqual(updates['repair']['category'], 'contract')
        self.assertIsNone(updates['review'])

    def test_reviewer_receives_original_ci_failure_after_repair_completes(self):
        body = '---\ntitle: Thing\nspec_ref: behavior.md\ndepends_on: []\nprogress: not-started\n---\n\n## Required behavior\nReject foreign tenants.\n'
        finding = {'category': 'ci', 'source': 'previous-head', 'detail': 'CI_FOREIGN_TENANT_FAILURE',
                   'candidate_log': str(self.store.directory / 'ci.log')}
        self.store.put_task('task-001', 'g', body,
                            {'dependencies': [], 'candidate_base': 'old-main', 'repair': finding})
        execution = Execution(self.store, self.claim)
        execution.claim['input'] = {'base': 'new-main'}
        receipt = {'task_body': body.replace('not-started', 'ready-for-review'), 'valid': True,
                   'head': 'fixed', 'base': 'new-main', 'branch': 'private', 'agent_exit': 0}
        with patch('pipeline.stages.prompt', return_value='assignment'), patch.object(execution, 'cloud_step', return_value=receipt):
            result, updates, findings = implement(execution, self.store.task('task-001'))
        self.store.finish(self.work, self.claim['token'], result, updates, findings)
        self.store.enqueue('review', 'task-001', 'g', {'candidate': 'fixed', 'base': 'new-main'})
        reviewer = Execution(self.store, self.store.claim('review', 'independent-reviewer'))
        self.assertIsNone(self.store.task('task-001')['data']['repair'])
        with patch.object(reviewer, 'command', return_value=subprocess.CompletedProcess([], 0, body, '')):
            assignment = prompt(reviewer, self.store.task('task-001'))
        self.assertIn('CI_FOREIGN_TENANT_FAILURE', assignment)
        self.assertIn('previous-head', assignment)

    def test_invalid_review_outcome_is_not_replayed_forever(self):
        prior = self.store.directory / 'attempts' / self.work / '0'
        (prior / 'bundle').mkdir(parents=True)
        (prior / 'bundle/job.json').write_text(json.dumps({'stage': 'review', 'task': 'task-001', 'branch': 'old'}))
        (prior / 'outcome.json').write_text(json.dumps({'published': True, 'head': 'source', 'valid': False}))
        claim = {**self.claim, 'stage': 'review'}
        execution = Execution(self.store, claim)
        with patch('pipeline.fullsend.binary', return_value=Path('/runner')), patch.object(execution, 'login', side_effect=RuntimeError('new model assignment required')):
            with self.assertRaisesRegex(RuntimeError, 'new model assignment required'):
                execution.cloud_step(self.store.task('task-001'), '')

    def test_cleanup_never_promotes_a_reviewers_checkpoint_to_implementation(self):
        self.store.reserve('finished-review', self.work, self.claim['token'], 'sandbox', 1)
        self.store.resource_state('finished-review', 'absent')
        self.store.db.execute("UPDATE work SET stage='review' WHERE id=?", (self.work,))
        self.store.retry(self.work, self.claim['token'], {'message': 'review host handoff interrupted'}, delay=0)
        prior = self.store.directory / 'attempts' / self.work / str(self.claim['token'])
        prior.mkdir(parents=True)
        (prior / 'recovery.json').write_text(json.dumps({'base': 'review-source'}))
        ident = self.store.enqueue('cleanup', 'task-001', 'g', {'resource': 'finished-review'}, priority=10000)
        claim = self.store.claim('cleanup', 'cleaner')
        execution = Execution(self.store, claim)
        with patch.object(execution, 'login'), patch('pipeline.stages.gateway.inventory', return_value=[]), patch('pipeline.stages.checkout', side_effect=AssertionError('must not promote review source')):
            _, updates, _ = cleanup(execution, self.store.task('task-001'))
        self.assertEqual(updates, {})
        self.assertIsNone(self.store.task('task-001')['data'].get('candidate'))

    def test_cleanup_purges_non_implementers_without_indexing_their_checkout(self):
        for stage in ('review','verify'):
            self.store.reserve('finished-'+stage,self.work,self.claim['token'],'sandbox',10)
        self.store.retry(self.work,self.claim['token'],{'message':'interrupted'},delay=0)
        for stage in ('review','verify'):
            with self.subTest(stage=stage):
                resource='finished-'+stage
                self.store.db.execute('UPDATE work SET stage=? WHERE id=?',(stage,self.work))
                ident=self.store.enqueue('cleanup','task-001','g',{'resource':resource})
                execution=Execution(self.store,self.store.claim('cleanup','cleaner',ident=ident))
                item={'name':resource,'phase':'Ready','labels':{'gyre.dev/pipeline':self.store.setting('owner')}}
                with patch.object(execution,'login'),patch('pipeline.stages.gateway.inventory',side_effect=[[item],[],[]]),patch.object(execution,'remote',return_value=subprocess.CompletedProcess([],0,'bounded log','')) as remote,patch.object(execution,'command',side_effect=AssertionError('non-implementer must not capture source')),patch.object(execution,'os',return_value=subprocess.CompletedProcess([],0,'','')) as operations:
                    _,updates,_=cleanup(execution,self.store.task('task-001'))
                self.assertEqual(updates,{})
                self.assertEqual(remote.call_count,1)
                self.assertIn('tail',remote.call_args.args)
                operations.assert_called_once_with('sandbox','delete',resource,timeout=180,check=False)
                self.assertEqual(self.store.db.execute('SELECT state FROM resources WHERE name=?',(resource,)).fetchone()[0],'absent')
                self.store.finish(ident,execution.claim['token'],{}, {})

    def test_crash_cleanup_stops_durable_agent_before_capturing_source(self):
        import sys,time
        workspace=Path(self.temp.name)/'workspace';root=workspace/'.gyre-agent-proof'
        root.mkdir(parents=True)
        helper=Path(__file__).with_name('pipeline-agent.py')
        (workspace/'pipeline-agent.py').write_bytes(helper.read_bytes())
        marker=workspace/'running'
        job={'argv':[sys.executable,'-c',f'from pathlib import Path; import time; Path({str(marker)!r}).touch(); time.sleep(60)'],
             'cwd':str(workspace),'timeout':60}
        (root/'command.json').write_text(json.dumps(job))
        subprocess.run([sys.executable,str(helper),'launch',str(root)],check=True,capture_output=True)
        self.addCleanup(lambda:subprocess.run([sys.executable,str(helper),'cancel',str(root)],capture_output=True))
        deadline=time.monotonic()+5
        while not marker.exists() and time.monotonic()<deadline:time.sleep(.02)
        self.assertTrue(marker.exists())
        self.store.reserve('orphan',self.work,self.claim['token'],'sandbox',10)
        self.store.retry(self.work,self.claim['token'],{'message':'lost host'},delay=0)
        ident=self.store.enqueue('cleanup','task-001','g',{'resource':'orphan'})
        execution=Execution(self.store,self.store.claim('cleanup','cleaner',ident=ident))
        item={'name':'orphan','phase':'Ready','labels':{'gyre.dev/pipeline':self.store.setting('owner')}}
        def remote(name,*args,**kwargs):
            if args[0]=='python3':
                code=args[-1].replace('/sandbox/workspace',str(workspace)).replace('/tmp/stage',str(workspace))
                return subprocess.run([sys.executable,'-c',code],check=True,capture_output=True,text=True)
            return subprocess.CompletedProcess([],0,'bounded log','')
        def capture(*args,**kwargs):
            self.assertEqual(json.loads((root/'result.json').read_text())['exit_code'],130)
            return subprocess.CompletedProcess([],0,'','')
        with patch.object(execution,'login'),patch('pipeline.stages.gateway.inventory',side_effect=[[item],[],[]]),patch.object(execution,'remote',side_effect=remote),patch.object(execution,'command',side_effect=capture) as capture_call,patch.object(execution,'os',return_value=subprocess.CompletedProcess([],0,'','')):
            cleanup(execution,self.store.task('task-001'))
        self.assertEqual(capture_call.call_count,1)

    def test_cleanup_captures_but_never_promotes_explicitly_fenced_source(self):
        self.store.reserve('fenced-pod', self.work, self.claim['token'], 'sandbox', 1)
        self.store.resource_state('fenced-pod', 'absent')
        self.store.db.execute("UPDATE work SET state='obsolete',token=token+1 WHERE id=?", (self.work,))
        prior = self.store.directory / 'attempts' / self.work / str(self.claim['token'])
        prior.mkdir(parents=True)
        (prior / 'recovery.json').write_text(json.dumps({'base': 'obsolete-source'}))
        self.store.enqueue('cleanup', 'task-001', 'g', {'resource': 'fenced-pod'}, priority=10000)
        claim = self.store.claim('cleanup', 'cleaner')
        execution = Execution(self.store, claim)
        with patch.object(execution, 'login'), patch('pipeline.stages.gateway.inventory', return_value=[]), patch('pipeline.stages.checkout', side_effect=AssertionError('must not promote fenced source')):
            _, updates, _ = cleanup(execution, self.store.task('task-001'))
        self.assertEqual(updates, {})

    def test_metadata_only_resubmission_does_not_resolve_a_real_review_finding(self):
        root = self.store.directory / 'source-proof'
        root.mkdir()
        def git(*args):
            return subprocess.check_output(['git', *args], cwd=root, text=True, stderr=subprocess.PIPE).strip()
        git('init', '-q', '-b', 'main')
        git('config', 'user.name', 'Test')
        git('config', 'user.email', 'test@example.com')
        git('config', 'commit.gpgsign', 'false')
        git('config', 'core.hooksPath', '/dev/null')
        body = '---\ntitle: Scope enforcement\nspec_ref: behavior.md\ndepends_on: []\nprogress: needs-revision\n---\n\n## Required behavior\nReject foreign tenants.\n'
        task_path = root / 'specs/tasks/task-001.md'
        task_path.parent.mkdir(parents=True)
        task_path.write_text(body)
        (root / 'production.rs').write_text('pub fn allowed() -> bool { true }\n')
        git('add', '.'); git('commit', '-qm', 'unrepaired candidate')
        base = git('rev-parse', 'HEAD')
        revised = body.replace('needs-revision', 'ready-for-review') + '\n## Shipped\n\n- Claimed to fix scope.\n'
        task_path.write_text(revised)
        git('add', '.'); git('commit', '-qm', 'notes only')
        head = git('rev-parse', 'HEAD')
        origin = self.store.directory / 'source-origin.git'
        subprocess.run(['git', 'clone', '--quiet', '--bare', str(root), str(origin)], check=True)
        git('remote', 'add', 'origin', str(origin))
        repair = {'category': 'review', 'source': base, 'id': 'finding', 'detail': 'Scope enforcement is absent.'}
        self.store.put_task('task-001', 'g', body, {'dependencies': [], 'candidate': base, 'candidate_base': base, 'repair': repair})
        execution = Execution(self.store, self.claim)
        execution.claim['input'] = {'base': base, 'candidate': base, 'repair': repair}
        receipt = {'task_body': revised, 'valid': True, 'head': head, 'base': base, 'branch': 'private', 'agent_exit': 0}
        with patch('pipeline.stages.prompt', return_value='assignment'), patch.object(execution, 'cloud_step', return_value=receipt), patch('pipeline.stages.checkout', return_value=root):
            from pipeline.execution import Retry
            with self.assertRaises(Retry) as raised:
                implement(execution, self.store.task('task-001'))
        self.assertTrue(raised.exception.fresh_model)
        self.assertEqual(self.store.task('task-001')['data']['repair'], repair)
        self.assertEqual(self.store.task('task-001')['data']['candidate'], base)


if __name__ == '__main__':
    unittest.main()
