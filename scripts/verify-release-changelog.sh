#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage: $0 <vX.Y.Z|X.Y.Z> [CHANGELOG.md]" >&2
  exit 2
}

[[ $# -ge 1 && $# -le 2 ]] || usage

requested="${1#refs/tags/}"
requested="${requested#v}"
changelog="${2:-CHANGELOG.md}"

semver_component='(0|[1-9][0-9]*)'
if [[ ! "$requested" =~ ^${semver_component}\.${semver_component}\.${semver_component}$ ]]; then
  echo "release version must be SemVer vX.Y.Z or X.Y.Z, got: $1" >&2
  exit 1
fi

if [[ ! -r "$changelog" ]]; then
  echo "changelog is not readable: $changelog" >&2
  exit 1
fi

heading="## v${requested}"
entry_count="$({
  awk -v heading="$heading" '
    index($0, heading) == 1 {
      suffix = substr($0, length(heading) + 1)
      if (suffix == "" || suffix ~ /^[[:space:]]/) count++
    }
    END { print count + 0 }
  ' "$changelog"
})"

if [[ "$entry_count" != "1" ]]; then
  echo "$changelog must contain exactly one '$heading' entry; found $entry_count" >&2
  exit 1
fi

duplicates="$({
  awk '
    /^## v[0-9]+\.[0-9]+\.[0-9]+([[:space:]]|$)/ { count[$2]++ }
    END { for (version in count) if (count[version] > 1) print version }
  ' "$changelog" | sort
})"

if [[ -n "$duplicates" ]]; then
  echo "duplicate release headings in $changelog:" >&2
  echo "$duplicates" >&2
  exit 1
fi

echo "changelog entry verified: v${requested}"
