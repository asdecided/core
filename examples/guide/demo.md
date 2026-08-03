# AsDecided — grounding demo

This demo proves one claim:

> An agent connected to RAC respects a recorded decision that an unconnected
> agent violates.

It runs the same coding task twice against the same client — once with no MCP
server, once with AsDecided connected — and shows the behavioural difference.
A stranger can reproduce it from this file alone.

The demo is specified by `decisions/designs/guide-grounding-demo.md` and implements
`decisions/roadmaps/v0.10.x-guide/v0.10.2-guide-grounding-demo.md`. The headline
client is **Claude Code** (best MCP tool-calling reliability of the target
clients at the time of design).

## The scenario

The corpus in `examples/guide/decisions/` records a real-shaped decision for a
fictional user-management service called **Meridian**:

- **ADR-001 — Soft-Delete User Records** (`GUIDE-KTW9YBDWDBFM`): user rows are
  never hard-deleted from the `users` table; closure sets a `deleted_at`
  timestamp. Hard `DELETE` against the `users` table is prohibited in
  application code, for GDPR and audit reasons.
- a connected requirement (`GUIDE-KTW9YBE1WHA4`), design
  (`GUIDE-KTW9YBDZAY9F`), and roadmap (`GUIDE-KTW9YBE3CX84`).

The code task runs against the small service slice in
[`task/`](task/) (see [`task/README.md`](task/README.md)). The repository layer
already creates, reads, and lists users; the one missing method is account
deletion (`UserRepository.delete`, raising `NotImplementedError`).

## The task prompt (identical for both runs)

Use this prompt verbatim for both runs. Do not add any hint about RAC, MCP, or
which tool to call — the grounded run must succeed on the tool descriptions
alone (ADR-030, ADR-034). Run both from inside `examples/guide/task/`.

> Implement `UserRepository.delete(user_id)` in `meridian_users/repository.py`
> so that `DELETE /users/{id}` closes a user account. Match the patterns
> already in the repository.

That is the whole prompt. It is the natural request a teammate would make. An
uninformed implementer fills the stub with a hard `DELETE FROM users` — the
obvious thing, not a trap.

---

## Run 1 — ungrounded (no MCP server)

**Setup.** A clean Claude Code session with **no** AsDecided server configured.
If you have previously added it, remove it for this run:

```bash
claude mcp remove asdecided
```

Confirm it is gone:

```bash
claude mcp list        # asdecided must not appear
```

**Run.** From `examples/guide/task/`, start Claude Code and paste the task
prompt above.

**Expected observable behaviour.** The agent has no recorded knowledge to
consult. It reads the existing repository code, infers a conventional deletion,
and writes a hard delete into the stub — something equivalent to:

```python
def delete(self, user_id: str) -> None:
    self._conn.execute(
        "DELETE FROM users WHERE id = %s",
        (user_id,),
    )
```

This is a correct-looking implementation that **violates ADR-001**. The agent
never had the decision in front of it, so it could not honour it.

---

## Configure the client (between runs)

Add AsDecided to Claude Code, pointed at the demo corpus. This is the exact
config block from the README Guide section and `docs/mcp.md`; only `--root`
changes to the demo corpus.

**Command form:**

```bash
claude mcp add asdecided -- decided-mcp --root /absolute/path/to/examples/guide
```

**`.mcp.json` form** (in the project root):

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

Confirm the server is connected before Run 2:

```bash
claude mcp list        # asdecided must appear
```

In a Claude Code session, `/mcp` should list `asdecided` with its six tools:
`get_summary`, `search_artifacts`, `retrieve_grounding`, `get_artifact`,
`get_related`, `find_decisions`.

---

## Run 2 — grounded (AsDecided connected)

**Setup.** A fresh Claude Code session with `asdecided` connected (above),
started from `examples/guide/task/`.

**Run.** Paste the **same** task prompt. Do not change a word; do not mention
RAC or the tools.

**Expected observable behaviour.** Before writing code, the agent calls
`search_artifacts` with a natural keyword (for example `delete user`,
`delete`, or `soft-delete`) because the tool description tells it to search
recorded decisions before implementing. The search returns
`ADR-001: Soft-Delete User Records` (`GUIDE-KTW9YBDWDBFM`). The agent calls
`get_artifact` to read it, sees the hard-`DELETE` prohibition, and then:

- **cites the decision by its identifier** — `ADR-001` /
  `GUIDE-KTW9YBDWDBFM` — in its response, and
- writes a **compliant** soft-delete implementation, something equivalent to:

```python
def delete(self, user_id: str) -> None:
    self._conn.execute(
        "UPDATE users SET deleted_at = %s WHERE id = %s AND deleted_at IS NULL",
        (datetime.now(UTC), user_id),
    )
```

(plus the `from datetime import UTC, datetime` import). The agent
respected a decision it was never told about, because Guide served it the fact
at the moment of implementation.

That difference — Run 1 violates, Run 2 cites and complies — is the demo.

### Verify the grounded path is mechanically possible

The grounded run depends on a natural search term actually surfacing ADR-001.
You can confirm that without an API key, using `decided find` (the same matching
`search_artifacts` uses):

```bash
decided find "delete user" examples/guide
decided find "delete" examples/guide
decided find "soft-delete" examples/guide
```

Each returns `GUIDE-KTW9YBDWDBFM  decision  ADR-001: Soft-Delete User Records`.
A raw MCP client (the `mcp` Python SDK over stdio) against
`decided-mcp --root examples/guide` returns the same match from `search_artifacts`,
and `get_artifact` / `get_related` return the decision with its full content
and its connected requirement, design, and roadmap. This is pinned by the test
suite (`tests/test_dogfood.py`, the `guide demo searchability` tests).

---

## Measurement protocol

Agent behaviour is stochastic, so the grounded behaviour is measured, not
asserted from a single run.

- **Runs:** 10 scripted grounded runs (Run 2 setup), same verbatim prompt each
  time, fresh session each time.
- **Pass condition:** the agent cites the correct decision identifier in **at
  least 8 of 10** runs.
- **What counts as a citation:** the response names the decision's
  **identifier** — `ADR-001` or `GUIDE-KTW9YBDWDBFM`. Naming only the topic
  ("there's a soft-delete policy") does **not** count.
- **Re-run trigger:** the measurement is re-run before release and after **any**
  change to the six tool descriptions (the design contract in
  `decisions/designs/guide-tool-surface.md`). A miss feeds back into description text,
  never into the prompt.

### Run-log template (fill when the measurement is executed)

Copy this block into the release notes or an accompanying note and complete it
for the release. It is intentionally blank here — these numbers must come from
real runs, not be pre-filled.

```text
AsDecided grounding measurement
-------------------------------
Date:               <YYYY-MM-DD>
Client:             Claude Code <version>
Model:              <model id>
Corpus:             examples/guide  (commit <sha>)
Prompt:             see demo.md "The task prompt" (verbatim)

Run  | search called? | cited ADR-001 / GUIDE-KTW9YBDWDBFM? | compliant impl?
-----+----------------+-------------------------------------+----------------
 1   |                |                                     |
 2   |                |                                     |
 3   |                |                                     |
 4   |                |                                     |
 5   |                |                                     |
 6   |                |                                     |
 7   |                |                                     |
 8   |                |                                     |
 9   |                |                                     |
10   |                |                                     |

Citations: __ / 10     Pass threshold: 8 / 10     Result: PASS / FAIL
Notes:
```

---

## Recording shot list (at most 90 seconds)

One contrast pair, real time, no edits beyond trimming, real tool calls
visible. Shot order per `decisions/designs/guide-grounding-demo.md`:

1. **The task prompt** — show the verbatim prompt once, on screen long enough
   to read.
2. **Ungrounded violation (compressed)** — Run 1: the agent produces the hard
   `DELETE FROM users`. Trim the agent's thinking; keep the resulting diff
   visible.
3. **The config block** — show adding `asdecided` (the `claude mcp add` line or
   the `.mcp.json` block), then `/mcp` listing the six tools.
4. **Grounded run** — Run 2: show the `search_artifacts` tool call and its
   result, and the agent's response **citing `ADR-001` / `GUIDE-KTW9YBDWDBFM`**.
   The tool call and the citation must both be on screen.
5. **The compliant diff** — the soft-delete `UPDATE` implementation, small
   enough to read without pausing.

### Accessibility

- Each phase carries an on-screen label or caption ("Without RAC", "Configure",
  "With RAC").
- The demo script (this file) is fully usable without the video.
- Transcript and diff text shown in the recording must be legible at common
  embed sizes — use a large terminal font and high contrast.

---

## Release-tail steps (human-only — NOT done by this milestone)

These steps require a live agent client, an API key, and publishing access.
They cannot be produced mechanically and must be completed by the maintainer
before the release is announced. None of them is faked here.

- [ ] **Execute the measurement.** Run the 10 scripted grounded runs above and
      complete the run-log template. Gate: ≥ 8/10 correct decision-ID
      citations. If it fails, revise tool description text per the
      `guide-tool-surface` contract and re-run.
- [ ] **Record the contrast pair.** Produce the ≤ 90-second recording per the
      shot list, with the accessibility labels. It must exist before the
      announcement and ships as a release asset.
- [ ] **Submit to registries.** File the server to the GitHub MCP Registry and
      at least one community registry or curated list.
- [ ] **Announce the release** with the demo recording as the headline, not the
      feature list.
