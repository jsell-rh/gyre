import json
from pathlib import Path
import tempfile
import unittest

from pipeline.diagnostics import diagnose


class DiagnosisTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.transcript = self.root / 'runs/test/iteration-1/output.jsonl'
        self.transcript.parent.mkdir(parents=True)

    def events(self, *events):
        self.transcript.write_text(''.join(json.dumps(e)+'\n' for e in events))

    def test_timeout_retains_real_runner_budget_and_unfinished_tool_without_arguments(self):
        (self.root / 'output.log').write_text('agent timed out after 1h0m7s without completing (timeout: 1h0m0s)\n')
        self.events({'type':'tool_execution_start','toolCallId':'one','toolName':'bash',
                     'args':{'command':'SECRET_SENTINEL'}})
        value = diagnose(self.root, 1)
        self.assertEqual(value['category'], 'model_timeout')
        self.assertEqual(value['budget'], '1h0m0s')
        self.assertEqual(value['unfinished_tools'], ['bash'])
        self.assertNotIn('SECRET_SENTINEL', (self.root/'diagnostic.json').read_text())

    def test_429_is_read_from_actual_error_not_model_discussion_or_tool_output(self):
        (self.root / 'output.log').write_text('Model said: rate limit 429\n')
        self.events({'type':'message_end','message':{'role':'assistant','stopReason':'stop',
                     'content':[{'type':'text','text':'An earlier HTTP 429 happened'}]}})
        self.assertEqual(diagnose(self.root,1)['category'], 'runner_failure')
        self.events({'type':'message_end','message':{'role':'assistant','stopReason':'error',
                     'errorMessage':'HTTP 429 token=SECRET_SENTINEL'}})
        value = diagnose(self.root,1)
        self.assertEqual(value['category'], 'inference_rate_limit')
        self.assertNotIn('SECRET_SENTINEL',json.dumps(value))

    def test_completed_tool_is_not_reported_as_still_running(self):
        self.events({'type':'tool_execution_start','toolCallId':'one','toolName':'bash'},
                    {'type':'tool_execution_end','toolCallId':'one'})
        self.assertEqual(diagnose(self.root,1)['unfinished_tools'], [])

    def test_recovered_api_error_does_not_mask_later_runner_failure(self):
        self.events({'type':'message_end','message':{'role':'assistant','stopReason':'error','errorMessage':'HTTP 429'}},
                    {'type':'message_end','message':{'role':'assistant','stopReason':'toolUse'}})
        self.assertEqual(diagnose(self.root,1)['category'],'runner_failure')

    def test_otel_preserves_provisioning_failure_instead_of_calling_it_a_model_error(self):
        self.events()
        frame={'resourceSpans':[{'scopeSpans':[{'spans':[{'name':'run','status':{'code':2,
               'message':'creating sandbox: sandbox creation failed after 3 attempts: SECRET_SENTINEL'}}]}]}]}
        (self.root/'runs/test/run-telemetry.jsonl').write_text(json.dumps(frame)+'\n')
        value=diagnose(self.root,1)
        self.assertEqual(value['category'],'sandbox_provisioning')
        self.assertNotIn('SECRET_SENTINEL',json.dumps(value))


if __name__ == '__main__':
    unittest.main()
