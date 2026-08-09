#!/usr/bin/env python3
"""Validate packaged export schemas against fixture and live-corpus output."""

from __future__ import annotations

import argparse
import copy
import json
import subprocess
import tempfile
from pathlib import Path
from typing import Any

from jsonschema import Draft202012Validator


ROOT = Path(__file__).resolve().parents[2]
SCHEMA_DIR = ROOT / "rust" / "rac-engine" / "assets" / "schemas"
SCHEMA_PATHS = {
    "viewer": SCHEMA_DIR / "export-viewer-v1.schema.json",
    "documents": SCHEMA_DIR / "export-documents-v1.schema.json",
    "graph": SCHEMA_DIR / "export-graph-v1.schema.json",
}


def run(engine: Path, *args: str) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(
        [str(engine), *args],
        cwd=ROOT,
        check=False,
        capture_output=True,
    )


def successful(engine: Path, *args: str) -> bytes:
    result = run(engine, *args)
    if result.returncode != 0:
        raise AssertionError(
            f"{' '.join(args)} exited {result.returncode}: "
            f"{result.stderr.decode('utf-8', errors='replace')}"
        )
    return result.stdout


def successful_at(engine: Path, cwd: Path, *args: str) -> bytes:
    result = subprocess.run(
        [str(engine), *args],
        cwd=cwd,
        check=False,
        capture_output=True,
    )
    if result.returncode != 0:
        raise AssertionError(
            f"{' '.join(args)} at {cwd} exited {result.returncode}: "
            f"{result.stderr.decode('utf-8', errors='replace')}"
        )
    return result.stdout


def assert_checkout_stability(engine: Path) -> None:
    artifact = """---
schema_version: 1
id: APP-111111111111
type: decision
---
# Stable source

## Context

Checkout locations differ.

## Decision

Keep exports stable.

## Consequences

Consumers keep one identity.

## Status

Accepted
"""
    with tempfile.TemporaryDirectory(prefix="asdecided-source-checkouts-") as temporary:
        roots = [Path(temporary) / "first", Path(temporary) / "second"]
        for root in roots:
            (root / ".decided").mkdir(parents=True)
            (root / "decisions").mkdir()
            (root / ".decided" / "config.yaml").write_text(
                "repository_key: APP\ncorpus:\n  source: acme/stable\n",
                encoding="utf-8",
            )
            (root / "decisions" / "decision.md").write_text(
                artifact,
                encoding="utf-8",
            )

        modes = ((), ("--documents",), ("--graph",))
        for extra in modes:
            first = successful_at(engine, roots[0], "export", "decisions", *extra)
            second = successful_at(engine, roots[1], "export", "decisions", *extra)
            if first != second:
                mode = extra[0] if extra else "viewer"
                raise AssertionError(
                    f"{mode} export changed across equivalent checkout locations"
                )


def object_keys(value: Any, schema: dict[str, Any], label: str) -> None:
    if not isinstance(value, dict):
        raise AssertionError(f"{label} is not an object")
    properties = set(schema["properties"])
    required = set(schema["required"])
    if required != properties:
        raise AssertionError(
            f"{label} schema does not require every declared property: "
            f"required={sorted(required)}, properties={sorted(properties)}"
        )
    emitted = set(value)
    if emitted != properties:
        raise AssertionError(
            f"{label} producer/schema drift: "
            f"emitted={sorted(emitted)}, properties={sorted(properties)}"
        )


def validate_shapes(
    viewer: dict[str, Any],
    documents: list[dict[str, Any]],
    graph: dict[str, Any],
    schemas: dict[str, dict[str, Any]],
) -> None:
    viewer_schema = schemas["viewer"]
    object_keys(viewer, viewer_schema, "viewer")
    object_keys(viewer["corpus"], viewer_schema["properties"]["corpus"], "viewer.corpus")
    artifact_schema = viewer_schema["properties"]["artifacts"]["items"]
    for index, artifact in enumerate(viewer["artifacts"]):
        object_keys(artifact, artifact_schema, f"viewer.artifacts[{index}]")
    relationship_schema = viewer_schema["properties"]["relationships"]["items"]
    for index, relationship in enumerate(viewer["relationships"]):
        object_keys(
            relationship,
            relationship_schema,
            f"viewer.relationships[{index}]",
        )

    document_schema = schemas["documents"]
    for index, document in enumerate(documents):
        object_keys(document, document_schema, f"documents[{index}]")
        object_keys(
            document["metadata"],
            document_schema["properties"]["metadata"],
            f"documents[{index}].metadata",
        )

    graph_schema = schemas["graph"]
    object_keys(graph, graph_schema, "graph")
    node_schema = graph_schema["properties"]["nodes"]["items"]
    for index, node in enumerate(graph["nodes"]):
        object_keys(node, node_schema, f"graph.nodes[{index}]")
    edge_schema = graph_schema["properties"]["edges"]["items"]
    for index, edge in enumerate(graph["edges"]):
        object_keys(edge, edge_schema, f"graph.edges[{index}]")


def load_exports(
    engine: Path, corpus: Path
) -> tuple[dict[str, Any], list[dict[str, Any]], dict[str, Any]]:
    corpus_text = str(corpus)
    viewer = json.loads(successful(engine, "export", corpus_text))
    document_bytes = successful(engine, "export", corpus_text, "--documents")
    documents = [json.loads(line) for line in document_bytes.splitlines() if line]
    graph = json.loads(successful(engine, "export", corpus_text, "--graph"))
    return viewer, documents, graph


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--engine", required=True, type=Path)
    args = parser.parse_args()
    engine = args.engine.resolve()

    schemas = {
        name: json.loads(path.read_text(encoding="utf-8"))
        for name, path in SCHEMA_PATHS.items()
    }
    validators: dict[str, Draft202012Validator] = {}
    for name, schema in schemas.items():
        Draft202012Validator.check_schema(schema)
        validators[name] = Draft202012Validator(schema)

        packaged = SCHEMA_PATHS[name].read_bytes()
        first = successful(engine, "export", "--schema", name)
        second = successful(engine, "export", f"--schema={name}")
        if first != packaged or second != packaged:
            raise AssertionError(f"--schema {name} did not emit the packaged bytes exactly")

    unknown = run(engine, "export", "--schema", "unknown")
    if unknown.returncode != 2:
        raise AssertionError(f"unknown schema exited {unknown.returncode}, expected 2")

    fixture = ROOT / "rust" / "fixtures" / "closure" / "export" / "proj" / "rac"
    live = ROOT / "decisions"
    for label, corpus in (("fixture", fixture), ("live", live)):
        viewer, documents, graph = load_exports(engine, corpus)
        validators["viewer"].validate(viewer)
        for document in documents:
            validators["documents"].validate(document)
        validators["graph"].validate(graph)
        validate_shapes(viewer, documents, graph, schemas)
        if not documents:
            raise AssertionError(f"{label} documents export was unexpectedly empty")

    sample = json.loads(
        (ROOT / "rac-localview" / "src" / "viewer" / "sample" / "lore-export.sample.json")
        .read_text(encoding="utf-8")
    )
    validators["viewer"].validate(sample)

    _, fixture_documents, _ = load_exports(engine, fixture)
    missing_key = copy.deepcopy(fixture_documents[0])
    del missing_key["id"]
    if validators["documents"].is_valid(missing_key):
        raise AssertionError("documents schema accepted a record missing required id")

    additive = copy.deepcopy(fixture_documents[0])
    additive["future_field"] = {"accepted": True}
    additive["metadata"]["future_metadata"] = "accepted"
    if not validators["documents"].is_valid(additive):
        raise AssertionError("documents schema rejected additive fields")

    assert_checkout_stability(engine)

    print(
        "export schemas and source identity: fixture, viewer sample, live corpus, "
        "and checkout-stability cases conform"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
