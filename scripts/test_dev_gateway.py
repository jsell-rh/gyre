"""Gateway registration replacement and credential privacy checks."""
import importlib.util
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("dev_gateway", Path(__file__).with_name("dev-gateway.py"))
gateway = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(gateway)


class GatewayTest(unittest.TestCase):
    def test_replaces_registration_and_verifies_new_identity(self):
        calls = []
        secret = "test-only-client-secret"

        def run(command, **kwargs):
            calls.append(command)
            self.assertNotIn(secret, " ".join(command))
            self.assertEqual(kwargs["env"]["OPENSHELL_OIDC_CLIENT_SECRET"], secret)
            if len(calls) == 1:
                return subprocess.CompletedProcess(command, 1, "", "Gateway already exists")
            if "whoami" in command:
                return subprocess.CompletedProcess(command, 0, '{"sub":"'+gateway.SUBJECT+'"}', "")
            return subprocess.CompletedProcess(command, 0, "", "")

        with patch.object(gateway.subprocess, "run", side_effect=run):
            gateway.configure({"OPENSHELL_OIDC_CLIENT_SECRET": secret}, secret)
        self.assertEqual(calls[1], ["openshell", "gateway", "remove", "gyre-gyre"])
        self.assertIn(gateway.ENDPOINT, calls[2])
        self.assertIn(gateway.CLIENT, calls[2])
        self.assertIn(gateway.AUDIENCE, calls[2])
        self.assertIn("whoami", calls[3])

    def test_false_success_removal_reports_permission_problem(self):
        replies = [subprocess.CompletedProcess([], 1, "", "Gateway already exists"),
                   subprocess.CompletedProcess([], 0, "removed", ""),
                   subprocess.CompletedProcess([], 1, "", "Gateway already exists")]
        with patch.object(gateway.subprocess, "run", side_effect=replies):
            with self.assertRaisesRegex(RuntimeError, "write access"):
                gateway.configure({}, "test-only-client-secret")


if __name__ == "__main__":
    unittest.main()
