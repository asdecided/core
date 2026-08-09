---
schema_version: 1
id: RAC-KWK9FB1J218M
type: requirement
---
# Requirement: Explain-Miss Diagnostics

## Status

Accepted

Classification: `[external]` — an author-facing diagnostic surface.
Initiative 1 of the `retrieval-diagnostics` roadmap.

## Problem

Lore has explain-*hit* — the explainable-retrieval evidence shows the
winning field, matched terms, and tier for results that surfaced — but no
way to ask "why did my query *not* surface artifact X." For an author
maintaining a corpus, the absence is often the more useful question: a
missing result could mean no term hit a tier, a term matched nothing, or
the budget truncated it, and today the author can only guess which.

## Requirements

- [REQ-001] Given a query and a named target artifact, the diagnostic MUST report deterministically why the target did not rank or did not match: at minimum distinguishing "no query term matched any tier of the target", "terms matched but the target ranked below surfaced results", and "the target ranked but the response budget truncated it".
- [REQ-002] The diagnosis MUST be a pure trace over the existing matcher, tokeniser (ADR-037), and tiers (ADR-038): the same match and ranking path that produced the result set, never a parallel search implementation.
- [REQ-003] The diagnostic MUST be scoped to a named target artifact — "why did *this* not surface" — never an open enumeration of everything that did not match.
- [REQ-004] Output MUST be deterministic and offline (ADR-002, ADR-066): the same corpus bytes and query yield the same diagnosis, with no embeddings and no model.
- [REQ-005] The surface MUST be additive (ADR-007): explain-miss output is new; existing search responses, result sets, and ranking are byte-identical to pre-change behaviour.
- [REQ-006] The diagnosis MUST explain the existing behaviour, never change it: no artifact matches or ranks differently because diagnostics exist.

## Acceptance Criteria

- For a fixture query that fails to surface a known artifact, the diagnostic
  names the deterministic reason, reproducibly across runs.
- Each distinct miss cause (no tier hit, outranked, budget-truncated) is
  exercised by a fixture and reported distinctly.
- Search goldens are unchanged: identical result sets and ordering before and
  after, for the same corpus and queries.
- The diagnostic requires a named target; invoking it without one is a usage
  error, not a corpus-wide dump.

## Success Metrics

- An author can answer "why didn't it find X" from the diagnostic alone,
  without reading matcher source — the deterministic complement to
  explain-hit.

## Risks

- Over-reporting trivial non-matches drowns the useful signal. Mitigation:
  REQ-003 scopes the question to one named target.
- The trace drifts from the real matcher as ranking evolves. Mitigation:
  REQ-002 requires the diagnosis to run over the same path, not a copy.

## Assumptions

- "Why didn't it find X" is a common authoring question once a corpus is
  large enough for relevance ranking to matter — the trigger that has now
  fired with ADR-078 shipped.
- The existing explain-hit evidence gives the vocabulary (field, terms,
  tier) that miss reporting can reuse.

## Related Decisions

- adr-002
- adr-007
- adr-037
- adr-038
- adr-066
- adr-078

## Related Roadmaps

- retrieval-diagnostics

## Related Requirements

- rac-explainable-retrieval
- rac-floor-ratio-boost-gate

## Verification

- Native unit and CLI tests cover no-term, partial-term, outranked,
  result-window-truncated, and named-target-required behaviour.
- The diagnostic calls the same matcher and ranking function as `find` and is
  additive to the existing search contract.
