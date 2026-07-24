---
schema_version: 1
id: RAC-KWGMT9ZMN69Q
type: roadmap
---
# Official OCI Image

## Status

Achieved

Delivered as recorded (ADR-061 terminal lifecycle): the root `Dockerfile`
installs the released `rac-core` on a minimal Python base with `rac` as the
entrypoint, and the official image publishes to `ghcr.io/itsthelore/asdecided-core`,
CalVer- and digest-pinned in the quickstart with GitLab CI, Bitbucket, and
Jenkins usage snippets — matching this item's recorded scope. A member of the
`agnostic-surfaces` programme, Track 1 (distribution and CI reach), named by
the SDK expansion council as the single highest-leverage artifact for
non-Python adoption: one image unlocks every docker-native CI platform with
zero wrapper code. This artifact remains the durable record; distribution
evolution continues under `agnostic-surfaces`, and the `rac-ci` wrappers
consume the image rather than re-solving installation.

## Outcomes

- A team runs the `rac` CLI anywhere a container runs — GitLab CI
  (`image:`), Bitbucket Pipes, Jenkins docker agents, local docker —
  without installing Python or pip.
- The image is official, pinned, and boring: built from the engine repo,
  versioned with the same CalVer releases as the PyPI package (ADR-076),
  reproducibly referenced by digest.
- Air-gapped estates gain a procurement-friendly install path: mirror
  one image instead of assembling a Python environment (ADR-086).

## Initiatives

- Add a Dockerfile to the engine repo that installs the released
  `rac-core` package on a minimal Python base and sets `rac` as the
  entrypoint; no engine code changes.
- Add a publish workflow that builds and pushes the image on the same
  release tags as the PyPI publish, tagging both the CalVer version and
  `latest`, to an official registry home (for example
  `ghcr.io/itsthelore/asdecided-core`; final home decided at pickup).
- Document the image on the install page: pull, pin-by-digest, and
  usage snippets for GitLab CI, Bitbucket Pipelines, and Jenkins docker
  agents running `rac validate` / `rac gate`.
- Record the image as the substrate the `rac-ci` platform wrappers
  consume, so wrapper work never re-solves installation.

## Constraints

- Packaging only: the image wraps the released `rac-core` distribution;
  it introduces no behaviour, flags, or configuration of its own.
- Deterministic and offline by default, matching the engine posture
  (ADR-002, ADR-086): no phone-home added by the image layer; telemetry
  behaviour is exactly the CLI's own.
- Per-platform wrapper logic (GitLab component, Bitbucket pipe, Jenkins
  step) is out of scope — that is the `rac-ci` item's concern (ADR-092).
- A standalone binary or `uvx`-based distribution is a recorded
  alternative, not part of this item; revisit if image adoption shows
  the container path is insufficient.

## Success Measures

- A documented GitLab CI job and a Bitbucket pipeline run a `rac` gate
  using only the published image and the snippet — no install steps.
- Image version and PyPI version are released together and match for
  every CalVer release after the item ships.
- The `rac-ci` wrappers, when built, consume the image rather than
  pip-installing.

## Assumptions

- GitHub-hosted registry (or equivalent) is acceptable for the official
  home; enterprise mirrors pull and re-host per their own policy.
- The engine's Python version support fits a maintained slim base image.

## Risks

- The image drifts from the PyPI release if publishing is manual;
  mitigated by publishing both from the same tag-triggered workflow.
- A vulnerable base image inherits into the official artifact; mitigated
  by pinning a maintained base, rebuilding on release, and including the
  image in the existing SBOM practice.

## Related Decisions

- ADR-111
- ADR-086
- ADR-092

## Related Roadmaps

- agnostic-surfaces
- rac-ci

## Related Tickets

- itsthelore/asdecided-core#280
