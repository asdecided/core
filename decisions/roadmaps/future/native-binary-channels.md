---
schema_version: 1
id: RAC-KXMQC8NEKP45
type: roadmap
---
# Native Binary Channels — Homebrew, Scoop, and the Channel Map

## Status

Planned

Follow-up to roadmap:native-engine-cutover. The cutover ships the native
engine inside the PyPI wheel; this item gives the compiled `rac` / `rac-mcp`
binaries first-class package-manager channels of their own, with Homebrew as
the flagship. Maintainer-decided posture: no code signing is on the critical
path — none of these channels requires it (Homebrew installs are not
quarantined; Scoop strips Mark-of-the-Web; pip sets neither), and signing
re-enters only if raw browser-download distribution is ever added.

## Outcomes

- A macOS or Linux user installs the native `rac` with one
  `brew install` — no Python on the machine, no pip, the covered surface
  at full native speed. Homebrew is the flagship channel.
- A Windows user has a native analogue via Scoop (and later winget), with
  pip remaining the universal fallback on every platform.
- The channel map is explicit and recorded: brew and Scoop ship the Rust
  binary alone (covered surface only); PyPI ships the wheel with the
  bundled binary plus the pure-Python engine (fenced `ingest`, arbiter,
  and any platform without a native build). A brew/Scoop install that
  invokes a fenced command gets a clear pointer to the pip extra.
- Distribution previews the recorded end-state direction: the binary-only
  channels are the purest expression of the native engine as the product,
  with Python behind them only where its surfaces are actually needed.

## Initiatives

- Stand up `itsthelore/homebrew-tap` with a `rac` formula: build from
  source via cargo (pinned toolchain), install `rac` and `rac-mcp`, with
  bottles built by the tap's CI for the supported macOS/Linux targets.
- A `brew install` smoke test in the tap's CI: `rac --version` (compiled-in
  version), a covered command against a fixture corpus, and the fenced-
  command pointer message.
- A Scoop manifest (own bucket first) for the Windows binary, exercising
  the same smoke test; a winget manifest as a later, appetite-driven step.
- Wire the release process so a `vX.Y.Z` tag updates the formula and
  manifest versions/checksums automatically (or by a scripted bump PR).
- Document the channel map in the README install table: brew / Scoop for
  the native binary, pip for the full package, and what each includes.
- Graduation to homebrew-core when notability criteria are met; the tap
  remains the canonical channel until then.

## Constraints

- The binary-only channels ship exactly the covered surface (ADR-116);
  they must not grow engine behavior of their own. A fenced command
  invoked from a binary-only install fails with a pointer to pip, never a
  partial reimplementation.
- Byte-parity discipline is unchanged: the channel binaries are the same
  cargo artifacts the wheel bundles, built from the same tagged commit
  with the same compiled-in version.
- Linux binaries hold a fixed glibc floor (2.17), built with
  `scripts/build-linux-release.sh` and run on that floor in CI before
  release; raising it drops supported distributions and is a deliberate
  change, never a side effect of the runner image.
- No code signing dependency: channels are chosen so unsigned binaries
  are first-class (brew, Scoop, pip). Adding a channel that requires
  signing (raw GitHub Release downloads as a promoted path, MSIX) is a
  separate decision.
- The PyPI wheel path stays fully supported — brew is the flagship, not
  a replacement; pip remains the only channel carrying `ingest` and the
  Python arbiter.

## Success Measures

- `brew install itsthelore/tap/rac` yields a working native `rac` on
  macOS (arm64) and Linux (x86_64), reporting the tagged version with no
  Gatekeeper interception and no Python present.
- `scoop install rac` (from the bucket) yields a working native
  `rac.exe` on Windows x86_64 with no SmartScreen prompt.
- A release tag propagates to formula and manifest without hand-editing
  checksums.
- The README channel map matches what each channel actually installs.

## Assumptions

- The Rust binaries remain self-contained (no dynamic dependencies beyond
  the platform baseline), so bottles and manifests stay trivial.
- The covered surface alone is a useful product for binary-channel users —
  true today: the CLI and MCP serving are covered; `ingest`/`explorer`
  are deliberately pip-only.
- Homebrew/Scoop users accept build-from-source or tap-CI bottles until
  homebrew-core notability is reached.

## Risks

- Channel drift: formula/manifest versions lag a release; mitigated by
  the automated bump wiring and a release-checklist line.
- Split-brain support burden ("which install do you have?"); mitigated by
  `rac --version` carrying the compiled-in version everywhere and the
  README channel map naming what each channel includes.
- WSL confusion on Windows (brew-in-WSL installs Linux binaries);
  mitigated by documenting Scoop/pip as the native Windows paths.

## Related Decisions

- ADR-116
- ADR-111
- ADR-072
- ADR-005

## Related Roadmaps

- native-engine-cutover
