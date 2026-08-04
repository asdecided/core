---
name: decided-artifacts
description: Author and maintain AsDecided Markdown artifacts — requirements, decisions, roadmaps, prompts, designs — using the decided CLI. Use when asked to create, read, validate, update, or link artifacts in a project's decisions/ directory.
---

# RAC artifacts

RAC models product artifacts as typed, deterministic Markdown files. Five
types exist: requirement, decision, roadmap, prompt, design. Type is
inferred from `##` section headings, never declared. Frontmatter carries
identity only and is machine-generated.

## Hard constraints

- Write artifact files only inside the host project's decisions directory
  (`decisions/` by default; if the project keeps artifacts elsewhere, confirm
  the path before writing). Never create or edit RAC artifacts outside
  that directory, and never modify files elsewhere in the project on
  this skill's behalf.
- Never hand-write or alter an artifact `id` or its frontmatter
  identity block. `decided new` mints ids. Do not edit `.decisions/config.yaml`.
- Do not invent sections or frontmatter fields. Use `decided schema <type>`
  to see what a type expects.
- Validation must pass before the work is done: `decided validate` exits 0
  and, if the project uses relationship links,
  `decided relationships <dir> --validate` exits 0.

## Create an artifact

```bash
decided new requirement decisions/requirements/<slug>.md
```

`decided new <type> <path>` writes the canonical template with a minted id.
It never overwrites an existing file. Then edit the file and replace
every TODO placeholder with real content, keeping the `##` headings
intact. Requirements use testable statements of the form
`- [REQ-001] ...` under `## Requirements`.

If the project has no `.decisions/config.yaml` yet, run `decided init` once at the
project root first (optionally `--key <PREFIX>` for the id prefix).

## Read and classify

```bash
decided inspect <file>          # type, confidence, present/missing sections
decided schema                  # list registered types
decided schema <type>           # sections for one type; --template prints a starter
```

An invalid but recognisable file still classifies as its type and then
fails validation — classification and validation are separate.

## Validate

```bash
decided validate <file-or-dir>              # structural checks; exit 0 = pass
decided relationships <dir> --validate      # link integrity across the corpus
```

Treat errors as blocking. Warnings are advisory (commonly a missing
recommended section); fix them when the content exists to fill them.

## Update and improve

```bash
decided improve <file>          # missing required/recommended sections, with prompts
```

Edit the Markdown directly, preserving the heading structure and the
frontmatter block untouched. Re-run `decided validate` after every edit.

## Link artifacts

Linking uses `## Related <Type>` sections (for example
`## Related Decisions`), one artifact id per line. Ids resolve from an
explicit `## ID` section, a `<letters>-<digits>` filename prefix (for
example `adr-004`), or the filename stem. Check a link target resolves
before adding it:

```bash
decided resolve <id> <dir>
decided find <text> <dir>
```

After adding links, run `decided relationships <dir> --validate`.

## Output for automation

Most commands accept `--json` for machine-readable output, and exit
codes follow the documented contract (0 pass, non-zero failure). Prefer
`--json` when a result feeds a script or a decision.
