import json
from unittest.mock import patch
from test_pipeline_fullsend import FullsendFixture
from pipeline.contract_recovery import normalize
from pipeline.stages import implement


class SavedContractRecoveryTests(FullsendFixture):
    def saved(self, amendment=None):
        from pipeline.execution import Execution
        cfg,repo,_=self.prepare()
        task=self.store.task('task-9999')
        assigned=self.body.replace('Add returns the sum.','- [ ] Add returns the sum.')
        observed=assigned.replace('progress: not-started','progress: ready-for-review').replace('- [ ]','- [x]')
        if amendment:observed=observed.replace('sum.',amendment)
        path=repo/'specs/tasks/task-9999.md';path.write_text(observed)
        (repo/'code.py').write_text('def add(a, b):\n    return a + b\n')
        self.call('git','add','.',cwd=repo);self.call('git','-c','core.hooksPath=/dev/null','commit','-qm','feat(task-9999): arithmetic',cwd=repo)
        head=self.call('git','rev-parse','HEAD',cwd=repo).strip()
        self.call('git','push','-q','origin','HEAD:refs/heads/saved',cwd=repo)
        receipt={'head':head,'base':self.base,'published':True,'valid':True,'agent_exit':0,'stream_exit':0,'task_body':observed}
        (self.execution.directory/'outcome.json').write_text(json.dumps(receipt))
        repair={'id':self.work,'category':'contract','source':head,'checkpoint':True,'agent_exit':0}
        self.store.finish(self.work,self.claim['token'],{}, {'candidate':head,'repair':repair})
        self.store.put_task(task['name'],'g',assigned,self.store.task(task['name'])['data'])
        work=self.store.enqueue('implement',task['name'],'g',{'base':self.base,'candidate':head,'repair':repair})
        claim=self.store.claim('implement','recovery');self.execution=Execution(self.store,claim)
        return head

    def test_completed_source_is_normalized_and_sent_to_review_without_model(self):
        original=self.saved()
        with patch.object(self.execution,'cloud_step',side_effect=AssertionError('must not recode')):
            result,updates,_=implement(self.execution,self.store.task('task-9999'))
        repo=self.execution.directory/'checkout'
        self.assertEqual(self.call('git','show',result['head']+':code.py',cwd=repo), 'def add(a, b):\n    return a + b\n')
        self.assertEqual(result['normalized_from'],original)
        self.assertIn('- [ ] Add returns the sum.',result['task_body'])
        self.assertIsNone(updates['repair']);self.assertIsNone(updates['review']);self.assertIsNone(updates['verified'])
        self.assertNotIn('delivered',updates)
        self.call('git','merge-base','--is-ancestor',original,result['head'],cwd=repo)
        self.assertTrue(self.call('git','ls-remote','origin','refs/heads/'+result['branch'],cwd=repo).startswith(result['head']))

    def test_amended_requirement_is_never_normalized(self):
        self.saved('difference.')
        self.assertIsNone(normalize(self.execution,self.store.task('task-9999')))

    def test_failed_agent_receipt_cannot_be_promoted(self):
        self.saved()
        repair=self.store.task('task-9999')['data']['repair']
        p=next((self.store.directory/'attempts'/repair['id']).glob('*/outcome.json'))
        result=json.loads(p.read_text());result['agent_exit']=1;p.write_text(json.dumps(result))
        self.assertIsNone(normalize(self.execution,self.store.task('task-9999')))

    def test_detached_clone_without_main_tracking_ref_is_supported(self):
        from pipeline.stages import checkout
        self.saved()
        path=checkout(self.execution,self.store.task('task-9999')['data']['candidate'])
        self.call('git','update-ref','-d','refs/remotes/origin/main',cwd=path)
        result=normalize(self.execution,self.store.task('task-9999'))
        self.assertTrue(result['valid'])
        self.call('git','rev-parse','--verify','refs/remotes/origin/main',cwd=path)
