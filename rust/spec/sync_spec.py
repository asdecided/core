#!/usr/bin/env python3
"""Cross-repo sync gate for the shared artifact-spec registry (ADR-063 Guard 1).

The registry's source of truth is the upstream `itsthelore/asdecided-spec`
(`schema/artifact-specs.json`); rac-core vendors it into
`rust/rac-engine/assets/spec/artifact-specs.json`, which the native engine embeds.
This gate proves the vendored copy has not drifted from the upstream: it
compares the `artifact_specs` and `relationship_descriptions` payloads (the keys
the engines consume) byte-for-parsed-value. The `_meta` block is provenance and
differs by design between the upstream (source) and the vendored (copy) roles,
so it is not compared.

The upstream location is given by the DECIDED_SPEC_DIR environment variable (a path
to a asdecided-spec checkout). When it is unset, the gate skips with exit 0 — until
asdecided-spec is wired into CI there is nothing to compare against, and the in-repo
native contract tests still prove the embedded registry is valid.

Usage:
    DECIDED_SPEC_DIR=/path/to/asdecided-spec python rust/spec/sync_spec.py
Exit 0 = in sync (or skipped); 1 = drift; 2 = setup error.
"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
VENDORED = REPO / "rust/rac-engine/assets/spec/artifact-specs.json"
PAYLOAD_KEYS = ("artifact_specs", "relationship_descriptions")


def _payload(path: Path) -> dict:
    data = json.loads(path.read_text(encoding="utf-8"))
    return {k: data.get(k) for k in PAYLOAD_KEYS}


def main() -> int:
    spec_dir = os.environ.get("DECIDED_SPEC_DIR")
    if not spec_dir:
        print("SKIP: DECIDED_SPEC_DIR unset — no upstream to compare")
        return 0
    upstream = Path(spec_dir) / "schema/artifact-specs.json"
    if not upstream.is_file():
        print(f"SETUP ERROR: {upstream} not found", file=sys.stderr)
        return 2

    vend = _payload(VENDORED)
    up = _payload(upstream)
    if vend != up:
        print(
            "FAIL: vendored rust/rac-engine/assets/spec/artifact-specs.json has drifted from "
            f"upstream {upstream}",
            file=sys.stderr,
        )
        for key in PAYLOAD_KEYS:
            if vend.get(key) != up.get(key):
                print(f"  - {key} differs", file=sys.stderr)
        return 1

    n_specs = len(vend["artifact_specs"] or [])
    n_rel = len(vend["relationship_descriptions"] or {})
    print(
        f"OK: vendored registry in sync with upstream ({n_specs} specs, "
        f"{n_rel} relationship descriptions)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
