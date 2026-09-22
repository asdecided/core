---
schema_version: 1
id: RAC-M32XGEVNX50E
type: decision
tags: [federation, extensibility, schema, spec, architecture]
---
# ADR-150: Inherit Parent Spec Bundles Across the Federation Graph

## Status

Accepted

**Amended 2026-09-22.** Decision 1 named the manifest's declaration order;
the manifest loader canonicalises `parents` for every consumer (ADR-146 rejects
a second precedence rule), so the implementation (asdecided/core #487) composes
in that canonical order and decision 1 now says so. Identity of a duplicate is
the admitted element's content, not its bytes, so key order and whitespace
cannot manufacture a conflict. Order never changes which types exist or their
content.

**Accepted 2026-09-22.** Accepted ahead of implementation, unlike ADR-083,
whose revision held at Proposed until the mechanism shipped: the decisions
here follow the federation rules ADR-137, ADR-144, ADR-146, and ADR-147
already fix, so the design question is settled by those decisions rather
than by a shipped increment. The engine change, the fixture corpus, and the
update to the `third-party-artifact-extensibility` design follow under the
`deterministic-substrate` roadmap (Tranche C).

## Category

Architecture

## Context

ADR-083 (revised, accepted) lets a corpus extend its artifact-type set with
one spec bundle pinned in `.decided/config.yaml`. Its decision 9 fixed the
first increment deliberately narrow: the effective registry is the invoking
corpus's own bundle, applied to every source in its federated closure, so an
inherited artifact whose type only a parent declares classifies as `unknown`
in the child, and the federation digest preimage is unchanged. Propagation
across the federation graph, and the adjudication of a name two sources
declare differently, were deferred to this decision.

The narrow increment contradicts what federation promises. ADR-140 applies
inherited decisions to child code and ADR-142 exports the inherited layer by
default; a parent that declares a `runbook` type and publishes runbooks
expects a child to read them as runbooks, enforce against them, and export
them, not to skip them as untyped documents. The federation graph composes
parents bottom-up with global source identity and no precedence (ADR-144,
ADR-146), and resolves genuine conflicts only through explicit,
Decision-backed overrides declared by the corpus that owns the replacement
(ADR-137, ADR-147). A type set that follows the same shape is the consistent
answer; a type set that ignores parents, or silently prefers the nearest
declaration, is not.

Two facts make the change smaller than it looks. A parent's bundle is already
pinned transitively: the federation digest frames the parent's raw
`.decided/config.yaml` bytes (ADR-134, ADR-145), and that config carries the
bundle's own SHA-256, so a parent cannot change its types without changing
the digest a child pinned. And the derived-cache generation key already
frames every verified parent's config bytes (ADR-148), so a parent re-pin
already invalidates the child's serving generation.

## Decision

1. **A child inherits its parents' admitted bundle types.** The effective
   registry of a corpus in a federated closure is: the built-ins in registry
   order; then the corpus's own admitted bundle elements in bundle order; then,
   for each direct parent in the canonical `parents` order the manifest loader
   fixes (sorted by source; the raw declaration order is authenticated but not
   semantic, so every consumer of the manifest reads the same order), that
   parent's effective registry beyond the built-ins, recursively. Composition
   is bottom-up, mirroring ADR-147: a node unions its parents' type sets, then
   adds its own. A name already present is skipped; an element whose admitted
   content is identical to the one already present is a silent duplicate.

2. **A same-name, different-content collision is a composition error.** Two
   sources declaring the same type name with different element content stop
   composition with `corpus-federation-artifact-type-conflict`, naming both
   sources, in the same failure class as a duplicate parent or a cycle
   (ADR-144). Nearest-wins, first-wins, and field merging are all rejected:
   silent precedence is exactly what ADR-146 refuses for artifacts.

3. **The only resolution is a Decision-backed type override.** A corpus may
   declare, in its `artifact_types` stanza, which source's element wins a
   named collision, with a live local Decision as rationale, in the ADR-137
   shape:

   ```yaml
   artifact_types:
     version: 1
     bundle:
       path: .decided/artifact-specs.json
       digest: sha256:...
     overrides:
       - name: runbook
         prefer: acme/standards
         rationale: APP-KWJ9D3C1S10N
   ```

   `prefer` is `local` or a global corpus source in the declaring corpus's
   inherited view; `rationale` must resolve to exactly one live local
   Decision, never an inherited or retired one; a name may be overridden at
   most once per corpus; an override for a name that does not collide is an
   error. The override is recorded policy that binds only the declaring corpus
   and its descendants, never precedence.

4. **The pin already covers it; no digest version bump.** Because a parent's
   config bytes, and therefore its bundle digest, are framed in the v1 and v2
   digests, inheriting types needs no new preimage and invalidates no existing
   pin. When the child composes, each parent's bundle is verified against the
   digest in that parent's captured config before any element is admitted;
   a mismatch is the parent-side `artifact-spec-bundle-digest-mismatch`,
   reported with the parent's source, and fails composition.

5. **Cache and serving keys follow the effective registry.** The local corpus
   hash folds every effective bundle digest in composition order, not only the
   invoking corpus's; the serving generation is already keyed to parent config
   bytes (ADR-148) and gains nothing new.

6. **Provenance is per element.** `validate --json` reports one
   `artifact_spec_bundles` entry per source that contributed types, each with
   its source, pin, admitted names, and warnings; the ADR-083 single
   `artifact_spec_bundle` key stays for the local corpus so existing readers
   are unchanged (ADR-007). MCP provenance stays bounded to what ADR-141
   already carries; artifacts carry source, types do not need to.

7. **Unchanged boundaries.** Inherited types are structural only, are not
   relationship targets, and add no edge kinds (ADR-055); a parent's bundle
   cannot override a built-in; `okf_type` follows the element, so a collision
   on it is the same collision as on the element.

## Consequences

A parent's knowledge model reaches its children whole: runbooks declared
upstream are runbooks downstream, enforced, searchable, and exported, with the
pin the child already holds proving which definition it trusts. The rules are
the federation's existing rules applied to one more kind of declaration, so
there is no second precedence model to learn or to get wrong.

The cost is that a type-name collision between parents is a hard stop until
someone records a decision. That is deliberate: two organisations calling
different shapes `policy` is a real disagreement, and the corpus that composes
them is the only place it can be settled on the record. The fixture and test
burden grows accordingly: composition tests with inherited types, identical
duplicates, conflicting duplicates, an override with and without a valid
rationale, and a parent bundle that fails its own digest.

Trade-offs accepted: the effective registry becomes closure-dependent, so a
child's `schema --list` differs from its parent's by design; the type-override
stanza is a second override surface beside `## overrides` in `.decided/corpus.md`,
kept in config because it governs the registry, not an artifact; and a
descendant cannot un-inherit a type short of overriding it.

## Alternatives Considered

- **Never propagate; a child re-declares what it needs (ADR-083 decision 9,
  kept).** Rejected: it makes a parent's typed artifacts silently untyped
  downstream, contradicting ADR-140 and ADR-142, and duplicates the bundle
  bytes in every child with no pin tying them together.
- **Nearest declaration wins.** Rejected: silent precedence, the thing ADR-146
  and ADR-137 exist to refuse; a child's typo would shadow a parent's contract
  without a trace.
- **Merge fields of same-name elements.** Rejected: a merged spec is one
  nobody wrote, reviewed, or pinned, and it makes classification depend on
  merge order.
- **A new digest version framing parent bundle bytes.** Rejected as
  unnecessary: the parent's config already pins its bundle digest, so the v1
  and v2 preimages cover the bytes transitively; a bump would invalidate every
  existing pin for no added assurance.
- **Type overrides in `.decided/corpus.md` beside artifact overrides.**
  Rejected: that manifest maps artifact identities; the registry is corpus
  configuration, and its pin lives in `config.yaml`, so its override belongs
  next to the pin.

## Related Decisions

- adr-083
- adr-007
- adr-055
- adr-134
- adr-137
- adr-140
- adr-141
- adr-142
- adr-144
- adr-145
- adr-146
- adr-147
- adr-148

## Related Requirements

- rac-growth-extensibility

## Related Designs

- third-party-artifact-extensibility
