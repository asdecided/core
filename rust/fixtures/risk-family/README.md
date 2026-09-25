The Risk family fixture (ADR-151). This file is an untyped document.

- `risks/vendor-lock-in.md` — a valid Risk linked from a decision, a
  requirement, a prompt, and a design through `## Related Risks`.
- `risks/bad-status.md` — `invalid-risk-status`.
- `risks/missing-impact.md` — no frontmatter; classifies as Risk and fails
  with `missing-impact`.
- `risks/retired.md` — a Deprecated Risk: the requirement's link to it is
  `relationship-target-superseded`, and its own `## Related Prompts` is
  `relationship-edge-unsupported` (Risk does not declare it).
- `decisions/`, `designs/` — carry a prose `## Risk` heading and still
  classify as their own type.
