---
schema_version: 1
id: RAC-KZKMJ92ABVJG
type: decision
---
# ADR-135: Use `corpus.source` as the Global Corpus Identity

## Context

Federation needs an identity that survives checkout moves and distinguishes an
artifact's corpus from the namespace used to mint its opaque ID. Existing
`repository_key` values are short ID-generation prefixes and can be shared or
changed independently of a repository's durable provenance. Directory names
and materialisation paths are local implementation details.

Exports need the same identity as validation, retrieval, MCP, and enforcement;
separate source derivations would make the same inherited artifact appear to
come from different corpora on different surfaces.

## Decision

`corpus.source` in the governing `.decided/config.yaml` is the stable global
identity of a corpus.

- Both child and parent must set explicit, non-empty, distinct `corpus.source`
  values before federation is valid.
- `repository_key` remains the namespace used to generate artifact IDs. It is
  not promoted to global corpus identity.
- The readable parent alias in `.decided/corpus.md` is local to that child and
  is not a second global identifier.
- Internal artifact identity is `(source, canonical_id)` and internal artifact
  path identity is `(source, relative_path)`.
- Every source-aware consumer and export uses the same configured value.
- Repository-key and directory-basename fallbacks may remain for compatible
  non-federated exports, but federation never relies on them.

Changing a configured source is an identity migration, not a checkout rename.

## Consequences

Parent provenance remains stable when the materialisation directory changes,
and a shared parent can deduplicate across exports from several children. The
engine can distinguish identical relative paths and opaque IDs that originate
in different corpora.

Federated repositories must choose and maintain durable source names. Existing
repositories that do not federate keep their current behavior and may continue
to use export fallbacks until they opt into explicit identity.

## Status

Proposed

## Category

Architecture

## Alternatives Considered

### Reuse `repository_key`

Rejected. It is an ID-generation namespace, not a globally unique provenance
contract, and changing that meaning would entangle two independent concerns.

### Use the checkout or parent directory path

Rejected. Paths vary across machines and child repositories and would make
source identity non-portable.

### Derive identity from the parent digest

Rejected. A new parent version would become a different corpus rather than a
new pinned state of the same corpus.

## Related Decisions

- adr-007
- adr-026
- adr-074
- adr-089
- adr-133
- adr-134

## Related Designs

- corpus-federation-mechanism

## Related Requirements

- rac-export-source-identity
- rac-federated-resolution-provenance
- rac-parent-corpus-inheritance
