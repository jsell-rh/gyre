"""Admission and namespace contracts for the extracted dispatcher integration."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

from pipeline.execution import Execution, Cancelled
from pipeline.store import Store

spec = importlib.util.spec_from_file_location('gyre_dispatch_adapter', Path(__file__).with_name('dispatch-adapter.py'))
adapter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(adapter)


class DispatcherContracts(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.store = Store(Path(self.temp.name)/'gyre')
        self.addCleanup(self.store.close)
        self.store.set_setting('slots', 1)
        self.store.put_task('task-001','g','requirements',{'triaged':'g','dependencies':[],'title':'One'})
        self.work = self.store.enqueue('implement','task-001','g',{'base':'main','candidate':None,'repair':None})

    def another(self):
        self.store.put_task('task-002','g','requirements',{'triaged':'g','dependencies':[],'title':'Two'})
        return self.store.enqueue('review','task-002','g',{'candidate':'code','base':'main'})

    def test_capacity_wait_does_not_launch_or_increment_failure(self):
        first=self.store.claim('implement','first',capacity=1)
        self.store.reserve('pod',self.work,first['token'],'sandbox',1)
        other=self.another()
        self.store.db.execute('UPDATE tasks SET data=? WHERE name=?',
            (json.dumps({'triaged':'g','dependencies':[],'title':'Two','candidate':'code','candidate_base':'main'}),'task-002'))
        request=adapter.job(self.store,dict(self.store.db.execute('SELECT * FROM work WHERE id=?',(other,)).fetchone()))
        with patch.object(adapter.subprocess,'Popen') as launch:
            self.assertFalse(adapter.launch(self.store,request))
        launch.assert_not_called()
        row=self.store.db.execute('SELECT state,token,failures FROM work WHERE id=?',(other,)).fetchone()
        self.assertEqual(tuple(row),('ready',0,0))

    def test_atomic_global_admission_spans_implementation_and_review(self):
        # Independent OS processes race over one shared slot, across stages.
        other=self.another()
        command=[sys.executable,'-c', '''
import json,sys
from pipeline.store import Store
s=Store(sys.argv[1]);c=s.claim(sys.argv[2],'racer',concurrency=10,capacity=1)
print(json.dumps(c and c['id']))
''',str(self.store.directory)]
        processes=[subprocess.Popen(command+[stage],cwd=Path(__file__).parent,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
                   for stage in ('implement','review') for _ in range(5)]
        results=[]
        for p in processes:
            out,err=p.communicate(timeout=15);self.assertEqual(p.returncode,0,err);results.append(json.loads(out))
        self.assertEqual(len([r for r in results if r]),1)
        self.assertEqual(self.store.compute_used(),1)
        self.assertEqual(self.store.db.execute("SELECT count(*) FROM work WHERE state='claimed'").fetchone()[0],1)

    def test_admission_transfers_and_deletion_keeps_capacity_charged(self):
        claim=self.store.claim('implement','first',capacity=1)
        self.assertEqual(self.store.compute_used(),1)
        self.assertTrue(self.store.reserve('pod',self.work,claim['token'],'sandbox',1))
        self.assertEqual(self.store.compute_used(),1)
        self.store.finish(self.work,claim['token'],{})
        self.store.resource_state('pod','deleting')
        self.another()
        self.assertIsNone(self.store.claim('review','second',capacity=1))
        self.store.resource_state('pod','absent')
        self.assertIsNotNone(self.store.claim('review','second',capacity=1))

    def test_drain_does_not_revoke_an_existing_admission(self):
        claim=self.store.claim('implement','first',capacity=1)
        self.store.set_setting('slots',0)
        self.assertTrue(self.store.reserve('pod',self.work,claim['token'],'sandbox',0))
        self.another()
        self.assertIsNone(self.store.claim('review','second',capacity=0))

    def test_cleanup_cannot_race_source_recovery_with_a_live_stage_on_same_task(self):
        claim=self.store.claim('implement','first',capacity=1)
        self.store.reserve('pod',self.work,claim['token'],'sandbox',1)
        self.store.resource_state('pod','deleting')
        cleaner=self.store.enqueue('cleanup','task-001','g',{'resource':'pod'})
        self.assertIsNone(self.store.claim('cleanup','cleaner',ident=cleaner))
        self.store.finish(self.work,claim['token'],{})
        self.assertIsNotNone(self.store.claim('cleanup','cleaner',ident=cleaner))

    def test_new_round_waits_for_prior_task_cleanup_even_if_other_slots_are_free(self):
        claim=self.store.claim('implement','first',capacity=2)
        self.store.reserve('pod',self.work,claim['token'],'sandbox',2)
        self.store.finish(self.work,claim['token'],{})
        self.store.resource_state('pod','deleting')
        repair=self.store.enqueue('implement','task-001','g',{'base':'main','repair':'new finding'})
        self.assertIsNone(self.store.claim('implement','new round',ident=repair,capacity=2))
        self.store.resource_state('pod','absent')
        self.assertIsNotNone(self.store.claim('implement','new round',ident=repair,capacity=2))

    def test_foreign_dispatcher_cannot_launch_retry_cancel_or_read_history(self):
        foreign=Store(Path(self.temp.name)/'hypershell');self.addCleanup(foreign.close)
        request=adapter.job(self.store,dict(self.store.db.execute('SELECT * FROM work WHERE id=?',(self.work,)).fetchone()))
        self.assertNotEqual(adapter.scope(self.store),adapter.scope(foreign))
        request['scope']=adapter.scope(foreign)
        with patch.object(adapter.subprocess,'Popen') as launch:
            self.assertFalse(adapter.launch(self.store,request))
        launch.assert_not_called()
        with self.assertRaisesRegex(ValueError,'different dispatcher'):
            adapter.control(self.store,request)

    def test_existing_workers_are_adopted_without_relaunch(self):
        claim=self.store.claim('implement','existing-worker')
        adapter.recover(self.store)
        row=next(r for r in adapter.read_rows(self.store) if r['key']==self.work)
        self.assertEqual(row['status'],'running')
        self.assertEqual(row['run_id'],f"{self.work}:{claim['token']}")
        self.assertFalse(adapter.authorize(self.store,row))

    def test_gateway_alias_does_not_change_hypershell_registration_or_share_refresh_tokens(self):
        from pipeline.isolation import isolate_gateway
        config=Path(self.temp.name)/'config'
        old=config/'openshell/gateways/shared'
        old.mkdir(parents=True)
        original=json.dumps({'name':'shared','auth_mode':'oidc','gateway_endpoint':'https://gateway.invalid',
                             'oidc_issuer':'https://issuer.invalid','oidc_client_id':'client'})
        (old/'metadata.json').write_text(original)
        (old/'oidc_token.json').write_text('PRIVATE_REFRESH_TOKEN')
        active=config/'openshell/active_gateway';active.write_text('shared')
        self.store.set_setting('gateway','shared')
        with patch.dict(os.environ,{'XDG_CONFIG_HOME':str(config)}):
            alias=isolate_gateway(self.store)
            self.assertEqual(isolate_gateway(self.store),alias)
        self.assertNotEqual(alias,'shared')
        self.assertEqual((old/'metadata.json').read_text(),original)
        self.assertEqual((old/'oidc_token.json').read_text(),'PRIVATE_REFRESH_TOKEN')
        self.assertEqual(active.read_text(),'shared')
        self.assertFalse((config/'openshell/gateways'/alias/'oidc_token.json').exists())
        self.assertEqual((config/'openshell/gateways'/alias/'metadata.json').stat().st_mode&0o777,0o600)

    def test_forge_provider_and_profile_are_instance_scoped_and_contain_no_key(self):
        from pipeline.isolation import provider_files
        other=Store(Path(self.temp.name)/'hypershell');self.addCleanup(other.close)
        with patch.dict(os.environ,{'GH_TOKEN':'PRIVATE_API_KEY'}):
            name,profile=provider_files(self.store,Path(self.temp.name)/'first')
            other_name,_=provider_files(other,Path(self.temp.name)/'second')
        self.assertNotEqual(name,other_name)
        self.assertNotEqual(name,'github')
        self.assertTrue(profile.read_text().startswith('id: '+name+'\n'))
        definition=json.loads((Path(self.temp.name)/'first/providers'/(name+'.yaml')).read_text())
        self.assertEqual(definition['type'],name)
        self.assertEqual(definition['credentials']['GH_TOKEN'],'${GH_TOKEN}')
        self.assertNotIn('PRIVATE_API_KEY',profile.read_text()+json.dumps(definition))

    def test_cancel_binds_to_attempt_and_pauses_task_without_killing_foreign_process(self):
        claim=self.store.claim('implement','gyre-dispatcher:test',capacity=1)
        request=adapter.job(self.store,claim)|{'run_id':f"{self.work}:{claim['token']}"}
        adapter.control(self.store,request,cancel=True)
        self.assertTrue(self.store.setting('task_paused:task-001'))
        with self.assertRaises(Cancelled):Execution(self.store,claim).current()
        self.store.retry(self.work,claim['token'],{'category':'observation','message':'canceled'})
        self.assertFalse(adapter.authorize(self.store,request))
        adapter.control(self.store,request)
        self.assertFalse(self.store.setting('task_paused:task-001',False))

    def test_expired_admission_is_released_by_cleanup_without_remote_delete(self):
        from pipeline.stages import cleanup
        claim=self.store.claim('implement','gone',capacity=1)
        self.store.db.execute('UPDATE work SET expires=0 WHERE id=?',(self.work,))
        adapter.recover(self.store)
        name=self.store.admission_name(self.work,claim['token'])
        work=self.store.enqueue('cleanup','task-001','g',{'resource':name})
        cleaner=self.store.claim('cleanup','cleaner',ident=work)
        execution=Execution(self.store,cleaner)
        with patch.object(execution,'os',side_effect=AssertionError('no external create happened')):
            cleanup(execution,self.store.task('task-001'))
        self.assertEqual(self.store.compute_used(),0)


if __name__=='__main__':unittest.main()
