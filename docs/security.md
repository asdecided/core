# Security posture

AsDecided is local-first and deterministic. The native `decided` CLI reads
repository Markdown and emits local files, stdout, and exit codes. Core
validation and retrieval require no hosted index, model call, account, or
Python runtime.

## No-egress boundary

Validation, relationships, review, gate, search, and export operate on the
local filesystem. The native MCP server emits no usage telemetry and has no
network side channel. `decided telemetry` records a local compatibility
preference only; the native build has no outbound sender (ADR-131). Regulated
installations can also record an explicit hard-lock with:

```bash
decided telemetry off --enterprise
```

Optional read-access audit records are local files. Shipping them elsewhere is
the operator's separate responsibility.

## Dependency surface

The shipped implementation is the Cargo workspace under `rust/`. Review
`rust/Cargo.lock` for its resolved dependency graph. The Python package,
Python dependency manifest, and Python engine were retired from this
repository; the final snapshot is preserved at `python-engine-final`.

## Verification

```bash
cd rust
cargo test --workspace --release
cargo clippy --workspace --all-targets -- -D warnings
```

CI also runs contract certification against `asdecided-spec` and live-corpus
invariants. This is a self-attested open-source security posture, not a
third-party certification.

## Vulnerability handling

Report suspected vulnerabilities through the private-advisory flow described
in the repository's [`SECURITY.md`](https://github.com/asdecided/core/blob/main/SECURITY.md).
We target acknowledgment within five business days, support the current
stable release on a latest-release basis, and do not promise a fix-time SLA.
Confirmed material issues may receive a GitHub Security Advisory and a CVE
request when eligible. The policy also includes a good-faith research safe
harbor and coordinated-disclosure guidance.

## Release verification

Each native release carries a `SHA256SUMS` file, a CycloneDX SBOM, and GitHub
artifact-build attestations. Archives include `LICENSE`, `NOTICE`, and
`THIRD-PARTY-NOTICES`; the same notices are present in the published container
image at `/usr/share/doc/asdecided/`. The release workflow runs the full native
Rust battery before building or publishing any archive, crate, image, or MCP
Registry entry.

After downloading an archive and the `SHA256SUMS` asset from the release, verify
the bytes and provenance from a trusted checkout of this repository:

```bash
sha256sum -c SHA256SUMS
gh attestation verify asdecided-x86_64-unknown-linux-gnu.tar.gz \
  --repo asdecided/core
```

The SBOM is the `asdecided-<tag>-sbom.cdx.json` release asset. Verify a GHCR
image's keyless signature with the GitHub Actions OIDC issuer and the exact
release workflow identity:

```bash
cosign verify \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  --certificate-identity-regexp \
    'https://github.com/asdecided/core/.github/workflows/native-publish.yml@refs/tags/v.*' \
  ghcr.io/asdecided/core:mcp-v<version>
```

The checksum, SBOM, and signature cover the published artifact; the source
repository and locked Cargo graph remain the reviewable build inputs.
