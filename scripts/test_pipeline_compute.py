import json
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace
from pipeline.store import Store
from pipeline.compute import profile, record_oom, from_usage
from pipeline.runtime import observe, failure, restart, snapshot
from pipeline.execution import Execution, Attention

class ComputeTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        self.store=Store(self.temp.name);self.addCleanup(self.store.close)
        self.store.put_task('task-001','g','spec',{})
        self.work=self.store.enqueue('implement','task-001','g',{})
        self.execution=Execution(self.store,self.store.claim('implement','test',capacity=10))
        self.store.reserve('owned',self.work,self.execution.claim['token'],'sandbox',10,{'compute':profile(self.store,'task-001','implement')})

    def test_only_observed_oom_escalates_and_second_tier_failure_stops(self):
        self.assertEqual(profile(self.store,'task-001','implement')['memory'],'4Gi')
        first=record_oom(self.store,'task-001','implement',4,{'reason':'OOMKilled'})
        self.assertEqual(first.category,'sandbox_oom')
        self.assertEqual(profile(self.store,'task-001','implement')['memory'],'8Gi')
        self.assertEqual(profile(self.store,'task-001','review')['memory'],'4Gi')
        self.assertEqual(profile(self.store,'task-002','implement')['memory'],'4Gi')
        self.assertIsInstance(record_oom(self.store,'task-001','implement',8,{}),Attention)
        self.assertEqual(profile(self.store,'task-001','implement')['memory'],'8Gi')

    def test_child_only_oom_is_not_misclassified_as_bad_candidate_code(self):
        usage={'memory.events':{'oom_kill':'1'},'memory.max':str(4*1024**3),'memory.peak':str(4*1024**3)}
        error=from_usage(self.execution,'owned',usage)
        self.assertEqual(error.category,'sandbox_oom')
        self.assertEqual(error.retry_delay,0)
        self.assertEqual(profile(self.store,'task-001','implement')['memory'],'8Gi')
        self.assertIsNone(from_usage(self.execution,'owned',{'memory.events':{'oom_kill':'0'}}))

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
        self.assertEqual(profile(self.store,'task-001','implement')['memory'],'8Gi')

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
