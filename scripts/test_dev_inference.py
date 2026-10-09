"""Credential routing and pinned-model configuration checks; no external inference."""
import importlib.util
import json
import os
from pathlib import Path
import select
import shutil
import subprocess
import tempfile
import time
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

    def test_smoke_accepts_only_successful_final_answer_and_ignores_status(self):
        for answer, stop, code, succeeds in (("OK", "stop", 0, True),
                                            ("Wrong", "stop", 0, False),
                                            ("OK", "error", 0, False),
                                            ("OK", "stop", 1, False)):
            with self.subTest(answer=answer, stop=stop, code=code):
                event = {"type": "message_end", "message": {"role": "assistant", "stopReason": stop,
                         "content": [{"type": "text", "text": answer}]}}
                output = json.dumps(event) + "\nWorking...\n"
                with patch.object(inference, "invoke", return_value=(code, output)) as invoke:
                    if succeeds:
                        inference.smoke({"ENMAAS_API_KEY": "test-only-key"}, "test-only-key")
                    else:
                        with self.assertRaisesRegex(RuntimeError, "smoke test failed"):
                            inference.smoke({"ENMAAS_API_KEY": "test-only-key"}, "test-only-key")
                self.assertIn("--mode=json", invoke.call_args.args[0])

    def test_missing_export_version_prevents_profile_update(self):
        with patch.object(inference, "invoke", side_effect=[(1, "AlreadyExists"), (0, "id: gyre-enmaas\n")]) as invoke:
            with self.assertRaisesRegex(RuntimeError, "resource_version"):
                inference.configure({}, "test-only-key")
        self.assertEqual(invoke.call_count, 2)

    @unittest.skipUnless(shutil.which("omp"), "OMP is needed to validate its native model registry")
    def test_omp_loads_pinned_provider_without_inference_or_model_discovery(self):
        with tempfile.TemporaryDirectory(prefix="gyre-model-test-") as directory:
            config = Path(directory) / ".omp/agent"
            config.mkdir(parents=True)
            for name in ("models.yml", "config.yml"):
                shutil.copyfile(inference.ROOT / "docker/dev-worker" / name, config / name)
            process = subprocess.Popen(
                ["omp", "--mode", "rpc", "--model", inference.MODEL, "--no-session",
                 "--no-extensions", "--no-skills", "--no-rules"],
                env={**os.environ, "HOME": directory, "PI_CODING_AGENT_DIR": str(config),
                     "ENMAAS_API_KEY": "test-only-api-key"},
                text=True, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            try:
                process.stdin.write('{"type":"get_state"}\n')
                process.stdin.flush()
                state = None
                deadline = time.monotonic() + 20
                while time.monotonic() < deadline:
                    if not select.select([process.stdout], [], [], max(0, deadline-time.monotonic()))[0]:
                        break
                    line = process.stdout.readline()
                    if not line:
                        break
                    reply = json.loads(line)
                    if reply.get("command") == "get_state":
                        state = reply["data"]
                        break
                self.assertIsNotNone(state, "OMP did not return its selected model")
            finally:
                process.terminate()
                process.communicate(timeout=5)
            model = state["model"]
            self.assertEqual(model["id"], "rits/zai-org/glm-5-3")
            self.assertEqual(model["baseUrl"], "https://api.enmaas.devshift.net/v1")
            self.assertEqual(model["api"], "openai-completions")
            self.assertEqual((model["contextWindow"], model["maxTokens"]), (262144, 65536))
            self.assertFalse(model["compat"]["supportsDeveloperRole"])
            self.assertEqual(state["model"]["provider"], "enmaas-glm-5-3")
            self.assertEqual(state["model"]["id"], "rits/zai-org/glm-5-3")


if __name__ == "__main__":
    unittest.main()
