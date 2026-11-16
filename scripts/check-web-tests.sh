#!/usr/bin/env bash
# Run the complete frontend suite with one worker; preserve every test.
set -euo pipefail
repo_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_dir/web"
npm ci
npm test -- --no-file-parallelism --maxWorkers=1
