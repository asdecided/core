# Repository Workflow

RAC runs against any directory of Markdown files, but it shines when a repository
gives its product knowledge an intentional home. This page describes the convention
RAC itself uses.

## The `decisions/` knowledge directory

Collect your artifacts under a top-level `decisions/` directory, grouped by type:

```text
decisions/
  requirements/   # what needs to exist
  decisions/      # ADRs — why choices were made
  designs/        # product experience thinking
  prompts/        # reusable AI collaboration patterns
  roadmaps/       # where the product is heading
  assets/         # supporting images and files
```

The directory layout is a convention, not a requirement — RAC classifies each file by
its [section headings](artifacts.md#how-classification-works), not its folder. Grouping
by type simply keeps a growing corpus navigable and makes `stats`/`portfolio` output
easy to read.

### Three documentation layers

RAC's own repository separates concerns into three layers
([ADR-022](https://github.com/itsthelore/asdecided-core/blob/main/decisions/decisions/adr-022-documentation-boundaries.md)):

- **`README.md`** — the front door: what RAC is and how to try it.
- **`docs/`** — user-facing guides (this directory).
- **`decisions/`** — RAC's internal, structured product knowledge.

`decisions/` is the corpus RAC manages; `docs/` is documentation *for people*. Keep them
distinct: users shouldn't need to read internal roadmaps or ADRs to be productive.

## Naming

- Requirements / prompts / designs: a descriptive slug — `login-flow.md`.
- Decisions: `adr-NNN-slug.md` — `adr-001-markdown-first.md`.
- Roadmaps: `vX.Y.Z-slug.md` — `v0.7.6-document-structure.md`.

See [artifacts.md](artifacts.md) for the sections each type expects.

## Everyday commands

Run these from the repository root:

```bash
decided validate decisions/                 # validate every recognized artifact in the tree
decided stats decisions/                    # counts, quality signals, per-type breakdown
decided relationships decisions/ --validate # check that cross-artifact references resolve
decided review decisions/                   # all of the above as one prioritized worklist
decided portfolio decisions/                # one-screen health summary + attention list
decided index decisions/ --json             # flat inventory for tools, CI, and agents
```

To check a single file as you edit it:

```bash
decided validate decisions/requirements/login-flow.md
decided inspect  decisions/requirements/login-flow.md
```

## In review and CI

Because everything is Markdown in Git, documentation and artifacts move through the
same pull-request workflow as code. A natural pre-merge check is `decided review decisions/`
— it validates every artifact, resolves every reference, and exits `1` if anything
blocking is found, so reviewers see whether new or edited artifacts are complete
and their links still resolve. RAC runs exactly this gate over its own `decisions/`
corpus in CI. See [testing.md](testing.md) for the contributor verification
workflow.
