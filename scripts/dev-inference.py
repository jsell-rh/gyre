#!/usr/bin/env python3
"""Configure sandbox inference from the local secret store; optionally smoke test."""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
PROVIDER = "gyre-enmaas"
MODEL = "enmaas-glm-5-3/rits/zai-org/glm-5-3"


def invoke(command, env, secret, timeout=60):
    result = subprocess.run(command, env=env, capture_output=True, text=True, timeout=timeout)
    # Provider operations must never echo the credential into controller logs.
    output = (result.stdout + result.stderr).replace(secret, "[redacted]")
    return result.returncode, output


def configure(env, secret, provider=PROVIDER, profile=None, credential="ENMAAS_API_KEY", profile_id=None):
    cli = [env.get("OPENSHELL", "openshell"), "-g", "gyre-gyre"]
    profile = str(profile or ROOT / "docker/dev-worker/enmaas.yaml")
    profile_id = profile_id or provider
    code, output = invoke([*cli, "profile", "import", "--file", profile], env, secret)
    if code and already_exists(output):
        code, exported = invoke([*cli, "profile", "export", profile_id, "--output", "yaml"], env, secret)
        if code:
            raise RuntimeError(exported.strip())
        version = re.search(r"(?m)^\s*resource_version:\s*['\"]?([1-9][0-9]*)['\"]?\s*$", exported)
        if not version:
            raise RuntimeError("Exported provider profile has no non-zero resource_version; update aborted.")
        with tempfile.TemporaryDirectory(prefix="gyre-provider-") as directory:
            updated = Path(directory) / "enmaas.yaml"
            updated.write_text(Path(profile).read_text() + f"\nresource_version: {version.group(1)}\n")
            code, output = invoke([*cli, "profile", "update", profile_id, "--file", str(updated)], env, secret)
    if code:
        raise RuntimeError(output.strip() or "gateway profile configuration failed")
    code, output = invoke([*cli, "provider", "create", "--name", provider, "--type", profile_id,
                           "--credential", credential], env, secret)
    if code and already_exists(output):
        code, output = invoke([*cli, "provider", "update", provider, "--credential", credential], env, secret)
    if code:
        raise RuntimeError(output.strip() or "gateway credential configuration failed")
    print(f"Configured {provider} (Bearer authentication).")


def configure_github(env):
    secret = env.get("GITHUB_TOKEN") or env.get("GH_TOKEN")
    if not secret:
        result = subprocess.run(["gh", "auth", "token"], capture_output=True, text=True, timeout=15)
        if result.returncode or not result.stdout.strip():
            raise RuntimeError("Cannot read GitHub credential; run gh auth login or set GITHUB_TOKEN.")
        secret = result.stdout.strip()
    configure({**env, "GITHUB_TOKEN": secret}, secret, provider="gyre-github-rw",
              profile=ROOT / "docker/dev-worker/github.yaml", credential="GITHUB_TOKEN",
              profile_id="gyre-github-custom")


def already_exists(output):
    return "already exists" in output.lower() or "alreadyexists" in output.lower()


def smoke(env, secret):
    # Match the worker's OMP config without writing to the user's home directory.
    with tempfile.TemporaryDirectory(prefix="gyre-inference-") as directory:
        config = Path(directory) / ".omp/agent"
        config.mkdir(parents=True)
        for name in ("models.yml", "config.yml"):
            shutil.copyfile(ROOT / "docker/dev-worker" / name, config / name)
        code, output = invoke(
            [env.get("OMP", "omp"), "--model", MODEL, "--no-session", "--no-tools",
             "--no-extensions", "--no-skills", "--no-rules", "--mode=json", "-p", "Reply with exactly: OK"],
            {**env, "HOME": directory, "PI_CODING_AGENT_DIR": str(config)}, secret, timeout=120)
        message = None
        for line in output.splitlines():
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(event, dict) and event.get("type") == "message_end" and event.get("message", {}).get("role") == "assistant":
                message = event["message"]
        response = "".join(part.get("text", "") for part in (message or {}).get("content", [])
                           if part.get("type") == "text").strip()
        if code or not message or message.get("stopReason") in ("error", "aborted") or response != "OK":
            diagnostic = (message or {}).get("errorMessage") or response or output[-2000:].strip()
            raise RuntimeError(f"Pinned model smoke test failed: {diagnostic}")
        print("Pinned GLM 5.3 smoke test: OK")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--smoke", action="store_true", help="also test the pinned model with OMP")
    parser.add_argument("--smoke-only", action="store_true", help="test inference without changing gateway state")
    parser.add_argument("--github", action="store_true", help="also configure GitHub clone/fetch/push credentials")
    parser.add_argument("--github-only", action="store_true", help="configure only the GitHub provider")
    args = parser.parse_args()
    gateway_env = {**os.environ, "OPENSHELL_WORKSPACE": "default",
                   "OPENSHELL_GATEWAY_INSECURE": "true", "OPENSHELL_NO_BROWSER": "1"}
    if args.github or args.github_only:
        configure_github(gateway_env)
    if args.github_only:
        return
    result = subprocess.run(["secret-tool", "lookup", "service", "pricetag", "key", "api-token"],
                            capture_output=True, text=True, timeout=15)
    secret = result.stdout.strip()
    if result.returncode or not secret:
        raise RuntimeError("Cannot read secret-tool entry service=pricetag key=api-token; "
                           "run this command in your logged-in desktop session with access to the secret store.")
    env = {**gateway_env, "ENMAAS_API_KEY": secret}
    if not args.smoke_only:
        configure(env, secret)
    if args.smoke or args.smoke_only:
        smoke(env, secret)


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, OSError, subprocess.TimeoutExpired) as exc:
        raise SystemExit(f"dev-inference: {exc}") from None
