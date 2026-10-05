#!/usr/bin/env bash
# Run the Linux release binaries on a glibc 2.17 userland (CentOS 7) against
# this repository's corpus. Needs Docker; the image is pinned by digest and
# matches the runner's architecture.
set -euo pipefail

FLOOR_IMAGE="centos:7@sha256:be65f488b7764ad3638f236b7b515b3678369a5124c47b8d32916d6487418ea4"

[[ $# -eq 1 ]] || { echo "usage: $0 <directory containing decided and decided-mcp>" >&2; exit 2; }

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bin_dir="$(cd "$1" && pwd)"

docker run --rm -i \
  -v "$root:/work:ro" -v "$bin_dir:/opt/asdecided:ro" -w /work \
  "$FLOOR_IMAGE" /bin/bash -euo pipefail -s <<'SMOKE'
ldd --version 2>&1 | sed -n 1p
/opt/asdecided/decided --version
/opt/asdecided/decided validate decisions/ > /dev/null
/opt/asdecided/decided relationships decisions/ --validate > /dev/null
/opt/asdecided/decided gate decisions/ > /dev/null
response="$(printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"floor-smoke","version":"1.0.0"}}}' \
  | /opt/asdecided/decided-mcp --root decisions)"
case "$response" in
  *'"serverInfo"'*) echo "decided-mcp: initialize ok" ;;
  *) echo "decided-mcp: unexpected initialize response: $response" >&2; exit 1 ;;
esac
SMOKE
