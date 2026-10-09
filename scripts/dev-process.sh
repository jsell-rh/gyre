#!/usr/bin/env bash
# Record a child result atomically so a restarted controller can adopt it.
set -uo pipefail
result=$1
shift
# Publish identity before any child can allocate cloud resources. PID alone is
# unsafe after restart because the kernel can reuse it.
python3 - "$result.process.json" "$$" <<'PY'
import json, os, pathlib, sys
path = pathlib.Path(sys.argv[1])
pid = int(sys.argv[2])
stat = pathlib.Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()
identity = {'pid': pid, 'start': stat[19],
            'boot': pathlib.Path('/proc/sys/kernel/random/boot_id').read_text().strip()}
temporary = path.with_suffix('.tmp')
temporary.write_text(json.dumps(identity))
os.replace(temporary, path)
PY
if [ "$?" -ne 0 ]; then exit 255; fi
"$@"
rc=$?
printf '%s\n' "$rc" > "$result.tmp"
mv "$result.tmp" "$result"
exit "$rc"
