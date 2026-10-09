#!/usr/bin/env python3
"""Save sandbox branch/worktree and stash patches locally before deletion."""
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
        "set -euo pipefail; git add -A; printf 'GYRE_RECOVERY_BEGIN\\n'; "
        "git diff --binary --cached origin/main | base64 -w0 && "
        "printf '\\nGYRE_RECOVERY_END\\n'; "
        "stashes=$(git stash list --format=%H); "
        "count=0; for sha in $stashes; do count=$((count + 1)); done; "
        "printf 'GYRE_STASH_COUNT %s\\n' \"$count\"; "
        "for sha in $stashes; do "
        "printf 'GYRE_STASH_RECOVERY_BEGIN %s\\n' \"$sha\"; "
        "git stash show --include-untracked --binary \"$sha\" | base64 -w0; "
        "printf '\\nGYRE_STASH_RECOVERY_END\\n'; done; "
        "printf 'GYRE_RECOVERY_COMPLETE\\n'"
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
            count = re.search(rb"(?m)^GYRE_STASH_COUNT ([0-9]+)\r?$", result.stdout)
            stashes = re.findall(
                rb"(?m)^GYRE_STASH_RECOVERY_BEGIN ([0-9a-f]{40,64})\r?\n"
                rb"([A-Za-z0-9+/=\r\n]*)^GYRE_STASH_RECOVERY_END\r?$", result.stdout)
            complete = re.search(rb"(?m)^GYRE_RECOVERY_COMPLETE\r?$", result.stdout)
            if match and count and complete and len(stashes) == int(count.group(1)):
                try:
                    patch = base64.b64decode(re.sub(rb"\s", b"", match.group(1)), validate=True)
                    stash_patches = [(sha.decode(), base64.b64decode(re.sub(rb"\s", b"", data), validate=True))
                                     for sha, data in stashes]
                except ValueError:
                    time.sleep((attempt + 1) * 3)
                    continue
                path = Path(destination)
                path.parent.mkdir(parents=True, exist_ok=True)
                if patch:
                    path.write_bytes(patch)
                    path.chmod(0o600)
                    print(f"saved sandbox branch/worktree diff: {path} ({len(patch)} bytes)")
                if stash_patches:
                    directory = path.with_suffix('.stashes')
                    directory.mkdir(exist_ok=True)
                    directory.chmod(0o700)
                    for sha, data in stash_patches:
                        archive = directory / f'{sha}.patch'
                        archive.write_bytes(data)
                        archive.chmod(0o600)
                        print(f"saved sandbox stash: {archive} ({len(data)} bytes)")
                return 0
        time.sleep((attempt + 1) * 3)
    print("could not save unpushed work before sandbox deletion", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
