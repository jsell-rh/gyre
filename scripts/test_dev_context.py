"""Prompt projection must retain exactly the task's desired contract."""
import importlib.util
from pathlib import Path
import unittest


spec = importlib.util.spec_from_file_location('context', Path(__file__).with_name('dev-context.py'))
context = importlib.util.module_from_spec(spec)
spec.loader.exec_module(context)


class ContextTest(unittest.TestCase):
    def test_large_generated_diagnostics_do_not_replace_requirements(self):
        body = ('---\ntitle: "Repair verified failure on main abcdef012345"\n'
                'spec_ref: GOAL.md\nprogress: needs-revision\ncommits: []\n---\n'
                '## Required behavior\nReject invalid payloads.\n'
                '## Baseline failure\n' + 'old diagnostic\n' * 10000 +
                '## Acceptance criteria\nKeep all enforcement active.\n'
                '## Review\nNewest concrete finding: restore unrelated script.\n')
        result = context.task_context(body, 'task.md')
        self.assertLess(len(result), 9000)
        self.assertIn('Reject invalid payloads.', result)
        self.assertIn('Keep all enforcement active.', result)
        self.assertIn('Newest concrete finding', result)
        self.assertIn('Read task.md', result)
        self.assertEqual(context.contract.generation(body, {}), context.contract.generation(result, {}))
        requirements = context.task_context(body, 'task.md', include_history=False)
        self.assertNotIn('Newest concrete finding', requirements)
        self.assertNotIn('old diagnostic', requirements)
        self.assertIn('Keep all enforcement active.', requirements)
        self.assertEqual(context.contract.generation(body, {}), context.contract.generation(requirements, {}))

    def test_non_generated_baseline_and_unknown_headings_are_requirements(self):
        body = ('---\ntitle: ordinary feature\nprogress: ready-for-review\n---\n'
                '## Baseline failure\nThis is normative.\n'
                '## Custom requirements\nMust preserve me.\n'
                '## Implementation Log\n## nested operational heading\nAlso normative to generation.\n')
        result = context.task_context(body, 'task.md')
        self.assertIn('This is normative.', result)
        self.assertIn('Must preserve me.', result)
        self.assertEqual(context.contract.generation(body, {}), context.contract.generation(result, {}))


if __name__ == '__main__':
    unittest.main()
