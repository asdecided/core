# Core — Enterprise Deployability Council Review (2026-08)

> Method: a six-seat review council — GTM strategist, Tech Evangelist,
> Contrarian, Engineering Leadership, Design Partner (staff platform engineer
> at a regulated ~5,000-engineer enterprise), and CISO — each ran an
> independent, read-only review of this repository at commit `8abe4a2`
> (v0.26.2 line, 2026-08-03) under a supervising chair. Findings were graded
> against named baselines: SLSA v1.0, OpenSSF Scorecard, NIST SSDF /
> SP 800-92 / SP 800-190, OWASP ASVS and the OWASP Top 10 for LLM
> Applications, SemVer and Keep a Changelog, Diátaxis, SIG/CAIQ-style vendor
> security review, CIS Docker Benchmark, and air-gap delivery patterns. The
> chair independently re-verified every load-bearing claim against source
> before scoring; one seat claim was corrected during synthesis (§8). Static
> review only — no builds, no network calls, no code changes; the sibling
> contract repo (`asdecided/spec`) was consulted read-only. Question under
> review: *what should change to improve enterprise deployability, scored by
> impact vs cost?* Recorded decisions were treated as authoritative; no
> recommendation re-opens a settled ADR, and every tension with one is
> flagged inline.

## 1. Verdict

The engine is more enterprise-ready than the repository around it. The
council's consensus, across all six seats: the load-bearing security and
operability properties are real and verifiable in source — a structurally
no-egress binary, a structurally read-only six-tool MCP surface with hardened
HTTP edges, loopback-by-default serving with mandatory fail-loud audit,
git-as-truth with a disposable schema-versioned cache, and the enterprise ADRs
(ADR-084 audit, ADR-086 telemetry hard-lock, ADR-088 profile scaffold)
implemented, not aspirational.

What blocks a regulated deployment today is almost entirely the wrapper:

1. **Nothing shipped is verifiable.** Release archives and images carry no
   checksums, signatures, provenance, or SBOM, and the release pipeline runs
   zero tests — so a mirror operator, an Actions consumer, and a vendor
   security review all stall at "prove what this binary is."
2. **The product fails its own thesis in its own repo.** The agent-guidance
   layer misgrounds agents (broken `CLAUDE.md` imports, prompts describing a
   retired Python CLI, a settled-decisions list missing ADR-121–129), Accepted
   corpus artifacts assert live falsehoods (`pip install
   requirements-as-code`, an "MIT license" footer on an Apache-2.0 repo), and
   the funnel is littered with dead commands — including the repo's own
   `.mcp.json` invoking a binary that no longer exists. For a product whose
   pitch is "recorded knowledge agents can trust," this is the first thing an
   enterprise evaluator hits.
3. **Strong properties are undocumented; absent ones are documented.** Docs
   describe a telemetry ping the binary deliberately cannot send, while the
   actual differentiator — zero network-client and zero crypto crates in the
   entire workspace — is stated nowhere a buyer or reviewer would find it.
   There is no vulnerability-handling program, no compliance pack, no
   supported-versions statement, no sustainability disclosure in this repo.

Almost every high-impact item is days of work. The council found no
architectural rework required for enterprise deployability at current scale —
and explicitly recommends *against* most of the heavyweight "enterprise
hardening" a council like this usually produces (§7).

## 2. What already works (calibration)

Verified in source by at least two seats and the chair:

- **Structural no-egress.** No HTTP-client, TLS, async-runtime, or crypto
  crate anywhere in the 73-package `rust/Cargo.lock`; the only network code
  is the inbound `std::net` listener in `rust/decided-mcp/src/http.rs`. The
  ADR-041 ping is "deliberately never implemented"
  (`rust/decided-mcp/src/sidecar.rs:12-13`); the ADR-086 enterprise hard-lock
  ships (`rust/rac-engine/src/consent.rs`, `commands.rs` lock/status text).
- **Structurally read-only serving, hardened edges.** Six-tool allowlist
  dispatch (`rust/decided-mcp/src/main.rs:427-433`), no write tool exists;
  request size/connection bounds, Transfer-Encoding and duplicate
  Content-Length rejection, origin denial, JSON-RPC envelope validation
  (#422), hard response budgets (#425, ADR-033), symlink-refusing confined
  rename staging (#426, ADR-129); fuzz campaigns recorded under `rust/fuzz/`.
- **Zero-trust-friendly defaults.** Loopback bind by default
  (`decided-mcp/src/main.rs:51-52`); HTTP serving refuses to start without a
  working, writable audit sink (`http.rs:44-66`) per ADR-098.
- **Safe upgrade/rollback by construction.** All truth in git; caches are
  schema-versioned and disposable — version mismatch degrades to a rebuild
  (`rac-engine/src/derived_cache.rs`, `index_format.rs`), so binary
  up/downgrade cannot corrupt state.
- **Supply-chain basics.** Committed lockfile, `--locked` in CI, Docker, and
  release builds; pinned toolchain (`rust-toolchain.toml`); secretless OIDC
  trusted publishing for crates.io and the MCP registry; checksum-pinned
  third-party publisher binary in `mcp-registry-publish.yml`.
- **Determinism and contract gates are CI-real.** Spec-registry sync,
  cross-repo conformance, MCP spec vectors, live-corpus invariants asserting
  cache-on/cache-off byte equality (`rust/tools/live_corpus_invariants.py`),
  self-enforced as ADR-120 code constraints via the sentry gate
  (`pr-checks.yml`).
- **An honest threat model at the root.** `SECURITY.md` names the poisoned
  artifact / prompt-injection channel as the primary attack surface and
  states the ADR-065 human-PR-review boundary plainly.
- **Champion-grade demo assets.** A falsifiable grounding demo with a 10-run
  measurement protocol (`examples/guide/demo.md`), a verified/unverified
  recipe discipline (`docs/integration-recipes.md`), evidence-cited
  comparison tables (`docs/index.md`).
- **Deliberate licensing.** Apache-2.0 with recorded rationale (ADR-071),
  LICENSE + NOTICE present, DCO not CLA.

## 3. Scoring model

- **Impact** — High: unblocks deployments/procurement that today stall;
  Medium: removes material friction or risk for a typical enterprise
  rollout; Low: polish.
- **Cost** — S: days for a single maintainer; M: 1–4 weeks; L: multi-month
  or an ongoing commitment.
- **Confidence** — how strongly the cited evidence supports the finding.
- **Tier** — 1: do now (High/S). 2: plan next (High/M or Medium/S).
  3: strategic, demand-led. D: declined by the council (§7).

## 4. Impact × cost matrix

| ID | Finding | Impact | Cost | Conf. | Seats | Tier |
| --- | --- | --- | --- | --- | --- | --- |
| DEP-01 | Release verification pack (checksums, attestation, SBOM, license files) | High | S | High | Eng, GTM, DP, CISO, Con | 1 |
| DEP-02 | Gate releases on the full test battery | High | S | High | Eng | 1 |
| DEP-03 | Agent-guidance drift purge + self-drift CI gate | High | S | High | Con, Ev, GTM | 1 |
| DEP-04 | Truth-in-telemetry; market the zero-egress reality | High | S | High | GTM, CISO, Con, DP | 1 |
| DEP-05 | Vulnerability-handling program in SECURITY.md | High | S | High | CISO, GTM | 1 |
| DEP-06 | Propagate ADR-065 to operators/agents (OWASP LLM01) | High | S | High | CISO | 1 |
| DEP-07 | Non-loopback bind interlock + hardening checklist | High | S | High | CISO | 1 |
| DEP-08 | Buyer-path falsehoods: license labels, dead links, phantom package | High | S | High | GTM, Ev, Con | 1 |
| DEP-09 | Dead-command sweep + CI docs-lint; unify tool count and server key | High | S | High | Ev, GTM | 1 |
| DEP-10 | Dependency automation: Dependabot + cargo-deny (ban network crates) | High | S | High | Eng, DP, Con | 1 |
| DEP-11 | Honest sustainability statement (solo-maintainer, support posture, escrow) | High | S | High | Con, GTM, CISO | 1 |
| DEP-12 | Shared-server fleet primitives (liveness, drain, caps, reference manifest) | High | M | High | DP | 2 |
| DEP-13 | GitHub Actions consume prebuilt, verified binaries | High | M | High | Eng, DP, Ev | 2 |
| DEP-14 | Corpus truth pass + ADR ratifications and rename residue | High | M | High | Con, CISO, Eng | 2 |
| DEP-15 | Minimum viable compliance pack + Enterprise/Trust page + core GOVERNANCE | High | M | High | CISO, GTM | 2 |
| DEP-16 | CLI reference rewrite + real `--help` bodies | High | M | High | Ev | 2 |
| DEP-17 | Audit consumption pack (paths, rotation, SIEM recipe, data sheet, profile stanza) | Medium | S | High | DP, CISO | 2 |
| DEP-18 | `.decided/config.yaml` reference page (+ env-var inventory) | Medium | S–M | High | DP, Ev | 2 |
| DEP-19 | Quickstart routes to the grounding aha; 2-week evaluation kit | Medium | S | High | Ev, GTM | 2 |
| DEP-20 | Changelog backfill + release-gate changelog check | Medium | S | High | GTM, Eng, Ev, Con | 2 |
| DEP-21 | Workflow hardening: SHA-pin actions, `permissions:` on rust-spike | Medium | S | High | Eng | 2 |
| DEP-22 | Container non-root/digest pins/healthcheck; OS test matrix; musl target | Medium | M | High | Eng, CISO | 2 |
| DEP-23 | Compatibility & upgrades page (schema_version, cache rebuild, migrations) | Medium | S | High | Eng | 2 |
| DEP-24 | Repo hygiene: archive engineering scratch, remove agent exhaust; RELEASING.md | Medium | S | High | Con, Eng | 2 |
| DEP-25 | Community health files; fix ecosystem.md contribution contradiction | Medium | S | High | Ev | 2 |
| DEP-26 | Tell the spec/conformance anti-lock-in story in core docs | Medium | S | High | Ev | 2 |
| DEP-27 | Offline docs bundle attached to releases | Low | S | High | DP | 2 |
| DEP-28 | Scale-envelope promotion (S2 on server-class Linux) + perf gates in CI | Medium | M | High | DP, Eng | 3 |
| DEP-29 | Commercial path: settle ADR-012's successor; buyable support | High | L | High | GTM | 3 |

Seat key: GTM, Ev (Tech Evangelist), Con (Contrarian), Eng (Engineering
Leadership), DP (Design Partner), CISO.

## 5. Tier 1 — do now (high impact, small cost)

### DEP-01 · Release verification pack
Release archives are tar/zip'd and uploaded with no digest file, signature,
provenance, or SBOM (`.github/workflows/native-publish.yml:47-65`); images
push unsigned; archives and images omit LICENSE/NOTICE and third-party
notices (Apache-2.0 §4 obligation); `docs/validation.md:199` links to an
SBOM section that no longer exists — the Python-era SBOM and no-egress test
were deleted in the cutover (commit `ec49eef`) while a v0.21.14 roadmap
still records them "Achieved."
**Grounding:** SLSA v1.0 L2 via GitHub artifact attestations; OpenSSF
Scorecard "Signed-Releases"; NIST SSDF PS.3; SIG/CAIQ first-page asks.
**Recommend:** add SHA256SUMS + `actions/attest-build-provenance` +
cosign-keyless image signing + a CycloneDX SBOM to `native-publish.yml`;
include LICENSE/NOTICE/THIRD-PARTY-NOTICES in every archive and image; make
`docs/security.md` document the verify steps; correct the stale roadmap
status through the corpus process. *(Impact High / Cost S / Confidence High /
ADR tension: none.)*

### DEP-02 · Gate releases on the full test battery
`native-publish.yml` triggers on `release: published` and only builds;
`crates-publish.yml` runs a packaging dry-run. Nothing re-runs the
contract/live-corpus batteries at the release ref — ADR-027 rule 2
("nothing is built or published unless every battery passes") was
implemented in the retired Python publish workflow and has no native
equivalent. A tag cut from any commit ships untested binaries to five
platforms, GHCR, and crates.io.
**Grounding:** ADR-027 itself; NIST SSDF PW.8; cargo-dist release norms.
**Recommend:** make both publish workflows `needs:` the full battery
(reusable `workflow_call` on rust-spike) before any build/upload step.
*(High / S / High / none — restores ADR-027 on the native path.)*

### DEP-03 · Purge agent-guidance drift; ship a self-drift gate
`CLAUDE.md:9-10` imports `@rac/prompts/…` — a directory that does not exist
(corpus moved to `decisions/`), so the "loaded every session" guidance never
loads; `decisions/prompts/rac-agent-session-start.md` calls the product "a
Python CLI," prescribes pytest and `rac validate rac/`; the release-gate
prompts ask "Was pytest run?"; `CLAUDE.md` says "next up: v0.22.0" against a
0.26.2 workspace; the managed settled-decisions block ends at ADR-120 while
`decisions/decisions/` holds 130 ADRs — agents are told to respect a list
missing the nine most recent decisions, including the distribution/audit
ones this review leans on. The repo's own `.mcp.json` runs the retired
`rac` binary under the retired `lore` name.
**Grounding:** docs-as-code / dogfooding — confidently wrong instructions
are worse than none, and this product's pitch is that recorded knowledge
stays trustworthy.
**Recommend:** one-day content purge (fix imports, rewrite prompts for the
Rust/`decided`/`decisions/` reality, re-run `decided export --agent-rules`,
fix `.mcp.json`), then a CI drift gate the product itself could ship:
imports resolve, managed block current, no retired-CLI strings in live
prompts. Convert the fix into the demo — "we run it on ourselves, gated."
*(High / S; gate S–M / High / ADR-047 tension noted: structural gates passed
while content rotted — state that limit rather than paper over it.)*

### DEP-04 · Truth-in-telemetry; market the zero-egress reality
Docs describe egress that does not exist: `docs/mcp.md` documents a
`--telemetry` flag the server rejects, claims "at most one ping per 24
hours," and names `decisions/mcp/ping.py` — no such file, and no sender
exists in Rust ("deliberately never implemented," `sidecar.rs:12-13`).
Meanwhile a live-looking PostHog write key sits compiled in
(`consent.rs:30`) with `endpoint_configured: true`, and CLI text prints
ping ceremony. Reviewers who grep will find a key with no sender and docs
claiming daily egress — burning cycles in the one direction procurement
punishes. The actual posture — zero network-client crates — is stated
nowhere buyer-visible.
**Grounding:** CAIQ answer accuracy; NIST SSDF RV.2 (docs match artifact).
**Recommend:** a "Data flows and egress" page: the native build contains no
outbound network code (verifiable from `Cargo.lock`), inbound HTTP only with
`--transport http`; rewrite `docs/mcp.md` §7 and consent strings to say the
consent record is forward-looking; decide the PostHog key's fate (empty it
or record why it stays) via a short ADR-041 amendment; put the one-line
ADR-086 answer in README and SECURITY.md. Reality is better than the docs
claim — say so. *(High / S / High / ADR-040/041 tension: amend the record,
never silently erase it.)*

### DEP-05 · A vulnerability-handling program, not a mailbox
`SECURITY.md` provides a private-advisory channel but deliberately declines
any response commitment; there is no supported-versions statement, no
GHSA/CVE issuance commitment, no embargo or safe-harbor language, and the
published docs-site security page carries no reporting channel at all.
**Grounding:** coordinated-disclosure norms (disclose.io, GitHub GHSA→CVE);
a pass/fail row on every vendor questionnaire.
**Recommend:** add an *acknowledgment* SLA (e.g., 5 business days — distinct
from a fix SLA the team cannot staff), a supported-versions table ("latest
minor only" is acceptable if stated), a GHSA/CVE commitment, and safe-harbor
wording; link from `docs/security.md`. *(High / S / High / none.)*

### DEP-06 · Propagate the ADR-065 trust posture to operators and agents
The untrusted-content model lives only in root `SECURITY.md`. Tool
descriptions served to agents carry no treat-as-data caveat; `docs/mcp.md`
never mentions the injection threat, `decided doctor`, or provenance-status
checking; the recommended session-start prompt says recorded decisions "take
precedence" with no guard against imperative text inside artifacts.
**Grounding:** OWASP LLM Top 10, LLM01 (indirect prompt injection via
retrieved content) — the textbook channel for an MCP server serving
untrusted Markdown; when sanitization is out of scope by decision (ADR-065),
consumer-side guidance and provenance signaling are the prescribed control.
**Recommend:** a "Trusting what the server serves" section in `docs/mcp.md`
(PR review is the boundary; run `doctor` in the PR gate; prefer `Accepted`
via provenance) and a treat-as-data caveat in the recommended session
prompt. Documentation only — no server verdicts, no sanitization. *(High /
S / High / fully inside ADR-065/ADR-034.)*

### DEP-07 · Interlock the non-loopback bind; publish a hardening checklist
`--host 0.0.0.0` serves the entire corpus unauthenticated in plaintext with
only a stderr notice; no TLS is possible in-engine (by design); rate
limiting appears nowhere in docs. The only control against public exposure
is one sentence of prose in `docs/shared-server.md`.
**Grounding:** OWASP ASVS V1.2/V13; secure-by-default design — deliberate
acknowledgment for unauthenticated listeners.
**Recommend:** require an explicit flag (e.g., `--behind-proxy`) for
non-loopback binds, refusing otherwise with a pointer to the shared-server
docs; add a deployment-hardening checklist page (proxy TLS + authn, rate
limiting, network policy, read-only corpus mount) usable as procurement
collateral. *(High / S / High / ADR-085/098 respected — configuration
honesty, not authentication; cite ADR-098 when landing it.)*

### DEP-08 · Fix the falsehoods on the buyer path
`docs/index.md:122` labels the license "MIT" over an Apache-2.0 LICENSE
(ADR-071 records the relicense); an **Accepted** requirement
(`decisions/requirements/rac-docs-site-landing-page.md`) mandates rendering
`pip install requirements-as-code` and an "MIT license" footer; a shipped
skill instructs `pip install 'decided-core[ingest]'` — a package that
appears in no repo or ADR; `docs/index.md:103` links `tree/main/rac`, a
renamed directory. An agent querying the corpus for "what license?" can
retrieve "MIT" as Accepted knowledge.
**Grounding:** single-source-of-truth; OpenChain-style license hygiene — a
license contradiction is an instant legal-review stall.
**Recommend:** fix the label and links immediately; supersede or re-status
the stale Accepted artifacts (the deeper sweep is DEP-14); correct the
skill's install text. *(High / S / High / none — this implements ADR-071.)*

### DEP-09 · Dead-command sweep + CI docs-lint; one tool count, one server key
First-hour copy-paste failures across the funnel: `docs/quickstart.md` pins
`ghcr.io/asdecided/core:2026.6.1` (a CalVer tag ADR-111 retired);
`docs/cli.md` invokes a nonexistent `decided-mcp-stats` binary;
`examples/claude-code/README.md` installs a skill by its retired name;
`CONTRIBUTING.md` clones `core.git` then `cd rac-core/rust`; engine error
strings recommend `decided ingest`, which exits 2 "not yet implemented."
The MCP tool surface is described as four, five, and six tools across
`docs/index.md`, `docs/mcp.md` (self-contradictory internally), and the
recipes — the server ships six; the headline tools `retrieve_grounding` and
`find_decisions` have no reference docs. Recipes split the server key
between `lore` and `asdecided` inside the same documents, so verification
steps fail for half the config dialects.
**Grounding:** five-minute-first-success DX norm; Diátaxis reference
accuracy — the first failed paste is where enterprise pilots die.
**Recommend:** sweep every command literal against the shipped surface; add
a CI lint extracting fenced `decided …` commands and validating them against
the subcommand table and skill names; write the six-tool reference section
once and link it everywhere; standardize the server key on `asdecided` with
a one-line legacy note. *(High / S / High / ADR-030 tension: "exactly four
read-only tools" was amended in practice by later accepted decisions —
document six, note the amendment, don't re-litigate.)*

### DEP-10 · Dependency automation with a banned-network-crates rule
No Dependabot/Renovate, no cargo-deny/cargo-audit anywhere; the tree already
carries `serde_yaml 0.9.34+deprecated` (archived upstream) and `derivative
2.2.0` (unmaintained). Nothing would surface a future RUSTSEC advisory
against any of the 73 packages.
**Grounding:** OpenSSF Scorecard "Dependency-Update-Tool"/"Vulnerabilities";
NIST SSDF RV.1.1.
**Recommend:** add `dependabot.yml` (cargo, github-actions, npm for
rac-localview) and a `cargo deny check advisories licenses bans sources` CI
job whose ban list includes HTTP-client crates — converting the no-egress
guarantee from prose into a CI-enforced invariant (the Rust port of the
isolation battery ADR-098 describes); record the serde_yaml/derivative
disposition in `deny.toml` comments. *(High / S / High / none.)*

### DEP-11 · An honest sustainability statement
The only governance/succession disclosure lives in the sibling spec repo
("solo-maintained by choice," `spec/GOVERNANCE.md`); core has no GOVERNANCE
file, and `docs/governance.md` is about gate policy — a due-diligence
namespace collision. SECURITY.md declines response commitments with nothing
in their place.
**Grounding:** post-xz-utils OSS due diligence — maintainer continuity is a
named enterprise risk; honest framing beats simulated governance.
**Recommend:** publish the truth in core: solo-maintained, best-effort
support with the DEP-05 acknowledgment window, release-cadence policy, and a
credible continuity story (Apache-2.0 + published spec + conformance
fixtures + local-first data = genuine source-escrow value); cross-link the
spec GOVERNANCE; retitle or cross-reference `docs/governance.md`
("Enforcement policy") to free the term. Do not invent a governance board.
*(High / S / High / none.)*

## 6. Tier 2 — plan next

### DEP-12 · Shared-server fleet primitives *(High / M)*
The flagship enterprise deployment shape fails standard platform review:
`GET` on any path returns 405 (nothing for liveness/readiness probes), no
signal handling anywhere in `rust/` (no SIGTERM drain), a hard-coded
`MAX_ACTIVE_CONNECTIONS: 64` (`http.rs:34`), strictly serial dispatch under
one state mutex (`http.rs:210-213`), no reference K8s manifest, and a
docker recipe that leaves the derived cache ephemeral — every restart pays
the cold build (31–55 s at 100k artifacts) with no readiness signal to hold
traffic. **Recommend:** a payload-free liveness route inside the fenced
transport module, SIGTERM drain, a configurable connection cap, a reference
manifest with a cache volume, and an explicit N-stateless-replicas
statement. *(ADR-091/098 tension: the no-`/metrics` conclusion stands — a
bodyless liveness route is transport, not a scrape endpoint; ratify that
reading when landing it. Grounding: SRE golden signals, K8s rollout norms.)*

### DEP-13 · Actions consume prebuilt, verified binaries *(High / M)*
All three GitHub Actions run `cargo build --release --locked` in the
consumer's CI — multi-minute builds per PR, crates.io/rustup egress that
locked-down runners forbid, and stale headers ("v0.12.3… Python engine");
`watchkeeper.yml` pins the inner action at `@main`, so even tag-pinned
consumers execute mutable code. **Recommend:** after DEP-01, fetch the
platform release archive and verify its SHA256 (container-action fallback);
replace `@main` with a release pin; add a `runs-on` input for GHES; refresh
headers; document the actions' versioning. *(Grounding: GitHub Actions
hardening, Scorecard Pinned-Dependencies, air-gapped CI delivery.)*

### DEP-14 · Corpus truth pass + ratifications *(High / M)*
Beyond DEP-03/08 quick fixes: v0.26.x means two contradictory things
(corpus roadmap = Python TUI theming, "Achieved"; changelog = MCP-registry
work); ADR-085 is cited by the server banner and docs as the no-auth
authority while its Status is **Proposed**; ADR-075's required-check list
names retired Python jobs; ADR-084's text pins `.rac/config.yaml` while code
reads `.decided/config.yaml`; the production CI workflow is still named
`rust-spike` after ADR-116 sanctioned the engine; the `asdecided` org rename
is recorded only in changelog prose, not a superseding decision; ADR-012
(open-core) sits Proposed 118 ADRs later (see DEP-29). **Recommend:** a
dogfooded `decided review` pass: ratify ADR-085, amend ADR-075's job list,
supersede stale Accepted artifacts, sweep rename residue with a grep
checklist, and record the brand consolidation (RAC → Lore → AsDecided) as a
superseding ADR so ADR-036/039 stop contradicting the shipped name.
*(Grounding: the product's own thesis — decisions cited to reviewers must
trace to ratified, current records.)*

### DEP-15 · Minimum viable compliance pack *(High / M)*
No threat-model document beyond SECURITY.md, no data-flow narrative, no
crypto statement, no SBOM, no core GOVERNANCE, and no page assembling any of
it. The crypto statement is unusually easy and unusually strong: **zero
cryptographic libraries in the workspace** — nothing to FIPS-validate;
transit crypto is wholly the proxy's — currently unclaimed anywhere.
**Recommend:** one "Enterprise / Trust" docs page + a compliance pack:
data-flow diagram (stdio, HTTP, audit sink, cache, no-egress boundary),
crypto statement, DEP-05 policy, DEP-17 audit data sheet, SBOM pointer,
support/continuity statement (DEP-11) — questionnaire-shaped, citing
ADR-086 as its canonical reference. *(Grounding: NIST SSDF PO.3; SIG/CAIQ —
reviews consume documents; absent documents read as absent controls.)*

### DEP-16 · CLI reference rewrite + real `--help` *(High / M)*
`docs/cli.md` opens "RAC ships a single command, `rac`, with twenty-two
subcommands" against a shipped 32-entry surface; 137 lines document the
retired `explorer`; `ingest` is documented as working but exits 2; the
binary's `--help` prints a one-line stub ("Help body is out of parity
scope") — in air-gapped environments the built-in help is often the only
reachable reference. **Recommend:** rewrite the preamble, prune retired and
unimplemented sections, and implement real help bodies — the Python-oracle
parity constraint that motivated the stub is historical, and help text is
explicitly out of parity scope (`cli.rs:4-5`), so this works with the
ADR-116/120 lockstep guards. *(Grounding: clig.dev help-first discovery;
Diátaxis reference accuracy.)*

### DEP-17 · Audit consumption pack *(Medium / S)*
The compliance feature that justifies purchase is hard to consume: docs
state the default path as `$XDG_STATE_HOME/decisions/audit.jsonl` while code
writes `…/decided/audit.jsonl` (`docs/mcp.md:377-381` vs
`decided-mcp/src/audit.rs:95-102`); the recorder appends to one unbounded
file with no rotation guidance (safe external rotation exists — the file is
reopened per write — but is stated nowhere); records carry personal data
(git name/email principal) plus verbatim query text with no data-category,
retention, or integrity guidance; the ADR-090 collector satellite is
unshipped; and `init --profile enterprise` does not scaffold the `audit:`
stanza that HTTP serving refuses to start without — the paved enterprise
path fails on first `--transport http`. **Recommend:** fix the doc paths;
document rotation; publish an audit-record data sheet (fields, personal-data
flags, retention recipe, integrity-is-the-collector's-job statement) and a
reference JSONL→SIEM sidecar config (Fluent Bit/Vector); extend the
enterprise profile to emit a commented `audit:` stanza — config a careful
admin would hand-write, inside ADR-088's config-only bound (record the
ADR-088/098 alignment as a small amendment). *(Grounding: NIST SP 800-92;
GDPR minimization/storage-limitation; enterprise audit-log norms.)*

### DEP-18 · Configuration reference *(Medium / S–M)*
`.decided/config.yaml` keys are scattered across four docs pages; validation
rule IDs the repo itself disables have no lookup page; env-var overrides
(`DECIDED_NO_CACHE`, `DECIDED_CACHE_DIR`, `DECIDED_AUDIT_PATH`,
`DECIDED_AUDIT_PRINCIPAL`, …) are inventoried nowhere. Fleet config
management needs one schema-documented contract page; consider
`decided schema config` for CI linting. *(Grounding: 12-factor config.)*

### DEP-19 · Route the funnel to the aha; ship an evaluation kit *(Medium / S)*
The quickstart ends at validation/linting and never mentions MCP, agents, or
the grounding demo — the differentiating value is only reachable from two
buried links, and the scaffold's single TODO artifact can't demo retrieval.
**Recommend:** a "watch it ground an agent" step pointing `decided-mcp
--root` at `examples/guide/` (shipped example corpora, inside ADR-044's
bound), plus an "Evaluate in your org" page sequencing existing assets
(quickstart → demo → shared server → audit → gate) into a two-week POC with
measurable exit criteria. *(Grounding: README-driven adoption funnel;
enterprise POC-kit norms.)*

### DEP-20 · Changelog backfill + gate *(Medium / S)*
`CHANGELOG.md` claims Keep a Changelog but skips v0.24.x, v0.25.x, and
v0.26.0 entirely (release-prep commits exist for all three) and carries two
`## v0.23.0` headers (one is un-remapped ADR-111 residue). Enterprise change
management approves upgrades from this file. **Recommend:** backfill from
release PRs, apply the ADR-111 remap to the stray header, add a
version-history note, and enforce the matching-entry rule ADR-111 already
specifies in the release gate.

### DEP-21 · Workflow hardening *(Medium / S)*
`rust-spike.yml` is the one workflow with no `permissions:` block;
third-party actions are tag-pinned, not SHA-pinned — including inside the
crates.io-credentialed publish job; checkout versions are mixed.
**Recommend:** `permissions: contents: read` on rust-spike, SHA-pin
third-party actions (Dependabot from DEP-10 keeps pins fresh), standardize
checkout@v5. *(Grounding: Scorecard Token-Permissions/Pinned-Dependencies.)*

### DEP-22 · Container and platform matrix *(Medium / M)*
The published image runs as root (no `USER`), bases are tag-pinned rather
than digest-pinned, and there is no healthcheck — root images stall at
admission control in regulated clusters. Tests run only on Linux (Windows is
`cargo check`; macOS untested) while five targets ship, and there is no musl
target for RHEL-class or distroless deployment. **Recommend:** non-root
`USER` with fixed UID, digest-pinned bases, a healthcheck for the HTTP
target; `cargo test` on windows/macos at merge (keeping PRs light per
ADR-027); add x86_64-musl to the release matrix; publish a platform-support
tier table. *(Grounding: CIS Docker 4.1, NIST SP 800-190, "test what you
ship.")*

### DEP-23 · Compatibility & upgrades page *(Medium / S)*
The code's upgrade story is excellent — schema-versioned cache, degrade to
rebuild, `decided migrate` for layout/metadata — and written down nowhere an
operator would look; no statement exists of what is held stable pre-1.0
(JSON contract, exit codes, MCP surface, cache format). **Recommend:** one
page operationalizing ADR-111's SemVer-as-intent / schema_version-as-contract
split, including rollback safety and the pre-1.0 breaking-change process.

### DEP-24 · Repo hygiene + RELEASING.md *(Medium / S)*
~2,100 lines of engineering scratch ship in `rust/` (CLOSURE/HEAL/INDEX
plans and reports), `.agent-context/` carries three-generations-stale agent
exhaust (`gaps/agent1..5.md`, a BRIEF pinned to v0.10.3), `rust/evidence/`
includes literal social-media drafts (X-THREAD.md), a retired-TUI design
system sits at the root, and an internal release runbook is published in
`docs/`. Meanwhile no RELEASING.md documents the actual five-workflow
release chain. **Recommend:** triage, don't delete — the parity/fuzz/scale
reports are strong evidence and belong archived under `research/` with dated
preambles; remove agent exhaust and drafts; move the runbook; write
RELEASING.md (also the bus-factor mitigation an acquirer audits).
*(Grounding: repo-as-storefront due diligence; the session-start prompt's
own rule that durable thinking lives in the corpus.)*

### DEP-25 · Community health files *(Medium / S)*
One issue template exists (a telemetry usage report) — no bug/feature
templates, no PR template, no CODE_OF_CONDUCT, SUPPORT, or CODEOWNERS; and
`docs/ecosystem.md` states "contribution policy is pending; external
additions cannot yet be accepted" while a DCO CONTRIBUTING.md exists at
root. **Recommend:** standard health files; fix the contradiction; link
governance from README. *(Grounding: GitHub community standards; enterprise
OSS-health scans.)*

### DEP-26 · Tell the anti-lock-in story *(Medium / S)*
The spec repo carries a genuine third-party implementation story (schemas,
vocabulary, conformance fixtures, MCP wire vectors, "Can I implement this
without asdecided/core?") — and core's docs mention it only as an internal
certification fixture. Procurement's "what if the vendor disappears" has a
ready answer that goes unsaid. **Recommend:** a Specification page in core
docs nav + an ecosystem row + a line in the trust page citing the contract
as the lock-in mitigation. *(Grounding: conformance-suite ecosystem plays —
LSP, OpenTelemetry.)*

### DEP-27 · Offline docs bundle *(Low / S)*
Docs are hosted-only; engineers inside the air gap can't reach them.
CI already builds the site strictly — attach `docs-site.tar.gz` to each
release alongside DEP-01's artifacts. *(Grounding: air-gap delivery includes
documentation.)*

## 7. Tier 3 — strategic, demand-led

### DEP-28 · Scale-envelope promotion + perf regression gates *(Medium / M)*
The certified envelope stops at 5,000 artifacts, measured on an
Apple-silicon laptop, not the Linux server class the shared-server recipe
deploys; the 100k probe failed its compaction gate; nothing in CI re-runs
the benchmarks, so the marketed numbers decay silently; serial dispatch
(DEP-12) makes requests-per-second the real sizing question and it is
unstated. **Recommend:** promote the S2 (10k) tier on server-class Linux
with peak-memory and RPS figures; wire the existing harnesses
(`p6_scale.rs`, ADR-097 benchmark families) as a scheduled,
threshold-gated CI job publishing `metrics.json` per release. *(Grounding:
SRE capacity planning; continuous benchmarking. ADR-119's demand-led tier
promotion anticipates exactly this demand.)*

### DEP-29 · Define the commercial path *(High / L)*
There is nothing to procure: no commercial entity beyond an individual
(NOTICE), no support tier, no SLA, no sales contact (the only contact is the
issue tracker), and the open/paid boundary is undefined — ADR-012
(open-core) has sat **Proposed** for 118 ADRs while every surface it
reserved for commercial (governance/audit capabilities, MCP-compatible
knowledge services) has since shipped free; the drafted
`commercial-layer-positioning` roadmap is unscheduled. A regulated buyer's
vendor-risk review cannot complete against "no vendor." **Recommend:**
settle ADR-012's successor (accept, refresh, or supersede with the
substrate/per-org positioning already drafted); publish a one-page
open-vs-commercial-vs-roadmap statement; name the entity and a
non-issue-tracker contact. The positioning work is days; a real support
commitment is an ongoing commercial build — hence Tier 3, with the honest
interim being DEP-11. *(Grounding: open-core packaging norms; enterprise
procurement requires a counterparty.)*

## 8. Declined or deferred by the council

The Contrarian seat's anti-recommendations, adopted by the chair where the
council concurred. Each is a deliberate *no* with a cheaper substitute, not
an oversight:

- **SOC 2 / ISO 27001 now.** There is no hosted service to audit; the
  deliverable is a local binary plus the customer's own git repo.
  Substitute: the DEP-15 questionnaire-shaped trust pack. Revisit only when
  a hosted offering exists.
- **Fix-time SLAs.** Unstaffable at bus-factor 1; a broken SLA is fatal
  where best-effort honesty is survivable. Substitute: DEP-05's
  acknowledgment window + DEP-11's disclosure.
- **SSO / RBAC / auth in the engine.** Settled red line (ADR-085/098):
  authentication belongs to the fronting proxy; identity is attributable,
  not authenticated (ADR-084). Substitute: DEP-07's bind interlock and a
  tested proxy reference config. Buyers who cannot accept an
  attribution-based audit trail need the proxy to verify the principal
  header — that is the documented design, not a gap to fix in-engine.
- **An engine `/metrics` endpoint.** ADR-091's conclusion stands; metrics
  belong to the deployment wrapper. DEP-12's payload-free liveness route is
  the transport-layer exception, flagged for explicit ratification.
- **OpenSSF Scorecard badge-chasing.** Half the checks measure ceremony a
  solo repo cannot honestly satisfy (review-required, org policies). Adopt
  the signal subset only — already covered by DEP-01/10/21.
- **Federation / org-wide grounding plane now.** The prior review's Bet 1
  is multi-repo serving infrastructure ahead of any reference single-repo
  production deployment. YAGNI until a design partner runs the current
  product in production; the spec/conformance story (DEP-26) is the
  scale-out answer procurement needs today.
- **New distribution channels.** Eight already exist and visibly consume
  the maintainer (v0.26.x is largely registry-publishing fixes). Freeze
  channel count; the growth constraint is corpus credibility (DEP-03/14),
  not reach.
- **Penetration test / FIPS program.** Low yield against a no-crypto,
  no-egress, read-only local binary; the fuzz campaigns and DEP-15's crypto
  statement answer the real question. Revisit on a named procurement demand.
- **Content sanitization or per-artifact trust scoring.** Forbidden by the
  ADR-065 posture and SECURITY.md's explicit non-promises; DEP-06's
  consumer-side guidance is the correct form of this control.
- **Audit dashboards / web UI.** A CLI report over the existing JSONL is
  the S-cost version; defer any UI until the collector satellite exists.

## 9. Dissents, tensions, and corrections

- **Signing depth.** The Contrarian would defer signing/attestation until a
  customer demands artifact verification; four seats ranked it the #1
  unlock. Chair ruling: GitHub-native attestation + SHA256SUMS + SBOM are
  near-zero marginal cost on hosted runners and gate everything downstream
  (mirrors, Actions, vendor review) — do now (DEP-01); defer only the
  heavier ceremony (custom verification policy infrastructure) until
  demanded. The Contrarian independently endorsed the SBOM + cargo-deny
  subset.
- **Serial dispatch ceiling.** One global mutex and a 64-connection cap
  behind a "multi-thousand-seat" ambition (ADR-098's own trigger) is an
  architecture question the council chose to *measure first* (DEP-28's RPS
  figure) rather than pre-emptively re-engineer. If measured throughput
  bounds fleet size materially, that becomes a roadmap item with its own
  decision record.
- **Attribution-not-authentication.** The design partner's compliance team
  and the CISO both note buyers must consciously accept an audit log of
  *claims* fronted by an authenticating proxy. This is settled (ADR-084/098)
  and honest — the council's ask is only that every audit-adjacent page says
  it as loudly as the ADRs do (DEP-15/17).
- **Correction during synthesis.** One seat inferred from `git log` that the
  repository's history had been re-seeded two weeks before this review
  ("14 days of provenance"). The chair traced this to the checkout being a
  shallow clone (`.git/shallow` present; the root commit carries a grafted
  parent) and struck the claim. The solo-maintainer observation stands on
  the spec repo's own disclosure, not on commit archaeology. Claims that
  could not be verified offline (marketplace/tag state, hosted-registry
  behavior) are marked at reduced confidence in seat reports and were not
  load-bearing for any Tier 1 recommendation.

## 10. Seat top-3 index

- **GTM:** define what a buyer can buy (DEP-29); restore SBOM/provenance and
  stop advertising deleted artifacts (DEP-01); market the zero-egress
  reality (DEP-04).
- **Tech Evangelist:** kill the dead commands with a CI docs-lint (DEP-09);
  make the CLI reference and `--help` describe the shipped CLI (DEP-16);
  route the funnel to the grounding demo (DEP-19).
- **Contrarian:** purge and gate self-corpus drift (DEP-03/14);
  truth-in-security quick wins (DEP-04/01/20); honest sustainability posture
  over enterprise theater (DEP-11).
- **Engineering Leadership:** checksums/attestation/SBOM (DEP-01); gate
  releases on the battery (DEP-02); prebuilt verified binaries in the
  Actions (DEP-13).
- **Design Partner:** verifiable mirroring (DEP-01); shared-server fleet
  primitives (DEP-12); a consumable audit path (DEP-17).
- **CISO:** a real disclosure program (DEP-05); ADR-065 propagated to
  operators and agents (DEP-06); the minimum viable compliance pack
  (DEP-15).
