"""Credential routing and pinned-model configuration checks; no external inference."""
import importlib.util
import json
import io
import urllib.error
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("dev_inference", Path(__file__).with_name("dev-inference.py"))
inference = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(inference)


class InferenceTest(unittest.TestCase):
    def test_github_provider_uses_gh_credential_without_argv_or_disk_secret(self):
        secret = "test-only-github-token"
        calls = []

        def run(command, **kwargs):
            calls.append(command)
            if command == ["gh", "auth", "token"]:
                return subprocess.CompletedProcess(command, 0, secret + "\n", "")
            self.assertNotIn(secret, " ".join(command))
            self.assertEqual(kwargs["env"]["GITHUB_TOKEN"], secret)
            if "import" in command:
                self.assertNotIn(secret, Path(command[-1]).read_text())
            return subprocess.CompletedProcess(command, 0, "", "")

        with patch.object(inference.subprocess, "run", side_effect=run):
            inference.configure_github({})
        self.assertIn("gyre-github-rw", calls[-1])
        self.assertIn("gyre-github-custom", calls[-1])
        self.assertEqual(calls[-1][-2:], ["--credential", "GITHUB_TOKEN"])

    def test_existing_profile_and_provider_are_updated_with_environment_credential(self):
        secret = "test-only-api-key"
        calls = []

        def run(command, **kwargs):
            calls.append(command)
            self.assertNotIn(secret, " ".join(command))
            self.assertEqual(kwargs["env"]["ENMAAS_API_KEY"], secret)
            create = "import" in command or "create" in command
            if "export" in command:
                return subprocess.CompletedProcess(command, 0, "id: gyre-enmaas\nresource_version: 7\n", "")
            if "profile" in command and "update" in command:
                self.assertIn("resource_version: 7", Path(command[-1]).read_text())
            return subprocess.CompletedProcess(command, 1 if create else 0, "AlreadyExists" if create else "", "")

        with patch.object(inference.subprocess, "run", side_effect=run):
            inference.configure({"ENMAAS_API_KEY": secret}, secret)
        self.assertEqual(len(calls), 5)
        self.assertEqual(calls[-1][-4:], ["update", "gyre-enmaas", "--credential", "ENMAAS_API_KEY"])

    def test_gateway_errors_redact_secret_and_do_not_attempt_update(self):
        with patch.object(inference.subprocess, "run", return_value=
                          subprocess.CompletedProcess([], 1, "", "gateway unavailable test-only-key")) as run:
            with self.assertRaisesRegex(RuntimeError, r"gateway unavailable \[redacted\]"):
                inference.configure({"ENMAAS_API_KEY": "test-only-key"}, "test-only-key")
        run.assert_called_once()

    def test_smoke_accepts_only_a_complete_pinned_response_without_runtime_or_catalog(self):
        for answer, stop, succeeds in (("OK", "stop", True), ("Wrong", "stop", False),
                                       ("OK", "length", False), ("OK", None, False)):
            with self.subTest(answer=answer, stop=stop):
                payload = {'choices': [{'finish_reason': stop,
                           'message': {'role': 'assistant', 'content': answer}}]}
                with patch.object(inference.urllib.request, 'urlopen',
                                  return_value=io.BytesIO(json.dumps(payload).encode())) as opened:
                    if succeeds:
                        inference.smoke({}, 'test-only-key')
                    else:
                        with self.assertRaisesRegex(RuntimeError, 'smoke test failed'):
                            inference.smoke({}, 'test-only-key')
                request = opened.call_args.args[0]
                self.assertEqual(request.full_url, 'https://api.enmaas.devshift.net/v1/chat/completions')
                self.assertEqual(json.loads(request.data)['model'], 'rits/zai-org/glm-5-3')
                self.assertNotIn('test-only-key', request.data.decode())
                self.assertEqual(request.get_header('Authorization'), 'Bearer test-only-key')

    def test_smoke_rejects_http_errors_and_missing_choices(self):
        failure = urllib.error.HTTPError('https://api.enmaas.devshift.net', 429, 'limited', {}, None)
        with patch.object(inference.urllib.request, 'urlopen', side_effect=failure):
            with self.assertRaisesRegex(RuntimeError, 'smoke test failed: HTTP 429'):
                inference.smoke({}, 'test-only-key')
        for payload in (b'{"choices":[]}', b'not-json',
                        b'{"choices":[{"message":{"role":"assistant","content":null}}]}'):
            with patch.object(inference.urllib.request, 'urlopen', return_value=io.BytesIO(payload)):
                with self.assertRaisesRegex(RuntimeError, 'smoke test failed'):
                    inference.smoke({}, 'test-only-key')

    def test_missing_export_version_prevents_profile_update(self):
        with patch.object(inference, "invoke", side_effect=[(1, "AlreadyExists"), (0, "id: gyre-enmaas\n")]) as invoke:
            with self.assertRaisesRegex(RuntimeError, "resource_version"):
                inference.configure({}, "test-only-key")
        self.assertEqual(invoke.call_count, 2)


if __name__ == "__main__":
    unittest.main()
