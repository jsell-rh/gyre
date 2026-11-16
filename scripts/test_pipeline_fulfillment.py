"""Real Git trees + simulated trusted verifier protocol; no product builds."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from pipeline.fulfillment import reconcile
from pipeline.store import Store
from pipeline.verification import COMMANDS
from pipeline.cockpit import snapshot

BODY='---\ntitle: Existing feature\nspec_ref: behavior.md §One\ndepends_on: []\nprogress: not-started\n---\n## Acceptance\nReal implementation\n'

class FulfillmentTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        self.store=Store(Path(self.tmp.name)/'state');self.addCleanup(self.store.close)
        self.repo=Path(self.tmp.name)/'repo';self.repo.mkdir()
        self.git('init','-q');self.git('config','user.name','Test');self.git('config','user.email','test@example.com')
        (self.repo/'specs/tasks').mkdir(parents=True)
        self.doc=self.repo/'specs/tasks/task-001.md';self.doc.write_text(BODY)
        (self.repo/'implementation.txt').write_text('already implemented\n')
        self.commit('Upstream implementation');self.base=self.git('rev-parse','HEAD')
        self.doc.write_text(BODY.replace('not-started','complete')+'\n## Shipped\nVerified existing code.\n')
        self.commit('Operational record');self.head=self.git('rev-parse','HEAD')
        self.store.put_task('task-001','g',BODY,{'dependencies':[], 'candidate':self.head,
            'review':{'candidate':self.head,'approved':True},'triaged':'g'})
        self.work=self.store.enqueue('verify','task-001','g',{'candidate':self.head,'base':self.base})
        claim=self.store.claim('verify','trusted-verifier')
        self.receipt={'candidate':self.head,'base':self.base,'head':self.head,
            'tree':self.git('rev-parse','HEAD^{tree}'),'tested_head':self.head,'engine':'gateway',
            'passed':True,'checks':[{'command':c,'exit_code':0} for c in COMMANDS]}
        self.store.finish(self.work,claim['token'],self.receipt,{'verified':self.receipt})
        self.proof=self.store.directory/'attempts'/self.work/'1/checkout'
        self.proof.parent.mkdir(parents=True);shutil.copytree(self.repo,self.proof)
        self.git('checkout','-q','--detach',self.base)
        (self.repo/'unrelated.txt').write_text('main advanced\n');self.commit('Unrelated upstream advance')
        self.main=self.git('rev-parse','HEAD')

    def git(self,*args):return subprocess.check_output(['git',*args],cwd=self.repo,text=True,stderr=subprocess.PIPE).strip()
    def commit(self,message):self.git('add','.');self.git('-c','core.hooksPath=/dev/null','commit','-qm',message)

    def test_notes_only_receipt_resolves_without_inventing_a_shipped_merge(self):
        self.assertEqual(reconcile(self.store,self.repo,self.main),['task-001'])
        self.assertEqual(reconcile(self.store,self.repo,self.main),[])
        data=self.store.task('task-001')['data']
        self.assertEqual(data['resolved']['source'],self.base)
        self.assertEqual(data['candidate'],self.head)
        self.assertIsNone(self.store.integration_lane())
        view=snapshot(self.store)
        self.assertEqual(view['tasks'][0]['state'],'resolved')
        self.assertEqual(view['confirmed_merges'],0)

    def test_redundant_read_only_verifier_retires_but_its_resource_remains_owned_for_cleanup(self):
        work=self.store.enqueue('verify','task-001','g',{'candidate':self.head,'base':self.main})
        claim=self.store.claim('verify','redundant-verifier')
        self.store.reserve('owned',work,claim['token'],'sandbox',10,{})
        self.assertEqual(reconcile(self.store,self.repo,self.main),['task-001'])
        row=self.store.db.execute('select state,token from work where id=?',(work,)).fetchone()
        self.assertEqual(row['state'],'obsolete');self.assertGreater(row['token'],claim['token'])
        self.assertEqual(self.store.db.execute("select work from resources where name='owned'").fetchone()[0],work)
        from pipeline.eligibility import eligible
        cleanup=self.store.enqueue('cleanup','task-001','g',{'resource':'owned','resource_token':claim['token']})
        self.assertIsNotNone(self.store.claim('cleanup','cleaner',eligible=lambda w:eligible(self.store,w)))

    def test_review_or_implementation_work_is_never_retired(self):
        work=self.store.enqueue('implement','task-001','g',{'base':self.main})
        claim=self.store.claim('implement','coder')
        self.assertEqual(reconcile(self.store,self.repo,self.main),[])
        self.assertEqual(self.store.db.execute('select state from work where id=?',(work,)).fetchone()[0],'claimed')

    def test_unrecorded_receipt_and_unresolved_binding_are_not_proof(self):
        task=self.store.task('task-001')
        self.store.put_task('task-001','other-generation',BODY,task['data'])
        self.assertEqual(reconcile(self.store,self.repo,self.main),[])
        self.store.put_task('task-001','g',BODY,task['data']|{'requirements':{'errors':['missing section']}})
        self.assertEqual(reconcile(self.store,self.repo,self.main),[])

    def test_normative_changes_and_existing_publication_prevent_fast_path(self):
        task=self.store.task('task-001')
        self.store.put_task('task-001','g',BODY+'\n## New requirement\nDifferent behavior\n',task['data'])
        self.assertEqual(reconcile(self.store,self.repo,self.main),[])
        self.store.put_task('task-001','g',BODY,task['data']|{'pr':'https://github.com/org/repo/pull/1'})
        self.assertEqual(reconcile(self.store,self.repo,self.main),[])

    def test_generated_production_assets_cannot_be_called_notes(self):
        # Even generated frontend output must ship normally.
        subprocess.run(['git','checkout','-q','--detach',self.head],cwd=self.proof,check=True)
        (self.proof/'web/dist').mkdir(parents=True);(self.proof/'web/dist/index.html').write_text('new production output')
        subprocess.run(['git','add','.'],cwd=self.proof,check=True)
        subprocess.run(['git','-c','core.hooksPath=/dev/null','commit','-qm','asset'],cwd=self.proof,check=True)
        new=subprocess.check_output(['git','rev-parse','HEAD'],cwd=self.proof,text=True).strip()
        receipt=self.receipt|{'head':new,'tested_head':new,'tree':subprocess.check_output(['git','rev-parse','HEAD^{tree}'],cwd=self.proof,text=True).strip()}
        self.store.db.execute('update work set result=? where id=?',(json.dumps(receipt),self.work))
        task=self.store.task('task-001');self.store.put_task('task-001','g',BODY,task['data']|{'verified':receipt})
        self.assertEqual(reconcile(self.store,self.repo,self.main),[])

if __name__=='__main__':unittest.main()
