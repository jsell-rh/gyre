"""Admission follows scoped workload evidence while cleanup remains durable."""
import json
import subprocess
import tempfile
import time
import unittest
from types import SimpleNamespace
from unittest.mock import patch

from pipeline.store import Store
from pipeline.runtime import observe
from pipeline.cockpit import snapshot


class CapacityTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        self.store=Store(self.temp.name);self.addCleanup(self.store.close)
        self.store.set_setting('slots',1)
        self.store.set_setting('runtime_kube_context','gyre-context')
        self.store.set_setting('runtime_namespace','gyre-namespace')
        self.store.put_task('old','g','spec',{})
        self.work=self.store.enqueue('implement','old','g',{})
        claim=self.store.claim('implement','first',capacity=1)
        self.store.reserve('old-pod',self.work,claim['token'],'sandbox',1)
        self.store.finish(self.work,claim['token'],{})
        self.store.resource_state('old-pod','deleting')
        self.store.put_task('next','g','spec',{})
        self.next=self.store.enqueue('review','next','g',{})
        self.sandbox={'metadata':{'name':'old-pod','uid':'sandbox-uid',
            'labels':{'openshell.ai/sandbox-id':'sandbox-id'}},'spec':{'operatingMode':'Suspended'}}

    def observe(self,pods=None,sandboxes=None):
        def kube(args,**kwargs):
            self.assertIn('gyre-context',args);self.assertIn('gyre-namespace',args)
            values=sandboxes if 'sandboxes.agents.x-k8s.io' in args else pods
            return SimpleNamespace(stdout=json.dumps({'items':values or []}))
        with patch('pipeline.runtime.subprocess.run',side_effect=kube) as calls:
            observe(self.store,force=True)
        self.assertIn('sandboxes.agents.x-k8s.io',calls.call_args_list[0].args[0])
        self.assertIn('pods',calls.call_args_list[1].args[0])

    def test_absent_workload_releases_slot_and_keeps_cleanup_and_provider(self):
        self.store.db.execute("INSERT INTO resources VALUES(?,?,?,?,?,?,?)",('provider',self.work,1,'provider','present','{}',time.time()))
        cleaner=self.store.enqueue('cleanup','old','g',{'resource':'old-pod'})
        self.assertIsNone(self.store.claim('review','blocked',ident=self.next,capacity=1))
        self.observe(sandboxes=[self.sandbox])
        view=snapshot(self.store)
        self.assertEqual(view['resources']['used'],0)
        self.assertEqual(view['resources']['deletion_pending'],1)
        row=self.store.db.execute("SELECT state FROM resources WHERE name='old-pod'").fetchone()
        self.assertEqual(row['state'],'deleting')
        self.assertEqual(self.store.db.execute("SELECT state FROM resources WHERE name='provider'").fetchone()[0],'present')
        self.assertIsNotNone(self.store.claim('review','next',ident=self.next,capacity=1))
        self.assertEqual(self.store.compute_used(),1)
        self.assertIsNotNone(self.store.claim('cleanup','cleaner',ident=cleaner))

    def test_absent_sandbox_also_releases_but_active_missing_pod_does_not(self):
        self.sandbox['spec']['operatingMode']='Active'
        self.observe(sandboxes=[self.sandbox])
        self.assertEqual(self.store.compute_used(),1)
        self.observe()
        self.assertEqual(self.store.compute_used(),0)

    def test_any_matching_pod_including_terminating_or_renamed_keeps_slot(self):
        for metadata in ({'name':'old-pod','deletionTimestamp':'now'},
                         {'name':'renamed','labels':{'openshell.ai/boundary-pair':'sandbox-id'}},
                         {'name':'renamed','ownerReferences':[{'uid':'sandbox-uid'}]},
                         {'name':'renamed','labels':{'openshell.ai/sandbox-name':'old-pod'}}):
            with self.subTest(metadata=metadata):
                self.observe(pods=[{'metadata':metadata}],sandboxes=[self.sandbox])
                self.assertEqual(self.store.compute_used(),1)
        self.observe(sandboxes=[self.sandbox])
        self.assertEqual(self.store.compute_used(),0)
        self.observe(pods=[{'metadata':{'name':'old-pod'}}],sandboxes=[self.sandbox])
        self.assertEqual(self.store.compute_used(),1)

    def test_unknown_inventory_never_releases_and_old_evidence_expires(self):
        for error in (subprocess.TimeoutExpired('kubectl',10),subprocess.CalledProcessError(1,'kubectl')):
            with patch('pipeline.runtime.subprocess.run',side_effect=error):observe(self.store,force=True)
            self.assertEqual(self.store.compute_used(),1)
        for value in ('not json','{}','{"items":null}','{"items":[],"metadata":{"continue":"more"}}'):
            with patch('pipeline.runtime.subprocess.run',return_value=SimpleNamespace(stdout=value)):
                observe(self.store,force=True)
            self.assertEqual(self.store.compute_used(),1)
        self.observe(sandboxes=[self.sandbox])
        self.assertEqual(self.store.compute_used(),0)
        with patch('pipeline.store.time.time',return_value=time.time()+181):
            self.assertEqual(self.store.compute_used(),1)
        with patch('pipeline.store.time.time',return_value=time.time()-10):
            self.assertEqual(self.store.compute_used(),1)
        self.store.set_setting('runtime_namespace','other-namespace')
        self.assertEqual(self.store.compute_used(),1)

    def test_receipt_cannot_release_new_claim_present_resource_or_creation_intent(self):
        self.observe(sandboxes=[self.sandbox])
        self.assertEqual(self.store.compute_used(),0)
        for state in ('present','intent'):
            self.store.resource_state('old-pod',state)
            self.assertEqual(self.store.compute_used(),1)
        self.store.resource_state('old-pod','deleting')
        self.store.db.execute("UPDATE resources SET token=token+1 WHERE name='old-pod'")
        self.assertEqual(self.store.compute_used(),1)

    def test_observation_cannot_release_a_resource_rebound_during_query(self):
        def kube(args,**kwargs):
            if 'pods' in args:
                self.store.db.execute("UPDATE resources SET token=token+1 WHERE name='old-pod'")
            return SimpleNamespace(stdout=json.dumps({'items':[]}))
        with patch('pipeline.runtime.subprocess.run',side_effect=kube):observe(self.store,force=True)
        self.assertEqual(self.store.compute_used(),1)
        data=json.loads(self.store.db.execute("SELECT data FROM resources WHERE name='old-pod'").fetchone()[0])
        self.assertNotIn('compute_observation',data)

    def test_failed_sandbox_inventory_does_not_hide_other_workers_oom_evidence(self):
        self.store.db.execute("INSERT INTO resources VALUES(?,?,?,?,?,?,?)",('live',self.work,1,'sandbox','present','{}',time.time()))
        pod={'metadata':{'name':'live','uid':'pod-uid','labels':{'openshell.ai/boundary-role':'workload'}},
             'spec':{'containers':[{'resources':{'limits':{'memory':'4Gi'}}}]},
             'status':{'containerStatuses':[{'state':{'terminated':{'reason':'OOMKilled','exitCode':137}}}]}}
        def kube(args,**kwargs):
            if 'sandboxes.agents.x-k8s.io' in args:raise subprocess.CalledProcessError(1,'kubectl')
            return SimpleNamespace(stdout=json.dumps({'items':[pod]}))
        with patch('pipeline.runtime.subprocess.run',side_effect=kube):observe(self.store,force=True)
        self.assertEqual(self.store.compute_used(),2)
        data=json.loads(self.store.db.execute("SELECT data FROM resources WHERE name='live'").fetchone()[0])
        self.assertEqual(data['runtime']['reason'],'OOMKilled')

    def test_verification_reserve_uses_same_occupancy_accounting(self):
        self.store.set_setting('slots',2)
        self.assertFalse(self.store.admission_available('implement'))
        self.observe(sandboxes=[self.sandbox])
        self.assertTrue(self.store.admission_available('implement'))


if __name__=='__main__':unittest.main()
