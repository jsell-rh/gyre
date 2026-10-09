#!/usr/bin/env python3
"""One bounded gateway operation; outputs an atomic result for the reconciler."""
import argparse
import fcntl
import json
import os
from pathlib import Path
import subprocess
import time
import re


def invoke(args, timeout=30):
    cli = os.environ.get("OPENSHELL", "openshell")
    result = subprocess.run([cli, *args], capture_output=True, text=True, timeout=timeout)
    if result.returncode:
        raise RuntimeError((result.stderr or result.stdout)[-4000:])
    return result.stdout


def login(state):
    with (state / "gateway-login.lock").open("a") as lock:
        deadline = time.monotonic() + 30
        while True:
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except BlockingIOError:
                if time.monotonic() >= deadline:
                    raise TimeoutError("gateway authentication lock timed out")
                time.sleep(0.1)
        invoke(["gateway", "login", "gyre-gyre"])


def decode_page(value):
    if isinstance(value, list):
        items, token = value, ""
    elif isinstance(value, dict):
        items = value.get("sandboxes", value.get("items"))
        token = value.get("next_page_token", value.get("nextPageToken", ""))
        if items is None:
            raise ValueError("unrecognized sandbox inventory schema")
    else:
        raise ValueError("unrecognized sandbox inventory schema")
    if not isinstance(items, list) or not isinstance(token, str):
        raise ValueError("invalid sandbox inventory page")
    if len(items) >= 100 and not token:
        raise ValueError("full inventory page has no continuation token; refusing partial accounting")
    result = []
    for item in items:
        if not isinstance(item, dict):
            raise ValueError("invalid sandbox inventory object")
        item = item.get("sandbox", item)
        metadata = item.get("metadata", {})
        name = item.get("name", metadata.get("name"))
        labels = item.get("labels", metadata.get("labels", {}))
        if not isinstance(name, str) or not name or not isinstance(labels, dict):
            raise ValueError("invalid sandbox inventory object")
        status = item.get("status", {})
        phase = item.get("phase", status.get("phase", "Unknown") if isinstance(status, dict) else str(status))
        result.append({"name": name, "labels": labels, "phase": phase})
    return result, token


def inventory():
    items, token, seen = [], "", set()
    deadline = time.monotonic() + 90
    for _ in range(100):
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("sandbox inventory exceeded its overall deadline")
        args = ["-g", "gyre-gyre", "sandbox", "list", "--output", "json", "--page-size", "100"]
        if token:
            args += ["--page-token", token]
        output = invoke(args, timeout=min(30, remaining))
        # OpenShell emits TLS diagnostics to stdout even with --output json.
        # Ignore only the prefix; malformed/truncated JSON still fails closed.
        start = re.search(r'(?m)^[ \t]*(?=[\[{])', output)
        if not start:
            raise ValueError('sandbox inventory did not contain a JSON document')
        page, following = decode_page(json.loads(output[start.start():]))
        items.extend(page)
        if not following:
            return items
        if following in seen:
            raise ValueError("sandbox inventory pagination repeated a token")
        seen.add(following)
        token = following
    raise ValueError("sandbox inventory exceeds bounded pagination limit")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("kind", choices=("inventory", "delete"))
    parser.add_argument("--result", required=True, type=Path)
    parser.add_argument("--sandbox")
    args = parser.parse_args()
    os.environ.update(OPENSHELL_NO_BROWSER="1", OPENSHELL_GATEWAY_INSECURE="true", OPENSHELL_WORKSPACE="default")
    state = Path(os.environ["GYRE_DEV_STATE"])
    try:
        login(state)
        if args.kind == "inventory":
            result = {"ok": True, "items": inventory()}
        else:
            if not args.sandbox:
                parser.error("delete requires --sandbox")
            invoke(["-g", "gyre-gyre", "sandbox", "delete", args.sandbox], timeout=90)
            result = {"ok": True}
    except (OSError, RuntimeError, ValueError, subprocess.TimeoutExpired) as exc:
        result = {"ok": False, "error": str(exc)}
    # Credentials are never arguments or result data, even if the CLI repeats
    # an injected value in an error message.
    encoded = json.dumps(result)
    for key in ("OPENSHELL_OIDC_CLIENT_SECRET", "GITHUB_TOKEN", "GH_TOKEN", "ENMAAS_API_KEY"):
        if os.environ.get(key):
            encoded = encoded.replace(os.environ[key], "[redacted]")
    args.result.parent.mkdir(parents=True, exist_ok=True)
    temporary = args.result.with_suffix(".tmp")
    temporary.write_text(encoded)
    temporary.chmod(0o600)
    os.replace(temporary, args.result)


if __name__ == "__main__":
    main()
