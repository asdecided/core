---
schema_version: 1
id: RAC-KXS19RDVX4DJ
type: requirement
---
# Requirement: AsDecided Org Endpoint Wiring

> The key words MUST, MUST NOT, SHOULD, SHOULD NOT, and MAY in this document are
> to be interpreted as described in BCP 14 (RFC 2119, RFC 8174) when, and only
> when, they appear in all capitals.

## Status

Accepted

## Problem

The org grounding plane (ADR-117) needs every repository in a fleet wired to
the organisation's shared AsDecided endpoint, and hand-editing client JSON across
hundreds of repositories is exactly the per-repo tax the topology exists to
remove. The engine already emits client wiring at init time (ADR-088), but
only at creation and only for the local stdio server. Wiring the org
endpoint must be one command that is safe on a fresh repository, an
already-initialized repository, and a repository whose client configs the
user has edited by hand — without the engine ever taking ownership of a
file's other content.

## Requirements

- [REQ-001] `decided init` MUST accept an `--org-endpoint <url>` option. The URL MUST begin with `http://` or `https://`; any other value is a usage error and nothing is written.

- [REQ-002] With the flag, the engine MUST ensure an `asdecided-org` entry of the shape `{"type": "http", "url": <url>}` exists under `mcpServers` in both `.mcp.json` and `.cursor/mcp.json`, creating a file (with exactly that entry) when it is absent.

- [REQ-003] The flag MUST apply on a fresh init and on an already-initialized repository alike: org wiring is an explicit operator action (ADR-117), not creation-time configuration, and `.decided/config.yaml` is not touched by it on either path.

- [REQ-004] Merging into an existing client config MUST preserve every byte of meaning the user wrote outside the `asdecided-org` key: other servers, other top-level keys, and key order are retained; only the `asdecided-org` entry is added or updated. When the file exists with a different `asdecided-org` URL, the URL is updated — the operator named the endpoint explicitly.

- [REQ-005] The operation MUST be idempotent: a second run with the same URL writes no file and reports no file written.

- [REQ-006] A client config that cannot be parsed as a JSON object with a `mcpServers` mapping MUST produce a structured error naming the file, exit non-zero, and leave every target file unmodified — no partial writes.

- [REQ-007] Without the flag, `decided init` behaviour and output MUST remain byte-identical to the previous engine (ADR-007); the existing profile and init contracts are unchanged.

- [REQ-008] The `--json` contract MUST grow additively (ADR-007): an `org_endpoint` field (the URL, or null when the flag was absent) and the org-written files appended to `files_written`.

## Acceptance Criteria

- Fresh directory, `decided init --org-endpoint https://asdecided.example.com/mcp`:
  both client configs exist and carry exactly the `asdecided-org` HTTP entry;
  `files_written` lists both.
- Already-initialized repository: the same command adds the entry to both
  files, reports `created: false`, and leaves `.decided/config.yaml` untouched.
- A hand-written `.mcp.json` with its own servers gains the `asdecided-org` key
  and loses nothing; a differing `asdecided-org` URL is updated in place.
- Re-running with the same URL is a no-op: no files written, none reported.
- An unparseable `.mcp.json` yields a structured error, a non-zero exit,
  and unmodified files.
- `decided init` without the flag is covered by the existing batteries
  unchanged, and `--profile` composes with `--org-endpoint` (local `asdecided`
  and `asdecided-org` entries side by side).

## Success Metrics

- Wiring a repository to the org endpoint is one command in every repo
  state the fleet actually contains, so org rollout reduces to repo-template
  work plus one command per existing repository.

## Risks

- Merging rewrites a file's formatting (JSON re-serialisation) even though
  content is preserved; mitigated by documenting the normalisation and
  writing only when the parsed content actually changes.
- Divergent client-config dialects could invalidate the single emitted
  shape; mitigated by pinning the two targets ADR-088 already owns and
  treating any further client as its own decision.

## Assumptions

- `{"type": "http", "url": …}` is a valid server entry for both target
  clients' streamable-HTTP support, as documented on the Org Grounding
  page.
- The org endpoint itself is operated under ADR-098's posture; this
  requirement covers client wiring only.

## Related Decisions

- adr-007
- adr-088
- adr-098
- adr-117
- adr-132

## Related Roadmaps

- org-grounding-plane

## Related Requirements

- rac-mcp-http-transport
