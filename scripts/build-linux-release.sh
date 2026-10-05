#!/usr/bin/env bash
# Build the shipped Linux binaries against the published glibc floor.
#
# The release archives (native-publish.yml) and the rust-spike battery both
# build through this script, so the floor a release ships is the floor CI
# proves. Needs cargo-zigbuild and zig on PATH (install-cargo-zigbuild.sh).
# Binaries land in rust/target/<target>/release/ unless CARGO_TARGET_DIR is set.
set -euo pipefail

GLIBC_FLOOR="2.17"

usage() {
  echo "usage: $0 <x86_64-unknown-linux-gnu|aarch64-unknown-linux-gnu>" >&2
  exit 2
}

[[ $# -eq 1 ]] || usage
target="$1"
[[ "$target" == *-unknown-linux-gnu ]] || usage

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
(cd "$root/rust" && cargo zigbuild --release --locked -p decided -p decided-mcp --target "${target}.${GLIBC_FLOOR}")

bin_dir="${CARGO_TARGET_DIR:-$root/rust/target}/${target}/release"
status=0
for binary in decided decided-mcp; do
  newest="$(objdump -T "$bin_dir/$binary" | grep -o 'GLIBC_[0-9][0-9.]*' | sed 's/^GLIBC_//' | sort -uV | tail -n 1 || true)"
  if [[ -z "$newest" ]]; then
    echo "$binary: no versioned glibc symbols found in $bin_dir/$binary" >&2
    status=1
  elif [[ "$(printf '%s\n%s\n' "$newest" "$GLIBC_FLOOR" | sort -V | tail -n 1)" != "$GLIBC_FLOOR" ]]; then
    echo "$binary: requires GLIBC_$newest, above the GLIBC_$GLIBC_FLOOR floor" >&2
    status=1
  else
    echo "$binary: newest glibc symbol GLIBC_$newest (floor GLIBC_$GLIBC_FLOOR)"
  fi
done
exit "$status"
