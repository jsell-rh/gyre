"""Namespace quotas must bound racing admissions before any sandbox create."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from pipeline.store import Store
from pipeline.capacity import build_observation,decision,pod_cost,quantity
from pipeline.compute import assigned_profile

GIB=1024**3


def resources(memory,cpu='1'):
    return {key:{'cpu':cpu,'memory':memory} for key in ('requests','limits')}


def pod(name,memory,owner=None,role=None):
    meta={'name':name,'labels':{},'ownerReferences':[]}
    if owner:meta['ownerReferences']=[{'kind':'Sandbox','name':owner}]
    if role:meta['labels']['openshell.ai/boundary-role']=role
    return {'metadata':meta,'spec':{'containers':[{'name':'agent','resources':resources(memory)}]},'status':{'phase':'Running'}}


class NamespaceCapacityTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        self.s=Store(self.tmp.name);self.addCleanup(self.s.close)
        self.s.set_setting('runtime_kube_context','only-this-context');self.s.set_setting('runtime_namespace','only-this-namespace')
        self.s.set_setting('slots',100)
        self.s.set_setting('verification_slots',0)
        self.foreign=[pod('external','3Gi')]
        self.quotas=[{'spec':{'hard':{'requests.memory':'14Gi','limits.memory':'30Gi','requests.cpu':'20','pods':'50'}},
                      'status':{'used':{'requests.memory':'3Gi'}}}]
        self.limits=[{'spec':{'limits':[{'type':'Container','max':{'memory':'6Gi','cpu':'2'},
            'default':{'memory':'1Gi','cpu':'500m'},'defaultRequest':{'memory':'512Mi','cpu':'250m'}}]}}]
        self.refresh()
        for i in range(8):
            name=f'task-{i:03d}';self.s.put_task(name,'g','body',{'triaged':'g','dependencies':[]})
            self.s.enqueue('implement',name,'g',{'candidate':None,'repair':None,'base':'main'})

    def refresh(self,pods=None):
        self.s.set_setting('namespace_capacity',build_observation(self.s,pods or self.foreign,self.quotas,self.limits))

    def claim(self):return self.s.claim('implement','actor',concurrency=100,capacity=100)

    def test_real_process_race_reserves_quota_before_backend_observes_pods(self):
        code='''
import sys,json
from pipeline.store import Store
s=Store(sys.argv[1]);c=s.claim('implement','racer',concurrency=100,capacity=100)
print(json.dumps(c['id'] if c else None))
'''
        ps=[subprocess.Popen([sys.executable,'-c',code,str(self.s.directory)],cwd=Path(__file__).parent,
                             stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True) for _ in range(8)]
        got=[]
        for p in ps:
            out,err=p.communicate(timeout=20);self.assertEqual(p.returncode,0,err);got.append(json.loads(out))
        self.assertEqual(sum(v is not None for v in got),2)
        self.assertEqual(self.s.setting('slots'),100)
        d=decision(self.s);self.assertFalse(d['allowed']);self.assertEqual(d['resource'],'requests.memory')
        self.assertEqual(d['used']['requests.memory'],12*GIB)

    def test_admission_transfer_and_observed_workload_are_not_charged_twice(self):
        a=self.claim();b=self.claim();self.assertIsNotNone(a);self.assertIsNotNone(b)
        self.assertTrue(self.s.reserve('owned',a['id'],a['token'],'sandbox',100))
        before=decision(self.s)['used']['requests.memory']
        worker=pod('owned','4Gi','owned','workload')
        supervisor=pod('boundary','512Mi','owned','supervisor')
        supervisor['spec']['containers'][0]['resources']={'requests':{'cpu':'250m','memory':'512Mi'},'limits':{'cpu':'500m','memory':'1Gi'}}
        self.refresh(self.foreign+[worker,supervisor])
        self.assertEqual(decision(self.s)['used']['requests.memory'],before)
        self.assertIsNone(self.claim())
        self.s.resource_state('owned','absent');self.refresh()
        self.assertIsNotNone(self.claim())

    def test_stale_or_wrong_scope_observation_preserves_existing_worker_and_control_work(self):
        a=self.claim();snap=self.s.setting('namespace_capacity');snap['at']=time.time()-31
        self.s.set_setting('namespace_capacity',snap)
        self.assertIsNone(self.claim());self.assertEqual(decision(self.s)['condition'],'CapacityUnknown')
        self.s.heartbeat(a['id'],a['token'])
        self.s.enqueue('cleanup','task-007','g',{'resource':'example'})
        self.assertIsNotNone(self.s.claim('cleanup','cleanup'))
        snap['at']=time.time();snap['namespace']='another-namespace';self.s.set_setting('namespace_capacity',snap)
        self.assertIsNone(self.claim())

    def test_quota_increase_resumes_without_changing_desired_slots_and_snapshot_error_blocks(self):
        self.assertIsNotNone(self.claim());self.assertIsNotNone(self.claim());self.assertIsNone(self.claim())
        self.quotas[0]['spec']['hard']['requests.memory']='24Gi';self.refresh()
        self.assertIsNotNone(self.claim());self.assertEqual(self.s.setting('slots'),100)
        self.s.set_setting('namespace_capacity_error',{'at':time.time(),'kind':'TimeoutExpired'})
        self.assertIsNone(self.claim())

    def test_direct_reservation_cannot_bypass_quota_or_container_limit(self):
        a=self.s.claim('implement','manual')
        self.s.set_setting('namespace_capacity',self.s.setting('namespace_capacity') | {'hard':{'requests.memory':4*GIB}})
        self.assertFalse(self.s.reserve('direct',a['id'],a['token'],'sandbox',100))
        self.assertEqual(self.s.db.execute("SELECT count(*) FROM resources WHERE name='direct'").fetchone()[0],0)
        self.limits[0]['spec']['limits'][0]['max']['memory']='4Gi';self.refresh()
        self.assertEqual(decision(self.s,budget={'cpu':'1','memory':'6Gi'})['condition'],'ContainerLimit')

    def test_reserved_compute_size_is_frozen_for_creation(self):
        from pipeline.compute import from_usage,profile
        from pipeline.execution import Execution
        original=self.claim()
        self.assertTrue(self.s.reserve('old',original['id'],original['token'],'sandbox',100))
        error=from_usage(Execution(self.s,original),'old',{'memory.events':{'oom_kill':1},'memory.max':str(4*GIB)})
        self.s.retry(original['id'],original['token'],{'category':error.category},delay=0)
        self.s.resource_state('old','absent')
        claim=self.s.claim('implement','retry',ident=original['id'],concurrency=100,capacity=100)
        assigned=assigned_profile(self.s,claim);self.assertEqual(assigned['memory'],'6Gi')
        self.s.set_setting('worker_memory_max_gib',4)
        self.assertEqual(profile(self.s,claim['task'],claim['stage'])['memory'],'4Gi')
        self.assertTrue(self.s.reserve('owned',claim['id'],claim['token'],'sandbox',100))
        self.assertEqual(assigned_profile(self.s,claim),assigned)
        self.assertEqual(json.loads(self.s.db.execute("SELECT data FROM resources WHERE name='owned'").fetchone()[0])['compute'],assigned)
        self.assertTrue(self.s.reserve('owned',claim['id'],claim['token'],'sandbox',100,{'compute':{'cpu':'1','memory':'1Gi'}}))
        self.assertEqual(json.loads(self.s.db.execute("SELECT data FROM resources WHERE name='owned'").fetchone()[0])['compute'],assigned)

    def test_quantity_and_restartable_init_accounting(self):
        self.assertEqual(quantity('1.5Gi'),int(1.5*GIB));self.assertEqual(quantity('250m',True),250)
        self.assertEqual(quantity('1000000n',True),1)
        p={'containers':[{'resources':resources('4Gi')}],
           'initContainers':[{'resources':resources('256Mi'),'restartPolicy':'Always'},
                             {'resources':resources('8Gi')},{'resources':resources('10Gi')}],
           'overhead':{'memory':'64Mi'}}
        self.assertEqual(pod_cost(p)['requests.memory'],10*GIB+320*1024**2)

    def test_model_admission_leaves_real_headroom_for_a_six_gib_verifier(self):
        self.s.set_setting('verification_slots',1)
        self.assertIsNotNone(self.claim())
        self.assertIsNone(self.claim(),'logical reserve at slot 99 must also reserve physical quota')
        self.assertTrue(decision(self.s,work={'task':'task-007','stage':'verify'},budget={'cpu':'1','memory':'6Gi'})['allowed'])
