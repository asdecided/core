---
schema_version: 1
id: RAC-KWJ8S53D06CH
type: requirement
---
# Requirement: Parent Corpus Inheritance

## Status

Accepted

Classification: `[internal]` — declare, verify, and capture a bounded graph of
offline read-only parent corpora. This is the topology and materialisation half
of the `corpus-federation` programme.

## Problem

One verified parent proves the source-aware federation substrate, but real root
repositories may need several independently owned corpora and those corpora may
share pinned ancestors. Flattening that graph outside AsDecided hides ownership
and exception lineage. Loading parent directories independently cannot define
cycles, diamonds, alias scope, divergent pins, or one atomic freshness boundary.

The existing manifest and digest version must remain exact. Graph support
therefore needs an explicit v2 carrier that authenticates nested topology while
preserving the deterministic, offline, root-only write boundary.

## Requirements

- [REQ-001] A corpus MUST declare inheritance through pinned source references under the exact lowercase heading `## inherits` in the fixed operational Markdown manifest `.decided/corpus.md` (ADR-089). The section MUST contain exactly one fenced YAML mapping, and the manifest MUST remain outside artifact discovery, search, relationships, and exports. Version 1 MUST retain one mapping; version 2 MUST carry the strict `parents` sequence defined by ADR-145.
- [REQ-002] Parent resolution MUST read only materialised bytes already on disk: it MUST NOT clone, pull, fetch, refresh, query a registry, or perform network I/O in validation, resolution, retrieval, enforcement, export, or serving paths (ADR-002, ADR-089). Updating a submodule or vendored directory and repinning it MUST remain an explicit user Git operation outside AsDecided.
- [REQ-003] The invocation corpus MUST remain the only writable graph node. No AsDecided command may write, rename, migrate, scaffold, recover a transaction, install generated rules, or create output beneath any direct, transitive, diamond, or duplicate physical parent route (ADR-018, ADR-065, ADR-080).
- [REQ-004] Validation MUST emit deterministic, distinct, stable-coded errors for missing materialisation, missing parent corpus or config, malformed declaration, source mismatch, containment failure, digest mismatch, snapshot change, duplicate parent, cycle, divergent pin, invalid node, overlapping roots, override divergence, and exceeded limits. No inherited artifact may enter the effective corpus after any verification failure, and partial overlays are forbidden.
- [REQ-005] The declaration MUST remain backward-compatible under ADR-089. Engines predating federation continue to ignore `.decided/corpus.md`; no-manifest repositories MUST retain contemporaneous single-corpus output byte-for-byte. A version-1 manifest MUST retain its exact one-parent/leaf-only semantics, `sha256:` digest and known vector, stable findings, transitivity rejection, and observable output.
- [REQ-006] Optional configuration defaults MAY assist materialisation discovery only through the established section-loader pattern. The Markdown manifest MUST remain the inheritance and pin source of truth, and configuration MUST NOT silently create, update, reorder, fetch, or repin it (ADR-089).
- [REQ-007] The capability MUST remain available to every user and MUST NOT be enterprise-gated (ADR-085, ADR-089). Graph implementation MUST conform to accepted ADR-144 through ADR-148; this requirement does not substitute for or amend that authority.
- [REQ-008] Under ADR-088, `decided init --parent-corpus` MUST emit deterministic v2-first guidance only when explicitly requested, with or without a profile and for fresh or already-initialised repositories. It MUST name `.decided/corpus.md`, the exact lowercase `## inherits` and `## overrides` headings, materialisation before pinning, and `decided corpus digest --version 2`; it MUST NOT create a manifest, fetch anything, or write parent bytes. Init/profile human and JSON output without the flag MUST remain byte-identical, and JSON MUST add the guidance field only when requested.
- [REQ-009] A v1 declaration MUST retain its exact `version`, `alias`, `source`, `root`, `corpus`, and full `sha256` fields. A v2 declaration MUST contain `version: 2` and one to 32 strict parent records, each with exactly `alias`, `source`, `root`, `corpus`, and a full lowercase `sha256-v2` digest. The `## inherits` version MUST select the whole manifest semantic mode; a present `## overrides` mapping MUST use the same version, and mixed versions or overrides without valid inheritance MUST fail. Every graph node MUST declare an explicit valid `corpus.source`; aliases MUST be unique and source-local, and duplicate direct sources MUST fail.
- [REQ-010] A v1 root MUST accept exactly one direct leaf parent and retain current multiple/transitive findings. A v2 root MUST accept the bounded acyclic graph in ADR-144. Active-ancestry source recurrence MUST be a cycle; a completed source MAY deduplicate only when every independently verified route has the same canonical v2 node digest, and the same source with a different digest MUST be a divergent-pin error.
- [REQ-011] Every parent materialisation MUST remain inside the canonical repository root that declares it and the complete closure MUST remain inside the invocation repository. V2 `root` and `corpus` values MUST be POSIX-relative UTF-8 and at most 4,096 bytes, 64 components, and 255 bytes per component. Absolute, empty-segment, `.`, `..`, backslash, drive, UNC, unresolved, and canonical-escape paths MUST fail. Materialisation/corpus roots MUST be real directories; configs, present manifests, and Markdown artifacts MUST be real regular files. Symlink/reparse traversal, hard-linked inherited files, and mount/volume/junction boundary crossing MUST fail before bytes enter a digest or corpus. If required identity metadata is unavailable, v2 MUST fail as an unsupported filesystem. The root-local walk MUST exclude every verified physical materialisation subtree.
- [REQ-012] Before overlay, the engine MUST verify source identity and a canonical versioned SHA-256 digest. V1 MUST retain its exact domain and config/artifact framing. V2 MUST use ADR-145's domain and framing over source, exact governing config, explicit nested-manifest presence, exact nested-manifest bytes when present, and sorted owned corpus-relative Markdown paths and bytes, excluding parent subtrees, checkout location, timestamps, iteration order, and normalization. The same pure calculations MUST be available through the existing operator command and explicit `--version 2` mode.
- [REQ-013] Every unique inherited node MUST pass structural and relationship validation in its source visibility context before root composition. A node error MUST surface as one deterministically selected sourced root blocker rather than one copy per diamond route; parent warnings and review advisories MUST remain parent-owned and MUST NOT be repeated in descendants. Remediation MUST NOT direct an AsDecided command to edit inherited bytes.
- [REQ-014] Version-2 operational mappings and every governing config parsed during graph capture MUST use only YAML mappings, sequences, and scalars, with at most 32 levels and 16,384 nodes, and MUST reject anchors, aliases, custom tags, and merge keys. Version-2 manifests MUST also reject unknown or duplicate YAML keys, malformed source values, aliases outside `^[a-z0-9](?:[a-z0-9._-]*[a-z0-9])?$`, aliases over 64 bytes, and source identities over 255 bytes. Parent order MUST grant no precedence, and discovery, validation, findings, catalog construction, and output MUST use fixed bytewise source-aware ordering. No-manifest and standalone v1 YAML behavior MUST remain unchanged.
- [REQ-015] Verification MUST capture and bound a target config and manifest, parse only declarations needed to establish safe local-walk exclusions, capture owned local bytes, and verify the incoming pin before following child paths named by that manifest. Digesting, parsing, validation, composition, and serving MUST consume one immutable snapshot; a file or directory identity change during capture MUST fail closed.
- [REQ-016] Version 2 MUST enforce the fixed limits in ADR-144 and ADR-145: 1 MiB config/manifest; path, YAML, alias, and source bounds; 32 direct parents; depth 16; 256 unique inherited sources; 1,024 edges; 4,096 overrides; 50,000 unique inherited Markdown files; 16 MiB per inherited file; 256 MiB unique inherited bytes; 512 MiB physical verification bytes; and 200,000 visited entries. Exactly-at-limit MUST succeed. On plus one, the counter for the exceeded dimension MUST stop, report `observed = limit + 1`, and fail without truncation through `corpus-federation-limit-exceeded`.
- [REQ-017] Every physical edge MUST be path, configured-source, and declared-pin verified even when its logical node later deduplicates. Every previously unseen canonical physical capture MUST verify its own outgoing descendant edges; only an identical canonical capture may reuse a verified route result. Logical bytes MUST sum exact config, present-manifest, and owned-Markdown bytes once per unique inherited node after verified deduplication; root bytes are excluded. Physical bytes MUST charge those inputs once per distinct canonical `(materialisation root, corpus root)` capture, including failed or later-deduplicated routes. Edge and override declarations MUST count across distinct physical manifests before logical node deduplication. Visited entries MUST include ignored, excluded, rejected, and later-deduplicated directory entries examined below inherited roots. The loader MUST store unique nodes and edges rather than enumerate every possible diamond path.
- [REQ-018] The loader MUST retain every canonical physical materialisation and corpus root in one read-only set. Sibling roots MUST NOT overlap except for the same verified logical route; valid nesting MUST follow a declared ancestry edge. Full/diff code enumeration and every mutation/output guard MUST use the complete set, including symlinked ancestors, lexical `..`, and nonexistent target suffixes.
- [REQ-019] A v2 edge MAY target a corpus with no manifest, a v1 manifest, or a v2 manifest. A nested v1 node MUST apply its exact one-parent/leaf-only relationship, collision, alias, and override rules in its own context. Its valid effective projection and any v1 override MUST then be normalized to source-aware keys as graph input, and a v2 ancestor MAY explicitly chain from that local replacement. During a v2-root run, every captured node MUST receive a canonical v2 node digest for deduplication and provenance while any declared legacy v1 edge pin and mapping spelling remain separately checked and attributable.
- [REQ-020] Core federation semantics MUST depend only on the explicitly materialised repository tree, governing config and manifests, stable source identities, and verified pins. They MUST NOT require a `.git` directory, remote name, default branch, forge URL, GitHub API, Cursor Origin API, or another host-specific service. Local-only repositories and checkouts from any standard Git remote MUST produce the same answer from identical bytes. Optional clone, review, or check integrations MUST remain outside graph verification and MUST NOT change corpus identity or resolution.
- [REQ-021] `decided corpus status [directory] [--json]` MUST verify the complete version-2 closure through the central immutable loader before rendering. It MUST report every logical source, canonical pin, manifest version, canonical source route, exact physical-route count, declared edge and alias, repository-relative materialisation/corpus path, catalog/effective/root-local count, override count, graph depth, and read-only boundary. Stable JSON schema version 1 MUST state that networking was absent, pins were verified, and snapshots were immutable. A closure failure MUST emit no partial report, and checkout location MUST NOT enter stable output.

## Acceptance Criteria

- Existing no-manifest CLI, MCP, export, and cache goldens and the complete
  valid/error v1 suite remain byte-identical.
- Equivalent v1 and v2 one-parent fixtures produce the same effective records;
  v2 additionally carries its graph pin and provenance.
- Two and three direct parents produce byte-identical public output under every
  root-list permutation. A nested-list permutation requires bottom-up repinning
  and then preserves effective identities/ranking/findings while pin and
  generation provenance changes. A transitive inherited Decision resolves and
  enforces against root code.
- A same-pin diamond emits one logical catalog record after verifying every
  physical route. A divergent-pin diamond, root-source recurrence, self-cycle,
  and longer cycle each fail with stable order and no overlay.
- Absolute, escaping, symlink, overlapping-sibling, and nested path attacks fail
  at every depth. Tests prove every physical parent tree remains byte-identical
  after every command, including nonexistent output targets through symlink or
  `..` spellings.
- V2 known vectors change for config, manifest presence/bytes, path, or Markdown
  bytes and remain stable across checkout path, timestamp, and filesystem order.
- Each fixed resource limit succeeds at the boundary and fails at plus one. A
  many-diamond fixture proves work scales with verified nodes, edges, and bytes,
  not the number of possible routes.
- A captured-file or directory-shape swap cannot mix hashed bytes with parsed or
  served bytes; verification fails closed.
- `corpus status` reports a same-pin diamond's one logical shared node and both
  independently verified physical routes, is byte-identical across checkout
  locations, works without `.git`, and emits no stdout after either copy is
  tampered.
- The runnable federation example validates from a plain local tree and makes
  no GitHub, Cursor Origin, branch-name, remote-name, or network assumption.
- YAML depth/node/anchor bombs, overlong path components, hard links,
  mount/volume crossings, junctions, and Windows reparse points fail within the
  fixed bounds rather than changing acceptance by platform.

## Success Metrics

- A root repository can pin several independent corpora and their shared
  ancestors in ordinary reviewed Git diffs, with networking disabled.
- Updating any graph edge or node is an explicit materialisation and bottom-up
  pin change; no command silently changes governing bytes.
- A shared source is verified on every physical route but represented once in
  the logical corpus and warning ownership.
- Identical materialised bytes remain portable between local Git, Cursor
  Origin, GitHub, and other standard Git forges without a federation adapter.

## Risks

- Unverified nested manifest bytes direct traversal. Mitigation: verify the
  target's v2 node pin before following its declared child paths.
- A diamond hides a tampered copy. Mitigation: every physical route is checked
  before source-and-pin deduplication.
- Large or adversarial graphs exhaust a server. Mitigation: fixed versioned
  limits cover topology, files, logical bytes, physical work, and visited entries.
- A path escapes through symlink or nonexistent suffix handling. Mitigation:
  canonical containment, no-follow capture, and the union read-only-root guard.
- Graph behavior leaks into v1. Mitigation: explicit semantic modes and complete
  v1 byte/error regression.

## Assumptions

- Every parent is a submodule or vendored directory beneath its declaring
  repository root.
- `corpus-source-identity` remains the stable global node identity and
  `repository_key` remains only the artifact-id generation namespace.
- ADR-144 and ADR-145 are accepted authority for recursive loader work.

## Related Decisions

- adr-002
- adr-018
- adr-065
- adr-080
- adr-085
- adr-088
- adr-089
- adr-134
- adr-135
- adr-138
- adr-144
- adr-145
- adr-148

## Related Designs

- corpus-federation-mechanism
- corpus-federation-graph-composition

## Related Roadmaps

- corpus-federation

## Related Requirements

- corpus-source-identity
- federated-resolution-provenance
