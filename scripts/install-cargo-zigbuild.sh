#!/usr/bin/env bash
# Install the pinned zig and cargo-zigbuild pair into a private virtualenv and
# put it on PATH for later GitHub Actions steps.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
venv="${1:-${RUNNER_TEMP:-/tmp}/cargo-zigbuild}"

python3 -m venv "$venv"
"$venv/bin/pip" install --quiet --disable-pip-version-check \
  --only-binary :all: --require-hashes \
  -r "$root/scripts/cargo-zigbuild-requirements.txt"

if [[ -n "${GITHUB_PATH:-}" ]]; then
  echo "$venv/bin" >> "$GITHUB_PATH"
fi
echo "$venv/bin"
