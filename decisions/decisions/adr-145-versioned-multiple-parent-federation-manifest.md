---
schema_version: 1
id: RAC-KZN54DB2M7FZ
type: decision
---
# ADR-145: Declare Multiple Offline Parents Through a Versioned Federation Manifest

## Context

ADR-134 fixes the secure first-increment carrier: one parent mapping in
`.decided/corpus.md`, local materialised bytes only, strict containment, source
verification, and a version-1 digest over config and artifact bytes. Its digest
intentionally excludes the operational manifest because transitive inheritance
is rejected. Reusing that pin for a graph would allow a parent's outgoing
topology and nested pins to change without changing the digest recorded by its
child.

Multiple and transitive parents therefore need an explicit new manifest and
digest mode. Existing version-1 repositories must not be reinterpreted.

## Decision

The fixed manifest remains `.decided/corpus.md`, with the exact lowercase
headings `## inherits` and `## overrides`. Manifest version 2 declares an
unordered `parents` sequence in the single fenced YAML mapping under
`## inherits`:

```yaml
version: 2
parents:
  - alias: standards
    source: acme/standards
    root: vendor/standards
    corpus: decisions
    digest: sha256-v2:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
  - alias: security
    source: acme/security
    root: vendor/security
    corpus: decisions
    digest: sha256-v2:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210
```

The sequence contains between one and 32 records. Each record has exactly
`alias`, `source`, `root`, `corpus`, and `digest`; unknown fields, duplicate
YAML keys, merge keys, duplicate aliases, and duplicate direct sources are
errors. Alias syntax is
`^[a-z0-9](?:[a-z0-9._-]*[a-z0-9])?$`, with the version-2 length bound from
ADR-144. A version-2 path is POSIX-relative UTF-8 and rejects absolute forms,
backslashes, drive or UNC prefixes, empty segments, `.`, and `..`.

The `version` in `## inherits` selects the complete manifest's semantic mode.
The `## overrides` section is optional; when present, its single fenced mapping
must carry the same `version`. Mixed section versions and `## overrides`
without a valid `## inherits` section are errors. Version 2 restricts both
operational mappings and every governing config parsed during graph capture to
YAML mappings, sequences, and scalar values with at most 32 levels and 16,384
nodes. Anchors, aliases, custom tags, and merge keys are rejected before
semantic construction. Each mapping, sequence, mapping key scalar, and value
scalar counts as one node in the event stream before construction. These
restrictions do not alter no-manifest or standalone version-1 parsing.

Each edge is relative to the repository root of the corpus that declares it.
Its materialisation root must be a strict canonical descendant of that root.
The materialisation and corpus roots must be real contained directories; the
governing config, present manifest, and discovered Markdown artifacts must be
real contained regular files. Symlinks and Windows reparse points are rejected.
Each `root` or `corpus` value is at most 4,096 UTF-8 bytes, 64 components, and
255 UTF-8 bytes per component. Recursion preserves ultimate containment inside
the invocation repository. Sibling physical roots may not overlap; nesting is
valid only along the declared ancestry edge or when two independently verified
routes are the same deduplicated source and pin.

An inherited regular file with more than one hard link is rejected. Capture
must also reject bind-mount, mount-point, volume, junction, or reparse-boundary
crossing between a declaring repository root and any inherited root or file.
When the platform cannot supply stable file identity, link-count, volume or
mount identity, and reparse metadata, version-2 verification fails with
`corpus-federation-unsupported-filesystem` rather than weakening the read-only
boundary.

AsDecided performs no clone, fetch, pull, refresh, registry lookup, or other
network operation. Updating bytes and pins remains an explicit Git operation
outside the engine.

Version-2 edge pins use a fixed SHA-256 byte contract:

```text
"asdecided-corpus-digest-v2\0"
frame(0x01, source UTF-8)
frame(0x02, exact governing .decided/config.yaml bytes)
frame(0x03, one byte: 0x00 when corpus.md is absent, 0x01 when present)
frame(0x04, exact .decided/corpus.md bytes)  # only when present
for each owned Markdown file in relative-path byte order:
    frame(0x05, corpus-relative POSIX UTF-8 path)
    frame(0x06, exact file bytes)
```

`frame` is the one-byte tag, unsigned 64-bit big-endian payload length, and
exact payload bytes. The owned snapshot excludes `.decided/` and every direct
parent materialisation subtree declared by the captured manifest. Checkout
path, timestamp, filesystem order, newline form, Unicode form, YAML meaning,
and Markdown meaning never enter the digest.

The manifest-presence record prevents adding or removing topology without a
repin. Because exact nested manifest bytes contain descendant pins, direct pins
form a Merkle-like commitment to the closure without hashing descendants into
the same node recursively.

Verification reads and bounds the target config and manifest, parses only the
strict declarations needed to establish local-walk exclusions, captures the
owned bytes, and verifies the incoming pin before following any child path
named by that manifest. Unverified manifest bytes never direct recursive
filesystem traversal. Every distinct canonical physical capture follows and
verifies its own child edges even when its source and node digest later
deduplicate; only the same canonical capture may reuse a completed route result.
Digesting, parsing, composition, and serving consume one captured byte snapshot
rather than reopening mutable files.

The operator surface is
`decided corpus digest --version 2 --root <parent-root> --corpus <parent-corpus>`
and prints `sha256-v2:` plus 64 lowercase hexadecimal characters. The current
command without `--version 2`, the `sha256:` prefix, the version-1 domain and
framing, its known vector, its stable findings, and valid version-1 output
remain byte-for-byte unchanged.

A version-1 root retains exactly one direct leaf parent, version-1 collision
and override rules, and its current transitivity failure. A version-2 edge may
target a corpus whose own manifest is absent, version 1, or version 2. A nested
version-1 node applies its existing one-parent/leaf-only rules in its own
context: its authored references, collision checks, alias qualification, and
override validation use the exact version-1 rules. Its resulting effective
projection is then one input to the enclosing version-2 graph. A valid nested
version-1 override is normalized to its resolved source-aware target, local
replacement, and local rationale keys, so a version-2 ancestor may explicitly
chain from that replacement without reinterpreting the version-1 node. During
a version-2 root composition, every captured node also receives a canonical
version-2 node digest for graph deduplication and provenance; a legacy
version-1 edge pin is still checked and retained as edge provenance.

This decision explicitly amends ADR-134 only where ADR-134 fixes one mapping
and a version-1 digest that omits topology. ADR-134's materialised-only,
offline, containment, exact-byte, source-verification, and pre-overlay failure
rules remain authoritative.

## Consequences

A direct pin now commits to the target's local content and its declared
outgoing topology, so bottom-up repinning is explicit and reviewable. V1 users
receive no semantic migration; graph behavior is entered only by authoring a
v2 manifest and v2 pins.

Operators must calculate new pins when converting to v2 or changing any nested
manifest byte, including comments and whitespace. This is intentionally
stricter than semantic YAML hashing and keeps verification portable.

## Status

Proposed

## Category

Architecture

## Alternatives Considered

### Put several mappings in a version-1 YAML list

Rejected. Older engines already reject that shape, and silently assigning new
meaning to it would break the recorded version-1 failure contract.

### Keep digest v1 and verify nested manifests separately

Rejected. The child pin would not authenticate which descendant sources and
pins its parent declared.

### Hash the fully expanded closure into every node

Rejected. It creates redundant work and awkward self-reference. Exact manifest
bytes already commit to the recursively verified descendant pins.

### Fetch missing parents from `source`

Rejected. `source` is stable identity, not a locator, and federation remains an
offline function of reviewed materialised bytes.

## Related Decisions

- adr-002
- adr-018
- adr-065
- adr-080
- adr-089
- adr-134
- adr-135
- adr-144

## Related Designs

- corpus-federation-mechanism
- corpus-federation-graph-composition

## Related Requirements

- parent-corpus-inheritance
- corpus-source-identity

## Related Roadmaps

- corpus-federation
