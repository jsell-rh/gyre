import tempfile
import unittest
from pathlib import Path
from pipeline.catalog import discover, metadata, sync_checkout
from pipeline.store import Store
from pipeline.eligibility import eligible


class DiscoveryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.store = Store(self.temp.name)

    def tearDown(self):
        self.store.close()
        self.temp.cleanup()

    def task(self, name, **data):
        self.store.put_task(name, 'g', 'body', {'dependencies': [], 'triaged': 'g', **data})

    def count(self, stage, base='main'):
        return len(discover(self.store, stage, base))

    def test_multiline_dependencies_preserved(self):
        self.assertEqual(metadata('---\ntitle: thing\ndepends_on:\n  - task-001\n  - task-002\nprogress: ready\n---\n')['dependencies'], ['task-001', 'task-002'])

    def test_dependency_blocks_implementation_until_delivery(self):
        self.task('task-001', triaged='g')
        self.task('task-002', triaged='g', dependencies=['task-001'])
        self.assertEqual(self.count('implement'), 1)
        self.task('task-001', delivered={'sha': 'actual'})
        self.assertEqual(self.count('implement'), 1)
        claim = self.store.claim('implement', 'worker')
        self.assertEqual(claim['task'], 'task-002')

    def test_missing_and_cyclic_dependencies_do_not_run(self):
        self.task('task-001', triaged='g', dependencies=['task-002'])
        self.task('task-002', triaged='g', dependencies=['task-001'])
        self.task('task-003', triaged='g', dependencies=['task-404'])
        self.assertEqual(self.count('implement'), 0)

    def test_review_and_verification_require_exact_source(self):
        self.task('task-001', candidate='a', candidate_base='main',
                  review={'candidate': 'old', 'approved': True})
        self.assertEqual(self.count('review'), 1)
        self.assertEqual(self.count('verify'), 0)
        self.assertEqual(self.count('publish'), 0)
        self.task('task-001', candidate='a', candidate_base='main',
                  review={'candidate': 'a', 'approved': True},
                  verified={'candidate': 'old', 'base': 'main'})
        self.assertEqual(self.count('verify'), 1)
        self.assertEqual(self.count('publish'), 0)

    def test_main_advancement_invalidates_verification(self):
        self.task('task-001', candidate='a', candidate_base='main',
                  review={'candidate': 'a', 'approved': True},
                  verified={'candidate': 'a', 'base': 'main'})
        self.assertEqual(self.count('publish', 'newmain'), 0)
        self.assertEqual(self.count('verify', 'newmain'), 1)

    def test_failure_is_repair_work_and_blocks_promotion(self):
        self.task('task-001', triaged='g', candidate='a', candidate_base='main',
                  review={'candidate': 'a', 'approved': True},
                  verified={'candidate': 'a', 'base': 'main'},
                  repair={'category': 'ci', 'source': 'a', 'id': 'finding'})
        self.assertEqual(self.count('implement'), 1)
        self.assertEqual(self.count('review'), 0)
        self.assertEqual(self.count('verify'), 0)
        self.assertEqual(self.count('publish'), 0)

    def test_discovery_replay_does_not_duplicate_work(self):
        self.task('task-001', triaged='g')
        self.count('implement')
        self.count('implement')
        self.assertEqual(self.store.db.execute('SELECT count(*) FROM work').fetchone()[0], 1)

    def test_blocked_review_can_resume_with_same_source_after_dependency_delivery(self):
        self.task('task-001', delivered={'sha': 'one'})
        self.task('task-002', dependencies=['task-001'], candidate='a', candidate_base='main')
        original = discover(self.store, 'review', 'main')[0]
        self.task('task-001')
        self.assertEqual(self.count('review'), 0)
        self.assertIsNone(self.store.claim('review', 'worker'))
        self.task('task-001', delivered={'sha': 'two'})
        self.assertEqual(discover(self.store, 'review', 'main'), [original])
        self.assertEqual(self.store.claim('review', 'worker')['id'], original)

    def test_claim_rechecks_dependencies_even_without_another_discovery(self):
        self.task('task-001', delivered={'sha': 'one'})
        self.task('task-002', dependencies=['task-001'])
        discover(self.store, 'implement', 'main')
        self.task('task-001')
        claim = self.store.claim('implement', 'worker', eligible=lambda work: eligible(self.store, work))
        # task-002's already queued work must not start after its prerequisite
        # is reopened, even if cron hasn't performed discovery again yet.
        self.assertTrue(claim is None or claim['task'] == 'task-001')

    def test_completed_label_does_not_hide_a_later_spec_change(self):
        root = Path(self.temp.name) / 'checkout'
        (root / 'specs/system').mkdir(parents=True)
        (root / 'specs/tasks').mkdir()
        (root / 'specs/GOAL.md').write_text('real code')
        spec = root / 'specs/system/behavior.md'
        spec.write_text('requirement one')
        body = '---\ntitle: Thing\nspec_ref: behavior.md\ndepends_on: []\nprogress: complete\ncommits: []\n---\n\n## Required behavior\nImplement the spec.\n'
        (root / 'specs/tasks/task-001.md').write_text(body)
        sync_checkout(self.store, root, 'old-main')
        original = self.store.task('task-001')
        self.assertTrue(original['data']['delivered']['imported'])
        spec.write_text('requirement one plus mandatory enforcement')
        sync_checkout(self.store, root, 'new-main')
        changed = self.store.task('task-001')
        self.assertNotEqual(changed['generation'], original['generation'])
        self.assertNotIn('delivered', changed['data'])
        self.assertEqual(changed['data']['candidate'], 'old-main')
        self.assertEqual(changed['data']['repair']['category'], 'requirements_changed')


if __name__ == '__main__':
    unittest.main()
