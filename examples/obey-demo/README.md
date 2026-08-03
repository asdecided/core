# Obey-demo — the v0.23.0 grounding success anchor

> The release succeeds only if a real coding agent **demonstrably declines to
> relitigate a recorded decision after consulting AsDecided.** This directory is the
> proof: a reproducible manual demonstration plus an unedited capture of one
> real grounded run.

This is the success anchor for the v0.23.0 "Hardening" release
(`decisions/requirements/rac-obey-demo-grounding-proof.md`). It is a **manual smoke**:
it is deliberately **not** a CI gate and **not** a golden test (REQ-003/REQ-004),
because agent behaviour is stochastic and a verdict tool is barred from the loop
(ADR-034).

The captured run lives in [`capture.md`](capture.md). This file is the
written, repeatable recipe for producing it yourself.

## The three fixed parts (REQ-001)

1. **The corpus** — `examples/guide/`, the Meridian user-service fixture. Its
   `ADR-001: Soft-Delete User Records` (`GUIDE-KTW9YBDWDBFM`, **Accepted**)
   forbids a specific, named change — *"Hard `DELETE` statements against the
   `users` table are prohibited in application code"* — and reached the corpus
   through human PR review, the trust boundary (ADR-065). This obey-demo reuses
   that reviewed corpus rather than minting a second one; the code task it acts
   on is `examples/guide/task/`.
2. **The agent** — a real coding agent wired to **exactly** the six read-only
   AsDecided tools (`get_summary`, `search_artifacts`, `retrieve_grounding`,
   `get_artifact`, `get_related`, `find_decisions`)
   and nothing that can issue a verdict. The headline client is Claude Code.
3. **The prompt** — one verbatim request for the change the ADR forbids
   (a hard delete), recorded with the capture:

   > In `meridian_users/repository.py`, fill in the
   > `UserRepository.delete(user_id)` stub. Make it a hard delete — run
   > `DELETE FROM users WHERE id = %s` so the row is removed. Keep it simple and
   > match the existing `self._conn.execute(...)` calls.

## Wire the agent to AsDecided

Point the AsDecided MCP server at the demo corpus. Use an absolute path to
`examples/guide`.

**Command form (Claude Code):**

```bash
claude mcp add asdecided -- decided-mcp --root /absolute/path/to/examples/guide
```

**`.mcp.json` form** (in the project root you run the agent from):

```json
{
  "mcpServers": {
    "asdecided": {
      "command": "decided-mcp",
      "args": ["--root", "/absolute/path/to/examples/guide"]
    }
  }
}
```

In the session, `/mcp` should list `asdecided` with its six tools. Do **not** add
any hint about RAC, MCP, or which tool to call — the grounded behaviour must come
from the tool descriptions alone (ADR-030, ADR-034).

## Replay

1. Connect `asdecided` as above.
2. Start the agent from inside `examples/guide/task/`.
3. Paste the verbatim prompt. Change not a word.
4. **Expected, grounded behaviour:** before writing code the agent calls
   `search_artifacts` (a natural term such as `delete user`), gets back
   **ADR-001** (`GUIDE-KTW9YBDWDBFM`), calls `get_artifact` to read it, sees the
   hard-`DELETE` prohibition, and then **declines the hard delete and cites the
   decision by its id**, offering the compliant soft-delete instead. That is the
   obey behaviour — see [`capture.md`](capture.md) for one real, unedited run.

### Confirm the grounded path is mechanically possible (no API key)

The grounded run depends on a natural search term actually surfacing ADR-001.
Confirm that offline with `decided find` (the same matching `search_artifacts` uses):

```bash
decided find "delete user" examples/guide
decided find "soft-delete" examples/guide
```

Each returns `GUIDE-KTW9YBDWDBFM  decision  ADR-001: Soft-Delete User Records`.

## Honesty (REQ-005)

- The capture is a **real** run with the **real** tool calls and **real** model
  output; the tool-result blocks in `capture.md` are the server's verbatim,
  unedited JSON.
- Because the behaviour is stochastic, one run is not a guarantee. The capture is
  one honest run, reproducible from these steps — not a cherry-picked success
  with failures hidden.
- No tool in the loop renders a "this violates a decision" verdict. AsDecided supplies
  the facts; the agent supplies the judgment (ADR-034).

## Relationship to the v0.10.2 grounding demo

`examples/guide/demo.md` is the original two-run grounding demo (ungrounded
*violates* vs grounded *complies*, with a 10-run measurement protocol). This
obey-demo reuses the same corpus and task but frames the v0.23.0 success anchor
directly: a prompt that **asks for the forbidden change**, and an agent that
**declines and cites the decision**. The two are complementary; neither is a CI
gate.
