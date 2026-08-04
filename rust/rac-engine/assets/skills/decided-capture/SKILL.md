---
name: decided-capture
description: Capture a NEW decision or requirement from a conversation (an interview) into ONE valid AsDecided artifact — you interview and propose, the human ratifies, `decided validate` closes, and promotion into the trusted corpus is by pull request reviewed by someone other than the author. Use when a user wants to record a fresh decision or requirement into AsDecided (a project's decisions/ directory) by talking it through rather than importing a document. For an existing document use decided-import; for bulk conversion use an external ingestion connector.
---

# RAC interview capture

Turn a conversation into one valid RAC artifact. You interview the author and
propose; the human ratifies; `decided validate` is the deterministic check; an
independent reviewer promotes it by pull request. This skill never adds AI to the
AsDecided core — it runs in the coding agent and uses the `decided` CLI.

## Hard constraints

- **Capture knowledge, not work.** Record a decision or a long-lived requirement
  and its rationale — never owners, assignees, sprints, priorities, or due dates.
  If the conversation is about *who does what by when*, that belongs in the team's
  work tracker, not in RAC; say so and stop.
- **The schema is not yours to invent.** Read the real shape with
  `decided schema <type>` (and `decided schema` for the recognised types). Use the real
  section names, the real artifact types, and the real `## Related <Type>`
  relationship sections. Never guess or hard-code a field, type, or relationship
  kind. If you cannot run `decided schema`, stop and say so.
- **No invention.** Capture only what the author actually tells you. Where a
  required section has no material, **ask a question** — do not fill it with
  plausible-sounding text. The interview gathers facts; it does not author them.
- **Human ratification is mandatory and explicit (before any file is written).**
  Present the draft and require the user to confirm or correct **(a) the artifact
  type, (b) the title, and (c) each relationship** before you write anything.
  Relationships are *suggestions to confirm*, never silently asserted. The `id` is
  minted by `decided new` (opaque, system-assigned) — never hand-write or choose it.
- **Close on deterministic validation.** After writing, run `decided validate`. If it
  fails, show the errors, offer to fix them, and re-validate. Never leave an
  invalid artifact behind.
- **Save is a draft commit; promotion is a pull request.** A fresh capture is
  *untrusted* until a human reviews it. Write the artifact, optionally commit it as
  a draft on a branch when the user confirms, but **never write straight to the
  trusted corpus, never auto-commit without confirmation, and never self-merge.**
  The trust boundary is review and merge by **someone other than the author**.

## 1. Capture the raw intent first

Let the author say it their way before you ask anything — paste or dictate the
decision in plain language. Do not gate capture behind your questions; collect the
brain-dump, then structure it. If they describe several distinct decisions, capture
one artifact at a time and offer to repeat for the rest.

## 2. Choose the type and read its real schema

Decide *with the author* whether this is a decision, requirement, design, roadmap,
or prompt (most captures are a decision or a requirement). Then read the actual
contract:

```bash
decided schema                     # the recognised artifact types
decided schema decision            # required / recommended / optional sections, and
                               # any controlled values (e.g. Status, Category)
decided schema decision --json     # the same, machine-readable
```

Map the captured intent onto *these* section names. Do not introduce sections the
schema does not define.

## 3. Interview to fill the template

Ask only the **two to four** things you cannot infer from the brain-dump or safely
default — and ask them as **confirmations, pre-filled with your best reading**, not
as blank prompts. Keep each question short and let the author skip or accept. Good
targets: the precise decision/requirement statement, the context that forced it,
the consequences or rationale, and a `## Status` value (use one of the controlled
values `decided schema` lists). Requirements are testable `- [REQ-001] ...` lines under
`## Requirements`; prefer normative wording (MUST / SHOULD / MAY), and avoid vague
verbs (support, handle, allow, enable) that `decided validate` warns on.

## 4. Dedupe, then draft

Before drafting, check whether the team already recorded this, so capture does not
create a duplicate:

```bash
decided find "<key words from the decision>" decisions/
```

If a close match exists, surface it and ask whether to update that artifact
(via `decided-import` / a normal edit) instead of creating a new one. Otherwise, draft
the content under the type's real headings, keeping the author's own wording where
it fits. For each **required** section with no captured material, leave a clearly
marked gap to raise with the user — do not invent.

## 5. Review gate — confirm before writing

Present, in the conversation (not as a file yet):

- the **draft** artifact;
- a short summary: the **chosen type**, the **proposed title**, and any
  **relationships the author named** (never relationships inferred by scanning the
  repository — that is out of scope);
- every **gap** where a required section had no material.

Ask the user to confirm or correct the type, the title, and each relationship.
Verify any relationship target actually resolves before proposing it:

```bash
decided resolve <id> decisions/
decided find "<text>" decisions/
```

Do not write a file until the user has confirmed.

## 6. Write, then validate

On confirmation, scaffold the file (this mints the id and writes the canonical
template — it never overwrites an existing file), then replace the template body
with the confirmed content:

```bash
decided new decision decisions/decisions/<slug>.md
```

Write artifacts only inside the host project's decisions directory (`decisions/` by default;
confirm the path if the project keeps them elsewhere) under the matching subfolder
(`decisions/decisions/`, `decisions/requirements/`, `decisions/roadmaps/`, `decisions/prompts/`,
`decisions/designs/`). If the project has no `.decisions/config.yaml`, run `decided init` once at
the project root first. Keep the `##` headings and the frontmatter block intact;
never edit the minted `id`.

Then close on validation:

```bash
decided validate decisions/decisions/<slug>.md
```

Treat errors as blocking — show them, offer fixes, and re-run until it exits 0.
`decided improve <file> --template` prints section stubs when the content exists to
fill a missing recommended section. If the artifact links to others, finish with
`decided relationships decisions/ --validate`.

## 7. Hand off — draft commit, promote by pull request

A validated capture is a **draft**, not yet trusted knowledge. With the user's
go-ahead, commit it on a branch (a draft), but leave the promotion to the team's
normal pull-request flow:

- the author's confirmation in this conversation is a *fidelity* check — "we
  captured what you meant" — **not** ratification into the record;
- the **trust boundary** is a pull request reviewed and merged by **someone other
  than the author** (the project's required-review gate), which is also where
  `decided validate` / `decided relationships --validate` run as checks.

Never open-and-self-approve, and never merge the author's own capture for them.

## Out of scope

- Reformatting an **existing document** into an artifact — use `decided-import`.
- Bulk or batch conversion, directory crawling, or "migrate my whole wiki" — use
  an external AsDecided ingestion connector.
- Inferring relationships by scanning the repository — only the links the author
  names, each confirmed.
- Inventing context, consequences, rationale, or requirements the author did not
  provide.
- Auto-committing without the human-review step, or merging a capture into the
  trusted corpus without independent review.
- Recording ownership, assignment, scheduling, or any work-tracking field.
