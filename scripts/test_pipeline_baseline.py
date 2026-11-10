import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest

from pipeline.execution import Retry
from pipeline.stages import baseline_repair, static_baseline_observation
from pipeline.store import Store


class BaselineIdentityTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.store = Store(self.root / 'state')
        self.addCleanup(self.store.close)
        specs = self.store.directory / 'source/specs'
        specs.mkdir(parents=True)
        (specs / 'GOAL.md').write_text('Real implementations and meaningful checks.\n')
        self.base = 'a' * 40

    def log(self, attempt, environment='e' * 64):
        path = self.root / attempt / 'output.log'
        path.parent.mkdir(parents=True)
        value = {'base': self.base, 'environment': environment,
                 'probe': ['bash', 'scripts/check-task-commit-attribution.sh'],
                 'log': 'A commit is missing its attribution.'}
        path.write_text('$ bash ' + str(path.parent / 'tools/checks.sh') + '\n' +
                        'GYRE_BASELINE_FAILURE_JSON ' + json.dumps(value) + '\n')
        return path

    def test_two_attempt_paths_create_one_repair_for_the_same_reproduced_failure(self):
        first = static_baseline_observation(self.log('attempt-a'), self.base)
        second = static_baseline_observation(self.log('attempt-b'), self.base)
        execution = SimpleNamespace(store=self.store)
        self.assertEqual(baseline_repair(execution, first), baseline_repair(execution, second))
        self.assertEqual(len(self.store.tasks()), 1)
        other = static_baseline_observation(self.log('attempt-c', 'f' * 64), self.base)
        self.assertEqual(baseline_repair(execution, first), baseline_repair(execution, other))
        # The same failure remains one prerequisite as main advances.
        other['base'] = 'b' * 40
        self.assertEqual(baseline_repair(execution, first), baseline_repair(execution, other))
        # A different failure of the same probe must not be conflated.
        different = self.log('different')
        different.write_text(different.read_text().replace('A commit is missing its attribution.', 'Another independent failure.'))
        self.assertNotEqual(baseline_repair(execution, first), baseline_repair(execution, static_baseline_observation(different,self.base)))

    def test_a_process_exit_without_matching_reproduction_cannot_create_a_repair(self):
        path = self.root / 'failed.log'
        path.write_text('exit 81 without baseline evidence\n')
        with self.assertRaises(Retry):
            static_baseline_observation(path, self.base)
        with self.assertRaises(Retry):
            static_baseline_observation(self.log('wrong-base'), 'b' * 40)
        self.assertEqual(self.store.tasks(), [])


if __name__ == '__main__':
    unittest.main()
