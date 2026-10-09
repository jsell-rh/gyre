#!/usr/bin/env python3
"""Register and authenticate the current Gyre development gateway."""
import argparse
import os
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parent.parent
NAME = "gyre-gyre"
ENDPOINT = "https://gw-openshell-0ac2a203e0863b91.hyp2-spoke1.infra.hypershell.app:443"
ISSUER = "https://keycloak.hyp2.infra.hypershell.app/realms/hypershell"
CLIENT = "hs-sa-3KQ0zvRhGqP5mG5wrftxN9XMZPL-3KQ140Y26FktQVUKTL02YrOKRaJ"
AUDIENCE = "gyre-3KQ0zvRhGqP5mG5wrftxN9XMZPL"
SUBJECT = "6bfd42b4-ba73-4d52-995b-f4026d9fb050"


def configure(env, secret):
    cli = env.get("OPENSHELL", "openshell")

    def call(*args):
        result = subprocess.run([cli, *args], env=env, capture_output=True, text=True, timeout=60)
        output = (result.stdout + result.stderr).replace(secret, "[redacted]")
        return result.returncode, output

    add = ["gateway", "add", "--name", NAME, "--oidc-issuer", ISSUER,
           "--oidc-client-id", CLIENT, "--oidc-audience", AUDIENCE, ENDPOINT]
    code, output = call(*add)
    if code and "already exists" in output.lower():
        code, output = call("gateway", "remove", NAME)
        if code:
            raise RuntimeError(output.strip())
        code, output = call(*add)
        if code and "already exists" in output.lower():
            raise RuntimeError("OpenShell could not replace the registration. Run this command "
                               "from your terminal with write access to ~/.config/openshell.")
    if code:
        raise RuntimeError(output.strip())
    code, output = call("-g", NAME, "whoami", "--output", "json")
    if code:
        raise RuntimeError(output.strip())
    if SUBJECT not in output:
        raise RuntimeError("Gateway authenticated as an unexpected subject; check the OIDC credential.")
    print(f"Authenticated {NAME} at {ENDPOINT} as {SUBJECT}.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inference", action="store_true", help="also configure EnMaaS and run its smoke test")
    parser.add_argument("--smoke", action="store_true", help="configure EnMaaS and smoke test (alias for --inference)")
    args = parser.parse_args()
    secret = os.environ.get("OPENSHELL_OIDC_CLIENT_SECRET", "")
    if not secret:
        goal = ROOT / "OPENSHELL-GOAL.md"
        match = re.search(r"Client secret:\s*`([^`]+)`", goal.read_text()) if goal.exists() else None
        secret = match.group(1) if match else ""
    if not secret:
        raise RuntimeError("Set OPENSHELL_OIDC_CLIENT_SECRET in your environment before running this command.")
    env = {**os.environ, "OPENSHELL_OIDC_CLIENT_SECRET": secret, "OPENSHELL_NO_BROWSER": "1",
           "OPENSHELL_GATEWAY_INSECURE": "true", "OPENSHELL_WORKSPACE": "default"}
    configure(env, secret)
    if args.inference or args.smoke:
        result = subprocess.run([os.environ.get("PYTHON", "python3"), str(ROOT / "scripts/dev-inference.py"),
                                 "--github", "--smoke"], env=env, check=False)
        if result.returncode:
            raise RuntimeError("Gateway is configured; EnMaaS setup did not complete.")


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, OSError, subprocess.TimeoutExpired) as exc:
        raise SystemExit(f"dev-gateway: {exc}") from None
