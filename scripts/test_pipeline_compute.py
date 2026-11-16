import json
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace
from pipeline.store import Store
from pipeline.compute import profile, record_oom, from_usage
from pipeline.runtime import observe, failure, restart, snapshot, ensure_workspace
from pipeline.execution import Execution, Attention, Retry

class ComputeTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        self.store=Store(self.temp.name);self.addCleanup(self.store.close)
        self.store.put_task('task-001','g','spec',{})
        self.work=self.store.enqueue('implement','task-001','g',{})
        self.execution=Execution(self.store,self.store.claim('implement','test',capacity=10))
        self.store.reserve('owned',self.work,self.execution.claim['token'],'sandbox',10,{'compute':profile(self.store,'task-001','implement')})

    def test_historical_escalation_and_incomplete_oom_proof_never_raise_memory(self):
        self.store.set_setting('compute_memory:task-001:implement',8)
        self.assertEqual(profile(self.store,'task-001','implement')['memory'],'4Gi')
        first=record_oom(self.store,'task-001','implement',4,{'reason':'OOMKilled'})
        self.assertEqual(first.category,'sandbox_oom')
        self.assertIsInstance(first,Attention)
        self.assertEqual(profile(self.store,'task-001','implement')['memory'],'4Gi')
        self.assertEqual(profile(self.store,'task-001','review')['memory'],'4Gi')
        self.assertEqual(profile(self.store,'task-002','implement')['memory'],'4Gi')

    def test_child_only_oom_is_not_misclassified_as_bad_candidate_code(self):
        usage={'memory.events':{'oom_kill':'1'},'memory.max':str(4*1024**3),'memory.peak':str(4*1024**3)}
        error=from_usage(self.execution,'owned',usage)
        self.assertEqual(error.category,'sandbox_oom')
        self.assertIsInstance(error,Retry)
        self.assertNotIsInstance(error,Attention)
        self.assertEqual(profile(self.store,'task-001','implement')['memory'],'6Gi')
        self.assertIsNone(from_usage(self.execution,'owned',{'memory.events':{'oom_kill':'0'}}))

    def test_oom_retry_is_scoped_and_six_gib_is_a_hard_ceiling(self):
        usage={'memory.events':{'oom_kill':1},'memory.max':str(4*1024**3)}
        self.assertNotIsInstance(from_usage(self.execution,'owned',usage),Attention)
        self.assertEqual(profile(self.store,'task-001','implement')['memory'],'6Gi')
        self.assertEqual(profile(self.store,'task-001','review')['memory'],'4Gi')
        self.assertEqual(profile(self.store,'task-002','implement')['memory'],'4Gi')
        # A configuration change affects the next allocation, not the live one.
        current=json.loads(self.store.db.execute("SELECT data FROM resources WHERE name='owned'").fetchone()[0])
        self.assertEqual(current['compute']['memory'],'4Gi')
        self.assertIsInstance(from_usage(self.execution,'owned',usage | {'memory.max':str(6*1024**3)}),Attention)
        self.assertEqual(profile(self.store,'task-001','implement')['memory'],'6Gi')
        self.store.put_task('task-001','new-requirements','changed spec',{})
        self.assertEqual(profile(self.store,'task-001','implement')['memory'],'4Gi')

    def test_four_gib_ceiling_and_invalid_settings_never_expand_allocations(self):
        usage={'memory.events':{'oom_kill':1},'memory.max':str(4*1024**3)}
        for ceiling in (4,8,'6',True):
            self.store.set_setting('worker_memory_max_gib',ceiling)
            with self.subTest(ceiling=ceiling):
                self.assertIsInstance(from_usage(self.execution,'owned',usage),Attention)
                self.assertEqual(profile(self.store,'task-001','implement')['memory'],'4Gi')

    def test_oom_evidence_must_belong_to_the_current_task_stage(self):
        proof={'source':'cgroup','sandbox':'not-owned','memory.events':{'oom_kill':1},'memory.max':str(4*1024**3)}
        self.assertIsInstance(record_oom(self.store,'task-001','implement',4,proof),Attention)
        proof['sandbox']='owned'
        self.assertIsInstance(record_oom(self.store,'task-001','review',4,proof),Attention)
        self.assertEqual(profile(self.store,'task-001','implement')['memory'],'4Gi')
        self.assertEqual(profile(self.store,'task-001','review')['memory'],'4Gi')

    def test_retry_waits_for_old_allocation_cleanup_before_using_six_gib(self):
        error=from_usage(self.execution,'owned',{'memory.events':{'oom_kill':1},'memory.max':str(4*1024**3)})
        self.store.retry(self.work,self.execution.claim['token'],{'category':error.category},delay=0)
        self.assertIsNone(self.store.claim('implement','retry',capacity=10))
        self.store.resource_state('owned','absent')
        claim=self.store.claim('implement','retry',capacity=10)
        self.assertIsNotNone(claim)
        self.assertEqual(claim['token'],self.execution.claim['token']+1)
        self.assertEqual(profile(self.store,'task-001','implement')['memory'],'6Gi')

    def test_gateway_not_honoring_six_gib_cannot_create_an_automatic_retry_loop(self):
        usage={'memory.events':{'oom_kill':1},'memory.max':str(4*1024**3)}
        self.assertNotIsInstance(from_usage(self.execution,'owned',usage),Attention)
        self.store.resource_state('owned','present',{'compute':profile(self.store,'task-001','implement')})
        self.assertIsInstance(from_usage(self.execution,'owned',usage),Attention)

    def test_full_workspace_or_inodes_holds_work_without_raising_memory(self):
        for disk in ({'total':1024**3,'free':0,'inode_free':100},
                     {'total':1024**3,'free':1024**3,'inode_free':0}):
            with self.subTest(disk=disk):
                error=from_usage(self.execution,'owned',{'workspace':disk})
                self.assertIsInstance(error,Attention)
                self.assertEqual(error.category,'sandbox_storage')
                self.assertEqual(error.diagnostic['source'],'statvfs')
                self.assertEqual(profile(self.store,'task-001','implement')['memory'],'4Gi')
        self.store.retry(self.work,self.execution.claim['token'],{'category':error.category},attention=True)
        newer=self.store.enqueue('implement','task-001','g',{'candidate':'new-checkpoint','base':'new-main'})
        self.assertIsNone(self.store.claim('implement','actor',ident=newer))

    def test_storage_reclamation_removes_only_owned_generated_output(self):
        import subprocess,sys
        from pathlib import Path
        from pipeline.runtime import reclaim_build_cache
        workspace=Path(self.temp.name)/'workspace'
        cache=workspace/'build/target';cache.mkdir(parents=True)
        (cache/'artifact').write_bytes(b'generated')
        source=workspace/'checkout/source.rs';source.parent.mkdir();source.write_text('valuable source')
        def invoke(args,**kwargs):
            code=args[-1].replace('/tmp/gyre-build',str(workspace/'build'))
            return subprocess.run([sys.executable,'-c',code],capture_output=True,text=True)
        reclaim_build_cache(self.execution,'owned',invoke)
        self.assertFalse(cache.exists())
        self.assertEqual(source.read_text(),'valuable source')
        cache.symlink_to(source.parent,target_is_directory=True)
        with self.assertRaises(RuntimeError):reclaim_build_cache(self.execution,'owned',invoke)
        self.assertEqual(source.read_text(),'valuable source')
        self.store.db.execute("UPDATE resources SET token=token+1 WHERE name='owned'")
        with self.assertRaises(Attention):reclaim_build_cache(self.execution,'owned',invoke)

    def test_build_disk_is_checked_through_openshell_without_cluster_mutations(self):
        self.store.set_setting('runtime_namespace','gyre')
        calls=[]
        def invoke(args,**kwargs):
            calls.append(args)
            if args[:2]==['sandbox','get']:
                value={'labels':{'gyre.dev/pipeline':self.store.setting('owner'),'gyre.dev/work':self.work}}
            else:value={'total':100*1024**3,'free':80*1024**3,'filesystem':'overlay'}
            return SimpleNamespace(returncode=0,stdout=json.dumps(value))
        with patch('pipeline.runtime.subprocess.run',side_effect=AssertionError('no direct cluster access')):
            ensure_workspace(self.execution,'owned',invoke)
        self.assertEqual([a[:2] for a in calls],[['sandbox','get'],['sandbox','exec']])
        for fs,free in (('tmpfs',80),('overlay',1)):
            def unavailable(args,**kwargs):
                if args[:2]==['sandbox','get']:return invoke(args,**kwargs)
                return SimpleNamespace(returncode=0,stdout=json.dumps({'total':100*1024**3,'free':free*1024**3,'filesystem':fs}))
            with self.assertRaises(Attention):ensure_workspace(self.execution,'owned',unavailable)

    def test_foreign_gateway_workspace_is_never_modified(self):
        self.store.set_setting('runtime_namespace','gyre')
        calls=[]
        def foreign(args,**kwargs):
            calls.append(args)
            return SimpleNamespace(returncode=0,stdout='{"labels":{}}')
        with self.assertRaises(ValueError):ensure_workspace(self.execution,'owned',foreign)
        self.assertEqual(len(calls),1)

    def test_durable_runtime_observer_preserves_allocation_and_captures_oom_once(self):
        self.store.set_setting('runtime_kube_context','dedicated-context');self.store.set_setting('runtime_namespace','gyre')
        pod={'metadata':{'name':'owned','uid':'unique-pod','labels':{'openshell.ai/boundary-role':'workload'}},
             'spec':{'containers':[{'resources':{'limits':{'memory':'4Gi'}}}]},
             'status':{'containerStatuses':[{'state':{'terminated':{'reason':'OOMKilled','exitCode':137}}}]}}
        with patch('pipeline.runtime.subprocess.run',return_value=SimpleNamespace(stdout=json.dumps({'items':[pod]}))):
            observe(self.store,force=True);observe(self.store,force=True)
        d=json.loads(self.store.db.execute("SELECT data FROM resources WHERE name='owned'").fetchone()[0])
        self.assertEqual(d['compute']['build_jobs'],1)
        self.assertEqual(self.store.db.execute("SELECT count(*) FROM events WHERE kind='sandbox_terminated'").fetchone()[0],1)
        self.assertEqual(failure(self.execution,'owned').category,'sandbox_oom')
        self.assertEqual(profile(self.store,'task-001','implement')['memory'],'6Gi')

    def test_lost_health_query_is_unknown_not_evidence_for_reallocation(self):
        import subprocess
        def unavailable(*args,**kwargs):raise subprocess.TimeoutExpired('query',20)
        self.assertIsNone(snapshot(self.execution,'owned',unavailable))
        self.assertEqual(profile(self.store,'task-001','implement')['memory'],'4Gi')
        self.assertEqual(self.store.db.execute("SELECT count(*) FROM events WHERE kind='compute_memory_escalated'").fetchone()[0],0)

    def test_owned_stopped_sandbox_restarts_once_not_as_a_new_allocation(self):
        calls=[];phase='Stopped'
        def invoke(args,**kw):
            nonlocal phase
            calls.append(args)
            if args[:2]==['sandbox','start']:phase='Ready';return SimpleNamespace(returncode=0,stdout='')
            return SimpleNamespace(returncode=0,stdout=json.dumps({'phase':phase,'labels':{'gyre.dev/pipeline':self.store.setting('owner'),'gyre.dev/work':self.work}}))
        self.assertTrue(restart(self.execution,'owned',invoke))
        self.assertEqual(self.store.db.execute("SELECT count(*) FROM resources WHERE kind='sandbox'").fetchone()[0],1)
        phase='Stopped'
        with self.assertRaises(Attention):restart(self.execution,'owned',invoke)
        self.assertEqual(sum(a[:2]==['sandbox','start'] for a in calls),1)
        self.store.resource_state('owned','deleting')
        with self.assertRaises(Attention):restart(self.execution,'owned',invoke)
