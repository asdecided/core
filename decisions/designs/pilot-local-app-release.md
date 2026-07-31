---
schema_version: 1
id: RAC-01K8P7A3M7QZ
type: design
---
# Pilot Local App Release

## Context

ADR-125 selects Pilot's signed local App Store. This design defines the release
prerequisite and the later catalogue handoff without adding a hosted AsDecided
service or maintaining a hand-written Pilot adapter.

## User Need

A Pilot user needs one discover-install-call flow that reads engineering
decisions from a repository already on their machine. They need clear authority
and integrity signals before allowing an adapter to execute a binary and read a
repository.

## Design

The Core release matrix builds `decided` and `decided-mcp` natively on GitHub's
hosted amd64 and arm64 runners for both Linux and macOS. Archive names continue
to use Rust target triples so Pilot submission automation can map them without
guessing host architecture.

After that release exists, Pilot's official `pilot-app` generator receives a
CLI specification with four exact asset URLs and SHA-256 values. It generates
the IPC adapter, manifest, help method, bundle, and catalogue submission. The
app exposes a small surface:

- `asdecided.help` — local method and parameter discovery;
- `asdecided.retrieve` — `decided retrieve <task> <repository> --json`;
- `asdecided.validate` — `decided validate <repository> --json`;
- `asdecided.relationships` — relationship inspection over the repository.

No passthrough method is included. The submission's product demo starts with a
retrieval against a repository path and explains that the path remains local.

## Constraints

- Pilot's generator and verifier are the authority for adapter shape; Core does
  not vendor or fork the IPC implementation.
- Release archives must exist before their URLs and hashes can be committed to a
  Pilot submission.
- The app may read only repositories available to the local Pilot node.
- A source-level `proc.exec` implementation is not sufficient evidence of
  deployed compatibility; the release gate includes a real node.

## Rationale

Native hosted runners avoid cross-compilation and produce target-labelled
archives through the existing release workflow. Generating the thin Pilot layer
keeps ownership of its evolving IPC and manifest details with Pilot while
AsDecided owns only the curated command mapping and product claims.

## Alternatives

- **Cross-compile from one Linux runner:** rejected because macOS targets need
  Apple tooling and native runners make the provenance easier to inspect.
- **Commit an app spec with placeholder hashes:** rejected because it cannot be
  verified or submitted and creates configuration that appears shippable.
- **Hand-write a Rust Pilot IPC adapter:** rejected because it duplicates the
  supported generator and creates a new protocol maintenance surface.

## Accessibility

The catalogue description and product demo use plain text, copyable commands,
and explicit parameter names. Success and errors must be represented in JSON,
not by colour or terminal formatting alone.

## Style Guidance

Lead with “engineering decisions on this machine,” not transport mechanics.
Use “local,” “read-only,” and “deterministic” precisely. Avoid “hosted,” “cloud
knowledge base,” and claims that Pilot itself enforces AsDecided decisions.

## Open Questions

- What is the narrowest Pilot filesystem grant that still permits a user-chosen
  repository path?
- Which relationship command shape gives Pilot callers stable JSON without
  exposing arbitrary CLI arguments?

## Related Decisions

- adr-125

## Related Requirements

- asdecided-pilot-app-distribution
