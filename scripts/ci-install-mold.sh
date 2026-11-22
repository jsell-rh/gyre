#!/usr/bin/env bash
# Avoid distro-mirror outages and pin the linker used by hosted CI.
set -euo pipefail
[[ $(uname -m) == x86_64 ]] || { echo 'Unsupported CI linker architecture' >&2; exit 1; }
mold_version=2.40.4
mold_sha256=4c999e19ffa31afa5aa429c679b665d5e2ca5a6b6832ad4b79668e8dcf3d8ec1
mold_temp=$(mktemp -d)
trap 'rm -rf "$mold_temp"' EXIT
curl --fail --location --silent --show-error --retry 5 --retry-all-errors \
  --connect-timeout 15 --max-time 180 \
  "https://github.com/rui314/mold/releases/download/v${mold_version}/mold-${mold_version}-x86_64-linux.tar.gz" \
  --output "$mold_temp/mold.tar.gz"
printf '%s  %s\n' "$mold_sha256" "$mold_temp/mold.tar.gz" | sha256sum --check --status
tar -xzf "$mold_temp/mold.tar.gz" -C "$mold_temp"
mold_bin_dir="${RUNNER_TEMP:?}/gyre-mold/bin"
mkdir -p "$mold_bin_dir"
install -m 0755 "$mold_temp/mold-${mold_version}-x86_64-linux/bin/mold" "$mold_bin_dir/mold"
ln -sfn mold "$mold_bin_dir/ld.mold"
printf '%s\n' "$mold_bin_dir" >> "${GITHUB_PATH:?}"
"$mold_bin_dir/mold" --version
