#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT="${1:-$ROOT_DIR/THIRD-PARTY-NOTICES}"
METADATA="$(mktemp)"
trap 'rm -f "$METADATA"' EXIT

cargo metadata \
  --manifest-path "$ROOT_DIR/rust/Cargo.toml" \
  --locked \
  --format-version 1 > "$METADATA"

mkdir -p "$(dirname "$OUTPUT")"
{
  cat <<'HEADER'
AsDecided third-party notices
============================

The entries below are generated from the locked Cargo dependency graph for
this release. The accompanying CycloneDX SBOM carries the same inventory in
machine-readable form. This project does not vendor third-party source; the
listed licenses and notices remain available from each upstream package.

Dependencies
------------
HEADER
  jq -r '
    .packages[]
    | select(.source != null)
    | [
        .name,
        .version,
        (.license // (if .license_file then "license file: " + .license_file else "license metadata unavailable" end)),
        (.repository // "repository not recorded")
      ]
    | @tsv
  ' "$METADATA" \
    | sort -f \
    | awk -F '\t' '{printf "- %s %s — %s — %s\n", $1, $2, $3, $4}'
} > "$OUTPUT"
