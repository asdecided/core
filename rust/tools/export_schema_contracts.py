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


def decision_artifact(
    artifact_id: str, title: str, decision: str, relationships: str = ""
) -> str:
    return f"""---
schema_version: 1
id: {artifact_id}
type: decision
---
# {title}

## Status

Accepted

## Context

The export schema fixture exercises verified corpus federation.

## Decision

{decision}

## Consequences

The emitted record remains deterministic and source-aware.
{relationships}"""


def assert_federated_exports(
    engine: Path,
    schemas: dict[str, dict[str, Any]],
    validators: dict[str, Draft202012Validator],
) -> None:
    parent_id = "STD-000000000001"
    parent_target_id = "STD-000000000002"
    replacement_id = "APP-000000000001"
    rationale_id = "APP-000000000002"
    with tempfile.TemporaryDirectory(prefix="asdecided-federated-export-") as temporary:
        child = Path(temporary) / "child"
        parent = child / "vendor" / "standards"
        child_corpus = child / "decisions"
        parent_corpus = parent / "decisions"
        for root in (child, parent):
            (root / ".decided").mkdir(parents=True)
            (root / "decisions").mkdir()
        (child / ".decided" / "config.yaml").write_text(
            "repository_key: APP\ncorpus:\n  source: acme/app\n",
            encoding="utf-8",
        )
        (parent / ".decided" / "config.yaml").write_text(
            "repository_key: STD\ncorpus:\n  source: acme/standards\n",
            encoding="utf-8",
        )
        (parent_corpus / "policy.md").write_text(
            decision_artifact(
                parent_id,
                "ADR-001: Parent policy",
                "Keep the inherited policy.",
                f"\n## Related Decisions\n\n- {parent_target_id}\n",
            ),
            encoding="utf-8",
        )
        (parent_corpus / "related.md").write_text(
            decision_artifact(
                parent_target_id,
                "ADR-002: Related parent policy",
                "Keep source-aware relationship endpoints.",
            ),
            encoding="utf-8",
        )
        (child_corpus / "replacement.md").write_text(
            decision_artifact(
                replacement_id,
                "ADR-001: Local replacement",
                "Apply the bounded local replacement.",
            ),
            encoding="utf-8",
        )
        (child_corpus / "rationale.md").write_text(
            decision_artifact(
                rationale_id,
                "ADR-002: Override rationale",
                "Approve the explicit local override.",
            ),
            encoding="utf-8",
        )

        pin = successful_at(
            engine,
            child,
            "corpus",
            "digest",
            "--root",
            "vendor/standards",
            "--corpus",
            "decisions",
        ).decode("ascii").strip()
        if len(pin) != 71 or not pin.startswith("sha256:"):
            raise AssertionError(f"unexpected parent digest shape: {pin}")
        (child / ".decided" / "corpus.md").write_text(
            f"""# Corpus federation

## inherits

```yaml
version: 1
alias: standards
source: acme/standards
root: vendor/standards
corpus: decisions
digest: {pin}
```

## overrides

```yaml
version: 1
items:
  - parent: standards::{parent_id}
    with: {replacement_id}
    rationale: {rationale_id}
```
""",
            encoding="utf-8",
        )

        viewer, documents, graph = load_exports(engine, child_corpus)
        validators["viewer"].validate(viewer)
        for document in documents:
            validators["documents"].validate(document)
        validators["graph"].validate(graph)
        validate_shapes(viewer, documents, graph, schemas)

        viewer_by_id = {artifact["id"]: artifact for artifact in viewer["artifacts"]}
        if set(viewer_by_id) != {
            parent_id,
            parent_target_id,
            replacement_id,
            rationale_id,
        }:
            raise AssertionError("federated viewer did not retain the full catalog")
        parent_provenance = viewer_by_id[parent_id]["provenance"]
        if (
            parent_provenance["source"] != "acme/standards"
            or parent_provenance["layer"] != "inherited"
            or parent_provenance["pin"] != pin
            or parent_provenance["overrides"][0]["state"] != "overridden"
        ):
            raise AssertionError("parent override provenance was incomplete")
        replacement_provenance = viewer_by_id[replacement_id]["provenance"]
        if (
            replacement_provenance["source"] != "acme/app"
            or replacement_provenance["layer"] != "local"
            or "pin" in replacement_provenance
            or replacement_provenance["overrides"][0]["state"] != "replacement"
        ):
            raise AssertionError("replacement provenance was incomplete")
        parent_edge = next(
            edge for edge in graph["edges"] if edge["source"] == parent_id
        )
        if parent_edge["source_identity"] != {
            "source": "acme/standards",
            "id": parent_id,
        } or parent_edge["target_identity"] != {
            "source": "acme/standards",
            "id": parent_target_id,
        }:
            raise AssertionError("graph endpoints were not source-aware")

        modes = ((), ("--documents",), ("--graph",))
        for extra in modes:
            payload = successful(
                engine,
                "export",
                str(child_corpus),
                *extra,
                "--local-only",
            )
            if extra == ("--documents",):
                ids = {json.loads(line)["id"] for line in payload.splitlines() if line}
            elif extra == ("--graph",):
                ids = {node["id"] for node in json.loads(payload)["nodes"]}
            else:
                ids = {artifact["id"] for artifact in json.loads(payload)["artifacts"]}
            if ids != {replacement_id, rationale_id}:
                raise AssertionError(f"{extra or ('viewer',)} local-only leaked parent records")

        successful_at(
            engine,
            child,
            "export",
            ".",
            "--okf",
            "--out",
            "okf-export",
        )
        okf_index = (child / "okf-export" / "index.md").read_text(encoding="utf-8")
        if "Parent policy" in okf_index or "Related parent policy" in okf_index:
            raise AssertionError("root OKF export leaked the materialised parent")

        forbidden_outputs = (
            ("--html", parent / "forbidden.html"),
            ("--okf", parent / "forbidden-okf"),
        )
        for mode, output in forbidden_outputs:
            result = run(
                engine,
                "export",
                str(child_corpus),
                mode,
                "--out",
                str(output),
            )
            if result.returncode != 1 or output.exists():
                raise AssertionError(
                    f"{mode} did not reject its inherited read-only output path"
                )


def object_keys(value: Any, schema: dict[str, Any], label: str) -> None:
    if not isinstance(value, dict):
        raise AssertionError(f"{label} is not an object")
    properties = set(schema["properties"])
    required = set(schema["required"])
    if not required <= properties:
        raise AssertionError(
            f"{label} schema requires undeclared properties: "
            f"required={sorted(required)}, properties={sorted(properties)}"
        )
    emitted = set(value)
    if not required <= emitted or not emitted <= properties:
        raise AssertionError(
            f"{label} producer/schema drift: "
            f"required={sorted(required)}, emitted={sorted(emitted)}, "
            f"properties={sorted(properties)}"
        )


def validate_identity(
    value: dict[str, Any], schema: dict[str, Any], label: str
) -> None:
    object_keys(value, schema["$defs"]["identity"], label)


def validate_provenance(
    value: dict[str, Any], schema: dict[str, Any], label: str
) -> None:
    object_keys(value, schema["$defs"]["provenance"], label)
    for index, override in enumerate(value.get("overrides", [])):
        override_label = f"{label}.overrides[{index}]"
        object_keys(override, schema["$defs"]["override"], override_label)
        for endpoint in ("parent", "replacement", "rationale"):
            validate_identity(
                override[endpoint],
                schema,
                f"{override_label}.{endpoint}",
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
        if "provenance" in artifact:
            validate_provenance(
                artifact["provenance"],
                viewer_schema,
                f"viewer.artifacts[{index}].provenance",
            )
    relationship_schema = viewer_schema["properties"]["relationships"]["items"]
    for index, relationship in enumerate(viewer["relationships"]):
        object_keys(
            relationship,
            relationship_schema,
            f"viewer.relationships[{index}]",
        )
        if "from_identity" in relationship:
            validate_identity(
                relationship["from_identity"],
                viewer_schema,
                f"viewer.relationships[{index}].from_identity",
            )
        if relationship.get("to_identity") is not None:
            validate_identity(
                relationship["to_identity"],
                viewer_schema,
                f"viewer.relationships[{index}].to_identity",
            )
        if "provenance" in relationship:
            validate_provenance(
                relationship["provenance"],
                viewer_schema,
                f"viewer.relationships[{index}].provenance",
            )

    document_schema = schemas["documents"]
    for index, document in enumerate(documents):
        object_keys(document, document_schema, f"documents[{index}]")
        object_keys(
            document["metadata"],
            document_schema["properties"]["metadata"],
            f"documents[{index}].metadata",
        )
        if "provenance" in document["metadata"]:
            validate_provenance(
                document["metadata"]["provenance"],
                document_schema,
                f"documents[{index}].metadata.provenance",
            )

    graph_schema = schemas["graph"]
    object_keys(graph, graph_schema, "graph")
    node_schema = graph_schema["properties"]["nodes"]["items"]
    for index, node in enumerate(graph["nodes"]):
        object_keys(node, node_schema, f"graph.nodes[{index}]")
        if "provenance" in node:
            validate_provenance(
                node["provenance"],
                graph_schema,
                f"graph.nodes[{index}].provenance",
            )
    edge_schema = graph_schema["properties"]["edges"]["items"]
    for index, edge in enumerate(graph["edges"]):
        object_keys(edge, edge_schema, f"graph.edges[{index}]")
        if "source_identity" in edge:
            validate_identity(
                edge["source_identity"],
                graph_schema,
                f"graph.edges[{index}].source_identity",
            )
        if edge.get("target_identity") is not None:
            validate_identity(
                edge["target_identity"],
                graph_schema,
                f"graph.edges[{index}].target_identity",
            )
        if "provenance" in edge:
            validate_provenance(
                edge["provenance"],
                graph_schema,
                f"graph.edges[{index}].provenance",
            )


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
    assert_federated_exports(engine, schemas, validators)

    print(
        "export schemas and source identity: fixture, viewer sample, live corpus, "
        "checkout-stability, and federation cases conform"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
