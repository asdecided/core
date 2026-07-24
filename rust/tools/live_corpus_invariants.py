#!/usr/bin/env python3
"""Exercise the live RAC corpus without pinning its output bytes.

Compatibility bytes belong to asdecided-spec. The repository's evolving corpus is
instead required to remain valid, deterministic, fresh, and cache-neutral.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def invoke(engine: Path, argv: list[str], env: dict[str, str]) -> bytes:
    process = subprocess.run(
        [str(engine), *argv],
        capture_output=True,
        env=env,
        check=False,
    )
    if process.returncode != 0:
        raise RuntimeError(
            f"{' '.join(argv)} exited {process.returncode}\n"
            f"stdout:\n{process.stdout.decode(errors='replace')}\n"
            f"stderr:\n{process.stderr.decode(errors='replace')}"
        )
    return process.stdout


def json_value(raw: bytes, label: str) -> object:
    try:
        return json.loads(raw)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"{label} did not emit JSON: {error}") from error


def contains_invalid(value: object) -> bool:
    if isinstance(value, dict):
        if value.get("valid") is False:
            return True
        return any(contains_invalid(child) for child in value.values())
    if isinstance(value, list):
        return any(contains_invalid(child) for child in value)
    return False


def assert_equal(left: bytes, right: bytes, label: str) -> None:
    if left != right:
        raise RuntimeError(f"{label} output changed between equivalent runs")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--engine", required=True, type=Path)
    parser.add_argument("--corpus", default="rac", type=Path)
    args = parser.parse_args()
    engine = args.engine.resolve()
    corpus = args.corpus.resolve()
    if not engine.is_file() or not corpus.is_dir():
        parser.error("--engine must be a file and --corpus must be a directory")

    # Keep the isolated corpus beneath the checkout so repository-scoped RAC
    # configuration remains discoverable. The directory is still removed at
    # the end of the run and the copied corpus never touches the source tree.
    with tempfile.TemporaryDirectory(
        prefix=".decided-live-invariants-", dir=corpus.parent
    ) as scratch_raw:
        scratch = Path(scratch_raw)
        isolated = scratch / "corpus"
        shutil.copytree(corpus, isolated)
        env = os.environ.copy()
        env.update(
            {
                "LC_ALL": "C",
                "TZ": "UTC",
                "DECIDED_CACHE_DIR": str(scratch / "cache"),
                "XDG_STATE_HOME": str(scratch / "state"),
            }
        )
        env.pop("DECIDED_NO_CACHE", None)

        validate_args = ["validate", str(isolated), "--json", "--no-cache"]
        validation = invoke(engine, validate_args, env)
        validation_value = json_value(validation, "validate")
        if contains_invalid(validation_value):
            raise RuntimeError("live corpus contains an invalid artifact")

        deterministic = [
            validate_args,
            ["inspect", str(isolated), "--json"],
            ["relationships", str(isolated), "--json"],
            ["stats", str(isolated), "--json"],
            ["find", "search", str(isolated), "--json", "--no-cache"],
        ]
        for argv in deterministic:
            assert_equal(
                invoke(engine, argv, env),
                invoke(engine, argv, env),
                f"determinism: {' '.join(argv[:2])}",
            )

        for command in (
            ["validate", str(isolated), "--json"],
            ["find", "search", str(isolated), "--json"],
        ):
            uncached = invoke(engine, [*command, "--no-cache"], env)
            cold = invoke(engine, [*command, "--cache"], env)
            warm = invoke(engine, [*command, "--cache"], env)
            assert_equal(uncached, cold, f"cache parity (cold): {command[0]}")
            assert_equal(uncached, warm, f"cache parity (warm): {command[0]}")

        probe = "freshnessprobez9"
        before = json_value(
            invoke(engine, ["find", probe, str(isolated), "--json", "--cache"], env),
            "freshness before",
        )
        if isinstance(before, dict) and before.get("matches"):
            raise RuntimeError("freshness probe unexpectedly exists before mutation")
        target = next(path for path in sorted(isolated.rglob("*.md")) if path.is_file())
        with target.open("a", encoding="utf-8") as handle:
            handle.write(f"\n\n{probe}\n")
        after = json_value(
            invoke(engine, ["find", probe, str(isolated), "--json", "--cache"], env),
            "freshness after",
        )
        if not isinstance(after, dict) or not after.get("matches"):
            raise RuntimeError("cache failed to expose a completed corpus mutation")

    print("live corpus invariants: PASS")
    print("  validity: PASS")
    print("  determinism: PASS")
    print("  cache/no-cache equality: PASS")
    print("  freshness after mutation: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
