#!/usr/bin/env python3
"""Certify RAC engines against asdecided-spec's output-parity conformance tier.

ADR-063 Guard 2: a cross-language conformance fixture suite proves output
parity. The neutral suite lives in `itsthelore/asdecided-spec`
(`conformance/output-parity.json` + `conformance/vectors/`): each case pins the
byte-for-byte stdout and exit code an engine must produce for a deterministic,
recency-free command over an example corpus. This runner replays every case
against one or more engines and asserts each reproduces the golden bytes exactly
— certifying the engine against the specification, not against another engine.

Point DECIDED_SPEC_DIR (or --spec-dir) at a asdecided-spec checkout. Give one or more
engines as `label=command` pairs (command is shell-split, run from the asdecided-spec
root). With no engines and no DECIDED_SPEC_DIR, the runner skips with exit 0 so CI
stays green until asdecided-spec is wired in.

Usage:
    DECIDED_SPEC_DIR=/path/to/asdecided-spec python rust/tools/conformance_certify.py \
        --engine "python=/path/.venv/bin/rac" \
        --engine "rust=/path/rust/target/release/decided"

Exit 0 = every engine reproduces every case (or skipped); 1 = a mismatch;
2 = setup error.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import shlex
import subprocess
import sys
from pathlib import Path


def run_case(cmd: list[str], argv: list[str], cwd: Path) -> tuple[int, bytes]:
    env = dict(os.environ)
    env.update({"DECIDED_NO_CACHE": "1", "LC_ALL": "C", "TZ": "UTC", "COLUMNS": "80"})
    p = subprocess.run(cmd + argv, cwd=cwd, capture_output=True, env=env)
    return p.returncode, p.stdout


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--spec-dir", default=os.environ.get("DECIDED_SPEC_DIR"))
    ap.add_argument(
        "--engine",
        action="append",
        default=[],
        metavar="LABEL=CMD",
        help="engine to certify; repeatable",
    )
    args = ap.parse_args()

    if not args.spec_dir:
        print("SKIP: no --spec-dir / DECIDED_SPEC_DIR — asdecided-spec not available")
        return 0
    spec_dir = Path(args.spec_dir)
    manifest_path = spec_dir / "conformance/output-parity.json"
    if not manifest_path.is_file():
        print(f"SETUP ERROR: {manifest_path} not found", file=sys.stderr)
        return 2
    if not args.engine:
        print("SETUP ERROR: give at least one --engine LABEL=CMD", file=sys.stderr)
        return 2

    import json

    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    cases = manifest["cases"]
    engines = []
    for spec in args.engine:
        if "=" not in spec:
            print(f"SETUP ERROR: --engine wants LABEL=CMD, got {spec!r}", file=sys.stderr)
            return 2
        label, cmd = spec.split("=", 1)
        engines.append((label, shlex.split(cmd)))

    failures = 0
    for label, cmd in engines:
        passed = 0
        for case in cases:
            golden = (spec_dir / case["stdout"]).read_bytes()
            assert hashlib.sha256(golden).hexdigest() == case["stdout_sha256"], (
                f"golden {case['stdout']} does not match its recorded sha256"
            )
            exit_code, out = run_case(cmd, case["argv"], spec_dir)
            ok = exit_code == case["expect_exit"] and out == golden
            if ok:
                passed += 1
            else:
                failures += 1
                why = []
                if exit_code != case["expect_exit"]:
                    why.append(f"exit {exit_code}!={case['expect_exit']}")
                if out != golden:
                    why.append("stdout != golden")
                print(f"  FAIL [{label}] {case['id']}: {', '.join(why)}", file=sys.stderr)
        print(f"[{label}] {passed}/{len(cases)} output-parity cases reproduced")

    if failures:
        print(f"conformance: {failures} mismatch(es)", file=sys.stderr)
        return 1
    print(f"conformance: all engines reproduce all {len(cases)} cases")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
