from unittest.mock import patch
import unittest
import test_pipeline_efficiency as fixture
from pipeline.review_preflight import check
from pipeline.stages import review


class ReviewPreflightTests(unittest.TestCase):
    git=fixture.EfficiencyTests.git
    commit=fixture.EfficiencyTests.commit
    def setUp(self):
        fixture.EfficiencyTests.setUp(self)
        self.git('remote','add','origin',str(self.repo))
        self.execution.claim['input'].update(candidate=self.head,base=self.base)
        self.git('checkout','-q','--detach',self.base)
        (self.repo/'code').write_text('upstream conflicting source\n');self.commit('main advances')
        self.main=self.git('rev-parse','HEAD');self.store.set_setting('observed_main',self.main)

    def test_real_source_conflict_returns_to_rebase_without_review_model_or_changed_source(self):
        self.git('checkout','-q','--detach',self.head)
        with patch('pipeline.stages.checkout',return_value=self.repo),patch.object(self.execution,'cloud_step',side_effect=AssertionError('do not review conflicted source')):
            result,updates,findings=review(self.execution,self.store.task('task-001'))
        self.assertEqual(result['preflight'],'source_conflicts')
        self.assertEqual(updates['repair']['base'],self.main)
        self.assertEqual(updates['repair']['files'],['code'])
        self.assertIsNone(updates['review']);self.assertIsNone(updates['verified'])
        self.assertNotIn('candidate',updates)
        self.assertEqual(self.git('rev-parse','HEAD'),self.head)
        self.assertEqual(self.git('status','--porcelain'),'')

    def test_assigned_base_is_not_evidence_the_candidate_includes_it(self):
        self.execution.claim['input']['base']=self.main
        with patch('pipeline.stages.checkout',return_value=self.repo):
            result,updates,_=check(self.execution,self.store.task('task-001'))
        self.assertEqual(result['preflight'],'source_conflicts')
        self.assertEqual(updates['repair']['base'],self.main)

    def test_generated_only_conflict_stays_with_verifier_and_does_not_request_coding(self):
        self.git('checkout','-q','--detach',self.base)
        p=self.repo/'web/dist/index.html';p.parent.mkdir(parents=True);p.write_text('base\n');self.commit('generated base')
        base=self.git('rev-parse','HEAD');p.write_text('candidate\n');self.commit('candidate assets');candidate=self.git('rev-parse','HEAD')
        self.git('checkout','-q','--detach',base);p.write_text('main\n');self.commit('main assets');main=self.git('rev-parse','HEAD')
        self.execution.claim['input'].update(base=base,candidate=candidate);self.store.set_setting('observed_main',main)
        with patch('pipeline.stages.checkout',return_value=self.repo):
            self.assertIsNone(check(self.execution,self.store.task('task-001')))

    def test_clean_merge_still_requires_normal_review(self):
        self.git('checkout','-q','--detach',self.base)
        (self.repo/'unrelated').write_text('new main file');self.commit('unrelated main')
        self.store.set_setting('observed_main',self.git('rev-parse','HEAD'))
        with patch('pipeline.stages.checkout',return_value=self.repo):
            self.assertIsNone(check(self.execution,self.store.task('task-001')))

    def test_preflight_records_the_handoff_through_the_real_executor(self):
        from pipeline.catalog import helper
        cli=helper('dev-pipeline')
        self.store.finish(self.work,self.execution.claim['token'],{})
        self.store.put_task('task-001','g','body',{'candidate':self.head,'candidate_base':self.base,'triaged':'g','dependencies':[]})
        work=self.store.enqueue('review','task-001','g',{'candidate':self.head,'base':self.base})
        claim=self.store.claim('review','reviewer')
        with patch('pipeline.stages.checkout',return_value=self.repo),patch('pipeline.execution.Execution.cloud_step',side_effect=AssertionError('no model')):
            cli.run_claim(self.store,claim)
        row=self.store.db.execute('SELECT state FROM work WHERE id=?',(work,)).fetchone()
        self.assertEqual(row['state'],'succeeded')
        task=self.store.task('task-001')
        self.assertEqual(task['data']['repair']['category'],'rebase')
        self.assertIsNone(task['data']['review'])
        self.assertEqual(len(self.store.findings('task-001')),1)
