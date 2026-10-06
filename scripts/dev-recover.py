#!/usr/bin/env python3
"""Save an unpushed sandbox worktree as a local patch before deletion."""
import base64
import os
from pathlib import Path
import re
import subprocess
import sys
import time


def main() -> int:
    sandbox, destination = sys.argv[1:]
    command = (
        "git add -A && printf 'GYRE_RECOVERY_BEGIN\\n' && "
        "git diff --binary --cached origin/main | base64 -w0 && "
        "printf '\\nGYRE_RECOVERY_END\\n'"
    )
    for attempt in range(3):
        try:
            result = subprocess.run(
                [os.environ.get("OPENSHELL", "openshell"), "-g", "gyre-gyre", "sandbox",
                 "exec", "-n", sandbox, "--no-login-shell", "--workdir", "/tmp/gyre",
                 "--", "bash", "-c", command],
                capture_output=True, timeout=120,
            )
        except (OSError, subprocess.TimeoutExpired):
            result = None
        if result and result.returncode == 0:
            match = re.search(rb"(?m)^GYRE_RECOVERY_BEGIN\r?\n([A-Za-z0-9+/=\r\n]*)^GYRE_RECOVERY_END\r?$",
                              result.stdout)
            if match:
                patch = base64.b64decode(re.sub(rb"\s", b"", match.group(1)), validate=True)
                if patch:
                    path = Path(destination)
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(patch)
                    print(f"saved unpushed work: {path} ({len(patch)} bytes)")
                return 0
        time.sleep((attempt + 1) * 3)
    print("could not save unpushed work before sandbox deletion", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
