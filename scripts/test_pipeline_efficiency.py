import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

from pipeline.catalog import helper
from pipeline.store import Store
from pipeline.execution import Execution,Attention,Wait
from pipeline.verification import prepare_bundle
from pipeline.repair_budget import check,source_findings
from pipeline.ci import retry_infrastructure
from pipeline.build_cache import select

remote=helper('pipeline-cache')
ci=helper('dev-ci')

class EfficiencyTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        self.root=Path(self.tmp.name);self.repo=self.root/'repo';self.repo.mkdir()
        self.git('init','-q');self.git('config','user.name','Test');self.git('config','user.email','test@example.com')
        (self.repo/'code').write_text('real source\n');self.commit('base');self.base=self.git('rev-parse','HEAD')
        (self.repo/'code').write_text('changed source\n');self.commit('candidate');self.head=self.git('rev-parse','HEAD')
        self.store=Store(self.root/'state');self.addCleanup(self.store.close)
        self.store.put_task('task-001','g','body',{})
        self.work=self.store.enqueue('implement','task-001','g',{})
        self.execution=Execution(self.store,self.store.claim('implement','test'))

    def git(self,*args):return subprocess.check_output(['git',*args],cwd=self.repo,text=True,stderr=subprocess.PIPE).strip()
    def commit(self,message):self.git('add','.');self.git('-c','core.hooksPath=/dev/null','commit','-qm',message)

    def test_old_head_reachable_from_base_requires_no_bundle_and_checks_exact_checkout(self):
        self.git('checkout','-q','--detach',self.base)
        target=self.root/'candidate.bundle';target.write_text('stale')
        identity={'head':self.base,'base':self.head}
        self.assertFalse(prepare_bundle(self.execution,self.repo,identity,target))
        self.assertFalse(target.exists())
        identity={'head':self.head,'base':self.base}
        with self.assertRaisesRegex(ValueError,'assigned head'):prepare_bundle(self.execution,self.repo,identity,target)
        self.git('checkout','-q','--detach',self.head)
        self.assertTrue(prepare_bundle(self.execution,self.repo,identity,target))
        self.assertIn(self.head,subprocess.check_output(['git','bundle','list-heads',str(target)],text=True))

    def test_only_explicit_generated_findings_are_deferred(self):
        stale={'file':'web/dist/','detail':'bundle belongs to main','category':'code'}
        defect={'file':'web/src/Inbox.svelte','detail':'missing real action','category':'code'}
        escape={'file':'web/dist/../../src/App.svelte','detail':'source defect'}
        self.assertEqual(source_findings([stale,defect,escape]),([defect,escape],[stale]))
        self.assertEqual(source_findings(['build failure']),(['build failure'],[]))

    def test_identical_source_and_findings_stop_but_real_progress_resets(self):
        findings=[{'file':'code','detail':'fix real defect'}]
        check(self.execution,self.store.task('task-001'),self.repo,self.head,findings)
        (self.repo/'specs/tasks').mkdir(parents=True);(self.repo/'specs/tasks/task-001.md').write_text('new notes');self.commit('notes')
        check(self.execution,self.store.task('task-001'),self.repo,self.git('rev-parse','HEAD'),findings)
        with self.assertRaises(Attention) as caught:check(self.execution,self.store.task('task-001'),self.repo,self.git('rev-parse','HEAD'),findings)
        self.assertEqual(caught.exception.category,'repair_stalled')
        (self.repo/'code').write_text('actual repair');self.commit('real progress')
        check(self.execution,self.store.task('task-001'),self.repo,self.git('rev-parse','HEAD'),findings)

    def test_ci_reruns_have_backoff_deduplication_and_a_budget(self):
        task=self.store.task('task-001');receipt={'head':self.head};observation={'runs':[100]};calls=[]
        def command(*args,**kwargs):
            calls.append(args)
            return subprocess.CompletedProcess(args,0,json.dumps({'head_sha':self.head,'status':'completed','run_attempt':1}),'')
        with patch.object(self.execution,'command',side_effect=command):
            for _ in range(2):
                with self.assertRaises(Wait):retry_infrastructure(self.execution,task,receipt,observation)
        self.assertEqual(sum(x[:3]==('gh','run','rerun') for x in calls),1)
        key='ci_infra_retry:task-001:'+self.head;self.store.set_setting(key,{'count':6,'next':0})
        with self.assertRaises(Attention) as caught:retry_infrastructure(self.execution,task,receipt,observation)
        self.assertEqual(caught.exception.category,'ci_infrastructure')

    def test_real_archive_round_trip_excludes_secrets_and_rejects_corruption(self):
        src=self.root/'cache';(src/'target/debug/deps').mkdir(parents=True)
        (src/'target/debug/deps/libdep.rlib').write_bytes(b'compiled dependency'*100)
        os.link(src/'target/debug/deps/libdep.rlib',src/'target/debug/deps/duplicate.rlib')
        (src/'cargo').mkdir();(src/'cargo/credentials.toml').write_text('must never export')
        archive=self.root/'cache.tar.gz';value=remote.pack(archive,src)
        dst=self.root/'restored';remote.restore(archive,value['sha256'],dst)
        self.assertEqual((dst/'target/debug/deps/libdep.rlib').read_bytes(),(src/'target/debug/deps/libdep.rlib').read_bytes())
        self.assertFalse((dst/'cargo/credentials.toml').exists())
        archive.write_bytes(archive.read_bytes()+b'corrupt')
        with self.assertRaises(ValueError):remote.restore(archive,value['sha256'],dst)

    def test_cache_export_is_downloadable_inside_the_gateway_workspace(self):
        stage=self.root/'stage';stage.mkdir()
        with patch.object(remote.subprocess,'run'),patch.object(remote,'pack',return_value={'bytes':1}) as pack:
            value=remote.export(self.repo,{},stage,{'key':'k'})
        pack.assert_called_once_with(self.repo.parent/'.gyre-build-cache.tar.gz')
        self.assertEqual(value['export']['bytes'],1)

    def test_cache_rejects_links_traversal_and_decompression_bombs(self):
        for name,kind,size in [('../escape',tarfile.REGTYPE,1),('target/link',tarfile.SYMTYPE,0),('target/huge',tarfile.REGTYPE,5)]:
            with self.subTest(name=name):
                archive=self.root/'bad.tar.gz'
                with tarfile.open(archive,'w:gz') as tar:
                    item=tarfile.TarInfo(name);item.type=kind;item.linkname='/etc/passwd';item.size=size
                    tar.addfile(item,io.BytesIO(b'x'*size) if size else None)
                with patch.object(remote,'MAX_EXPANDED',4),self.assertRaises(ValueError):remote.restore(archive,remote.digest(archive),self.root/'bad')
                self.assertFalse((self.root/'escape').exists())

    def test_cache_only_reuses_successful_producer_after_its_source_is_upstream(self):
        source=self.store.directory/'source';shutil.copytree(self.repo,source)
        folder=self.store.directory/'build-cache';folder.mkdir()
        key='a'*64;stem=folder/(key+'-'+self.head);archive=stem.with_suffix('.tar.gz');archive.write_bytes(b'dependency')
        value={'source':self.head,'bytes':archive.stat().st_size,'sha256':remote.digest(archive)};stem.with_suffix('.json').write_text(json.dumps(value))
        self.assertIsNone(select(self.execution,key,self.base))
        self.assertIsNotNone(select(self.execution,key,self.head))
        archive.write_bytes(b'poison');self.assertIsNone(select(self.execution,key,self.head))

    def test_cache_key_changes_with_lockfiles_toolchain_and_build_settings(self):
        (self.repo/'Cargo.lock').write_text('dependencies A')
        with patch.object(remote.subprocess,'check_output',return_value='version1') as versions:
            first=remote.key(self.repo,{'image':'immutable-image'}, {'RUSTFLAGS':'one','RUSTUP_HOME':'/trusted/rustup'})
            self.assertTrue(all(c.kwargs['env']['RUSTUP_HOME']=='/trusted/rustup' for c in versions.call_args_list))
            (self.repo/'Cargo.lock').write_text('dependencies B')
            self.assertNotEqual(first,remote.key(self.repo,{'image':'immutable-image'},{'RUSTFLAGS':'one'}))
            self.assertNotEqual(first,remote.key(self.repo,{'image':'immutable-image'},{'RUSTFLAGS':'two'}))

    def test_old_generated_only_rejection_requeues_review_without_fabricating_approval(self):
        from pipeline.repair_budget import reconcile_generated
        data={'candidate':self.head,'review':{'candidate':self.head,'approved':False},
              'repair':{'category':'review','details':[{'file':'web/dist/','detail':'stale output'}]}}
        self.store.put_task('task-001','g','body',data)
        self.assertEqual(reconcile_generated(self.store),[])  # active work stays intact
        self.store.finish(self.work,self.execution.claim['token'],{})
        self.assertEqual(reconcile_generated(self.store),['task-001'])
        self.assertIsNone(self.store.task('task-001')['data']['review'])
        self.assertIsNone(self.store.task('task-001')['data']['repair'])
        self.assertEqual(self.store.task('task-001')['data']['candidate'],self.head)

    def test_review_defers_only_generated_findings_and_still_blocks_real_source_defects(self):
        from pipeline.stages import review
        body='---\ntitle: Test\nspec_ref: behavior.md §One\nprogress: ready-for-review\n---\n## Acceptance\nReal code.\n'
        doc=self.repo/'specs/tasks/task-001.md';doc.parent.mkdir(parents=True);doc.write_text(body);self.commit('task')
        candidate=self.git('rev-parse','HEAD');self.git('remote','add','origin',str(self.repo))
        self.store.put_task('task-001','g',body,{})
        self.execution.claim['input']={'candidate':candidate};self.execution.claim['stage']='review'
        result={'head':candidate,'branch':'review','valid':True,'agent_exit':0,'stream_exit':0,
            'verdict':{'candidate':candidate,'approved':False,'findings':[{'file':'web/dist/','detail':'stale bundle'}]}}
        with patch.object(self.execution,'cloud_step',return_value=result),patch('pipeline.stages.checkout',return_value=self.repo):
            _,updates,findings=review(self.execution,self.store.task('task-001'))
            self.assertTrue(updates['review']['approved']);self.assertFalse(findings)
            result['verdict']['findings'].append({'file':'web/src/Inbox.svelte','detail':'real source bug'})
            _,updates,findings=review(self.execution,self.store.task('task-001'))
            self.assertFalse(updates['review']['approved']);self.assertTrue(findings)

    def test_old_setup_handoff_recovers_actual_review_and_never_discards_running_work(self):
        from pipeline.ci import recover_setup_handoff
        raw=self.base;published=self.head
        receipt={'candidate':raw,'head':published,'base':self.base,'passed':True}
        result={'head':raw,'valid':True,'verdict':{'candidate':raw,'approved':True,'findings':[]}}
        self.store.finish(self.work,self.execution.claim['token'],{})
        review_work=self.store.enqueue('review','task-001','g',{'candidate':raw});claim=self.store.claim('review','reviewer')
        self.store.finish(review_work,claim['token'],result)
        self.store.put_task('task-001','g','body',{'candidate':published,'repair':{'category':'ci','source':published},'pr':'url','published_head':published})
        self.work=self.store.enqueue('implement','task-001','g',{'again':True})
        self.execution=Execution(self.store,self.store.claim('implement','active-coder'))
        task=self.store.task('task-001');observation={'status':'infrastructure'}
        self.assertFalse(recover_setup_handoff(self.execution,task,receipt,observation))
        self.store.finish(self.work,self.execution.claim['token'],{})
        self.assertTrue(recover_setup_handoff(self.execution,task,receipt,observation))
        self.assertEqual(self.store.task('task-001')['data']['candidate'],raw)
        self.assertEqual(self.store.task('task-001')['data']['verified'],receipt)
        self.assertIsNone(self.store.task('task-001')['data']['repair'])

    def test_efficiency_report_does_not_count_abandoned_old_claims_as_active_all_night(self):
        metrics=helper('dev-efficiency');now=1000000
        self.store.db.execute("DELETE FROM events")
        self.store.db.execute("UPDATE work SET state='obsolete',updated=? WHERE id=?",(now-100000,self.work))
        self.store.db.execute("INSERT INTO events(work,kind,detail,at) VALUES(?,?,?,?)",(self.work,'claimed','{}',now-200000))
        result=metrics.report(self.store.db,hours=1,now=now)
        self.assertEqual(sum(result['compute_worker_minutes'].values()),0)

    def setup_result(self, *, product=False, changed=False, stale=False, mixed=False, unknown=False):
        checks=[{'name':'Test','detailsUrl':'https://github.com/x/y/actions/runs/10/job/100'}]
        if mixed:checks.append({'name':'Other','detailsUrl':'https://github.com/x/y/actions/runs/10/job/101'})
        def run(*args,**kwargs):
            url=args[2]
            if '/contents/' in url:value={'type':'file','sha':('d' if changed and self.head in url else 'c')*40}
            elif '/jobs/' in url:value={'id':int(url.rsplit('/',1)[1]),'run_id':10,'name':'Other' if url.endswith('101') else 'Test','head_sha':self.base if stale else self.head,'status':'completed','conclusion':'failure','run_attempt':1,'steps':[{'name':'Run tests' if product or url.endswith('101') else 'Install mold linker','conclusion':'failure','started_at':'2026-11-11T10:00:00Z','completed_at':'2026-11-11T10:01:00Z'}]}
            else:value={'id':10,'head_sha':self.head,'status':'completed','conclusion':'failure','run_attempt':1,'path':'.github/workflows/ci.yml'}
            return subprocess.CompletedProcess(args,0,json.dumps(value),'')
        return ci.transient_setup_failure(run,'x/y',self.head,self.base,checks,{10:('Test\tUNKNOWN STEP\t2026-11-11T10:00:30Z ' if unknown else 'Test\tInstall mold linker\t')+'Temporary failure resolving archive.ubuntu.com\n'})

    def test_real_dns_setup_signature_is_infrastructure_but_product_and_changed_inputs_are_not(self):
        self.assertEqual(self.setup_result()['status'],'infrastructure')
        self.assertEqual(self.setup_result(unknown=True)['status'],'infrastructure')
        for kwargs in ({'product':True},{'changed':True},{'mixed':True}):self.assertIsNone(self.setup_result(**kwargs))
        with self.assertRaisesRegex(RuntimeError,'evidence changed'):self.setup_result(stale=True)

if __name__=='__main__':unittest.main()
