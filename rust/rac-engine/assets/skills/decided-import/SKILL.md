---
name: decided-import
description: Reformat ONE existing Markdown document (a decision, requirement, design, roadmap, or prompt) into ONE valid AsDecided artifact, with a mandatory human-review step before any file is written and `decided validate` as the deterministic close. Use when a user wants to add or import a single existing document into AsDecided (a project's decisions/ directory). Single-document only — for rich documents or bulk conversion use an external AsDecided ingestion connector.
---

# RAC single-document import

Reshape one existing document into one valid RAC artifact. You propose; the
human ratifies; `decided validate` is the deterministic check. This skill never
adds AI to the AsDecided core — it runs in the coding agent and uses the `decided` CLI.

## Hard constraints

- **One document in, one artifact out.** If asked to import a directory, a
  whole wiki, or several documents at once, stop and explain that this skill
  handles exactly one Markdown document → one artifact by design. For rich
  documents or bulk conversion, point the user to an external AsDecided
  ingestion connector.
- **The schema is not yours to invent.** Read the real shape with
  `decided schema <type>` (and `decided schema` for the list of recognised types).
  Use the real section names, the real five artifact types, and the real
  `## Related <Type>` relationship sections. Never guess or hard-code a field,
  type, or relationship kind. If you cannot run `decided schema`, stop and say so.
- **Human review is mandatory and explicit (before any file is written).**
  Present the draft and require the user to confirm or correct **(a) the
  artifact type, (b) the title, and (c) each relationship** before you write
  anything. Relationships are *suggestions to confirm*, never silently
  asserted. The `id` is minted by `decided new` (opaque, system-assigned) — never
  hand-write or choose it; show it after.
- **Close on deterministic validation.** After writing, run `decided validate`.
  If it fails, show the errors and offer to fix them, then re-validate. Never
  leave an invalid artifact behind.
- **No invention.** Reformat only what the source says. Do not invent context,
  consequences, rationale, or requirements. Where a required section has no
  source material, flag the gap and ask the user — do not fill it with
  plausible-sounding text.

## 1. Get the source

Ask the user for the one document — pasted text or a file path — and, if it is
not obvious, what kind of decision or requirement it represents. If they offer
more than one document or a directory, apply the single-document constraint
above before going further.

## 2. Get Markdown (if the source is rich text)

The native engine does not ingest DOCX, PDF, HTML, PPTX, or XLSX. Ask the user
to run an external AsDecided ingestion connector and provide the resulting
Markdown for review. Pasted Markdown needs no conversion. This skill never
installs Python packages or silently changes the source document.

## 3. Choose the type and read its real schema

Pick the type with the user (or cross-check with `decided inspect <file>`, which
infers type from `##` headings and reports confidence). Then read the actual
contract:

```bash
decided schema                     # the recognised artifact types
decided schema decision            # required / recommended / optional sections, and
                               # any controlled values (e.g. Status, Category)
decided schema decision --json     # the same, machine-readable
```

Map the source onto *these* section names. Do not introduce sections the schema
does not define.

## 4. Draft the artifact

Reshape the source content under the type's real headings. Keep the author's
own wording where it fits. For each **required** section the source does not
cover, leave it clearly marked as a gap to raise with the user — do not invent
content. (Requirements use testable `- [REQ-001] ...` lines under
`## Requirements`; a `## Status` value, when present, must be one of the
controlled values `decided schema` lists for that type.)

> Normative language: writing requirements with BCP-14 keywords (MUST / SHOULD /
> MAY) is good practice, but note that `decided validate` does not yet enforce them.
> It does warn on vague verbs (support, handle, allow, enable) in requirements.

## 5. Review gate — confirm before writing

Present, in the conversation (not as a file yet):

- the **draft** artifact;
- a short summary: the **chosen type**, the **proposed title**, and any
  **relationships the source explicitly names** (never relationships inferred by
  scanning the repository — that is out of scope);
- every **gap** where a required section had no source material.

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
confirm the path if the project keeps them elsewhere) under the matching
subfolder (`decisions/decisions/`, `decisions/requirements/`, `decisions/roadmaps/`,
`decisions/prompts/`, `decisions/designs/`). If the project has no `.decisions/config.yaml`, run
`decided init` once at the project root first. Keep the `##` headings and the
frontmatter block intact; never edit the minted `id`.

Then close on validation:

```bash
decided validate decisions/decisions/<slug>.md
```

Treat errors as blocking — show them, offer fixes, and re-run until it exits 0.
`decided improve <file> --template` prints section stubs when the content exists to
fill a missing recommended section. If the artifact links to others, finish with
`decided relationships decisions/ --validate`.

## Out of scope

- Bulk or batch import, directory crawling, or "migrate my whole wiki" (one
  Markdown document → one artifact only; use an external ingestion connector
  for broader conversion).
- Inferring relationships by scanning the repository — only the links the source
  document itself names, each confirmed by the user.
- Auto-committing the new artifact without the human-review step.
- Inventing facts, context, or rationale not present in the source.
