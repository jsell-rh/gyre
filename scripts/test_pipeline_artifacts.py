"""Real Git conflicts and artifact receipts; no product compilation on the host."""
from pathlib import Path
import json
import subprocess
import tempfile
import unittest
from pipeline.catalog import helper

artifacts=helper('pipeline-artifacts')


class ArtifactTest(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name);self.repo=self.root/'work';self.origin=self.root/'origin.git'
        subprocess.run(['git','init','-q','--bare',str(self.origin)],check=True)
        self.repo.mkdir();self.git('init','-q');self.git('config','user.name','Test');self.git('config','user.email','test@example.com');self.git('config','commit.gpgsign','false');self.git('config','core.hooksPath','/dev/null')
        self.git('remote','add','origin',str(self.origin))
        (self.repo/'web/dist').mkdir(parents=True);(self.repo/'web/dist/index.html').write_text('base')
        (self.repo/'source.txt').write_text('base')
        (self.repo/'specs/tasks').mkdir(parents=True)
        self.body='---\ntitle: task\ncommits: ["'+'a'*40+'"]\nprogress: complete\n---\nRequired behavior.\n'
        (self.repo/'specs/tasks/task-001.md').write_text(self.body)
        self.commit('base');self.base=self.git('rev-parse','HEAD')
        self.branch='pipeline/verified/'+'a'*32+'-1'

    def git(self,*args,check=True):
        r=subprocess.run(['git',*args],cwd=self.repo,capture_output=True,text=True)
        if check:self.assertEqual(r.returncode,0,r.stderr)
        return r.stdout.strip() if check else r

    def commit(self,title):self.git('add','-A');self.git('commit','-qm',title)

    def conflict(self,source=False):
        self.git('checkout','-qb','feature')
        (self.repo/'web/dist/index.html').write_text('candidate')
        if source:(self.repo/'source.txt').write_text('candidate')
        self.commit('candidate');candidate=self.git('rev-parse','HEAD')
        self.git('checkout','--detach',self.base)
        (self.repo/'web/dist/index.html').write_text('main')
        if source:(self.repo/'source.txt').write_text('main')
        self.commit('main');base=self.git('rev-parse','HEAD')
        self.assertNotEqual(self.git('merge','--no-ff',candidate,check=False).returncode,0)
        return base

    def test_bundle_only_conflict_is_resolved_without_worker_but_source_conflict_survives(self):
        base=self.conflict(source=True)
        generated,remaining=artifacts.resolve_generated_conflicts(self.repo,base)
        self.assertEqual(generated,['web/dist/index.html'])
        self.assertEqual(remaining,['source.txt'])
        self.assertIn('<<<<<<<',(self.repo/'source.txt').read_text())
        self.assertEqual((self.repo/'web/dist/index.html').read_text(),'main')
        (self.repo/'source.txt').write_text('resolved source');self.git('add','source.txt');self.git('commit','--no-edit')
        self.assertEqual(self.git('diff','--name-only','--diff-filter=U'),'')

    def test_published_generated_conflict_is_reproduced_without_compute(self):
        from pipeline.ci import generated_conflict
        from types import SimpleNamespace
        from unittest.mock import patch
        base=self.conflict(source=False)
        candidate=self.git('rev-parse','feature');self.git('merge','--abort');self.git('checkout','--detach',candidate)
        self.git('push','origin',base+':refs/heads/main')
        calls=[]
        def command(*args,**kwargs):
            self.assertEqual(args[0],'git','classification must never invoke a compiler or model')
            calls.append(args)
            return subprocess.run(args,**kwargs,capture_output=True,text=True)
        with patch('pipeline.stages.checkout',return_value=self.repo):
            generated,paths=generated_conflict(SimpleNamespace(command=command),{}, {'head':candidate},base)
        self.assertTrue(generated);self.assertEqual(paths,['web/dist/index.html'])
        self.assertFalse(self.repo.exists(),'temporary inspection checkout must be discarded')

    def test_rename_rename_generated_assets_can_finish_merge_without_source_repair(self):
        (self.repo/'web/dist/base.js').write_text('same generated content');self.commit('bundle')
        base=self.git('rev-parse','HEAD')
        self.git('checkout','-qb','candidate-assets');self.git('mv','web/dist/base.js','web/dist/candidate.js');self.commit('candidate bundle')
        candidate=self.git('rev-parse','HEAD')
        self.git('checkout','--detach',base);self.git('mv','web/dist/base.js','web/dist/main.js');self.commit('upstream bundle')
        upstream=self.git('rev-parse','HEAD')
        self.assertNotEqual(self.git('merge','--no-ff',candidate,check=False).returncode,0)
        generated,remaining=artifacts.resolve_generated_conflicts(self.repo,upstream)
        self.assertTrue(generated);self.assertEqual(remaining,[])
        self.git('commit','--no-edit')
        self.assertEqual(self.git('status','--porcelain'),'')
        self.assertTrue((self.repo/'web/dist/main.js').exists())
        self.assertFalse((self.repo/'web/dist/candidate.js').exists())

    def test_fresh_artifact_publication_preserves_requirements_and_all_existing_attribution(self):
        identity={'head':self.base,'tree':self.git('rev-parse','HEAD^{tree}')}
        (self.repo/'web/dist/index.html').write_text('fresh build from integrated source')
        (self.repo/'web/dist/new.js').write_text('fresh bundle')
        publication=artifacts.publish(self.repo,'task-001',self.branch)
        artifacts.validate(self.repo,identity,publication,'task-001',self.branch)
        self.assertEqual((self.repo/'specs/tasks/task-001.md').read_text(),artifacts.append_commit(self.body,publication['artifact_commit']))
        after=(self.repo/'specs/tasks/task-001.md').read_text()
        self.assertIn('a'*40,after)
        self.assertEqual(after.split('---',2)[2],self.body.split('---',2)[2])
        self.assertIn('title: task',after);self.assertIn('progress: complete',after)
        self.assertEqual(self.git('ls-remote','origin','refs/heads/'+self.branch).split()[0],publication['head'])
        # Appending attribution cannot weaken requirements, even with the
        # expected two-commit ancestry and an otherwise valid generated build.
        (self.repo/'specs/tasks/task-001.md').write_text(after.replace('Required behavior.','Weakened behavior.'))
        self.git('add','specs/tasks/task-001.md');self.git('commit','--amend','--no-edit')
        forged_metadata=publication|{'head':self.git('rev-parse','HEAD'),'tree':self.git('rev-parse','HEAD^{tree}')}
        with self.assertRaisesRegex(ValueError,'requirements'):
            artifacts.validate(self.repo,identity,forged_metadata,'task-001',self.branch)
        # A forged "generated build" that changes source cannot be approved.
        self.git('checkout','--detach',self.base)
        (self.repo/'source.txt').write_text('unreviewed source');self.commit('forged artifact')
        forged=publication|{'artifact_commit':self.git('rev-parse','HEAD')}
        with self.assertRaisesRegex(ValueError,'changes source'):
            artifacts.validate(self.repo,identity,forged,'task-001',self.branch)

    def test_unchanged_generated_tree_has_no_extra_commit(self):
        identity={'head':self.base,'tree':self.git('rev-parse','HEAD^{tree}')}
        publication=artifacts.publish(self.repo,'task-001',self.branch)
        self.assertIsNone(publication['artifact_commit'])
        self.assertEqual(publication['head'],self.base)
        artifacts.validate(self.repo,identity,publication,'task-001',self.branch)
        self.assertEqual((self.repo/'specs/tasks/task-001.md').read_text(),self.body)
