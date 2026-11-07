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
import hashlib


def invoke(args, timeout=30):
    cli = os.environ.get("OPENSHELL", "openshell")
    result = subprocess.run([cli, *args], capture_output=True, text=True, timeout=timeout)
    if result.returncode:
        raise RuntimeError((result.stderr or result.stdout)[-4000:])
    return result.stdout


def gateway_name():
    return os.environ.get('GYRE_OPEN_GATEWAY', 'gyre-gyre')


def auth_path(state, name):
    return Path(state) / ('gateway-auth-'+hashlib.sha256(name.encode()).hexdigest()[:12]+'.json')


def auth_status(state, name=None):
    try:
        return json.loads(auth_path(state,name or gateway_name()).read_text())
    except (OSError,ValueError):
        return {}


def login(state, force=False, max_age=300):
    """Coalesce refreshes, retry with backoff, and retain usable cached auth."""
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
        name=gateway_name()
        status=auth_status(state,name)
        now=time.time()
        def probe():
            # whoami can describe a local identity without exercising access.
            # This bounded, read-only request proves the credential is usable.
            invoke(['-g',name,'sandbox','list','--output','json','--page-size','1'],timeout=15)
        def save(value):
            path=auth_path(state,name)
            temporary=path.with_suffix('.tmp')
            temporary.write_text(json.dumps(value));temporary.chmod(0o600);temporary.replace(path)
        if status.get('retry_at',0)>now:
            if not force:
                try:
                    probe()
                    return
                except (RuntimeError,OSError,subprocess.TimeoutExpired):
                    pass
            raise RuntimeError(f"Gateway login refresh backing off; retry in {int(status['retry_at']-now)+1}s")
        if not force and now-status.get('refreshed_at',0)<max_age:
            try:
                probe()
                return
            except (RuntimeError,OSError,subprocess.TimeoutExpired):
                pass
        try:
            invoke(['gateway','login',name])
            probe()
        except (RuntimeError,OSError,subprocess.TimeoutExpired) as exc:
            failures=status.get('failures',0)+1
            delay=min(300,5*2**min(failures-1,6))
            value=status | {'gateway':name,'failures':failures,'retry_at':time.time()+delay,
                            'error_kind':type(exc).__name__,'using_cached':False}
            if not force:
                try:
                    probe()
                    value['using_cached']=True
                    save(value)
                    return
                except (RuntimeError,OSError,subprocess.TimeoutExpired):
                    pass
            save(value)
            raise RuntimeError(f'Gateway login refresh failed ({type(exc).__name__}); retry in {delay}s') from exc
        save({'gateway':name,'refreshed_at':time.time(),'failures':0,'retry_at':0,'using_cached':False})


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
        args = ["-g", gateway_name(), "sandbox", "list", "--output", "json", "--page-size", "100"]
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
            invoke(["-g", gateway_name(), "sandbox", "delete", args.sandbox], timeout=90)
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
