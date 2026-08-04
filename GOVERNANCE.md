# Project Governance and Continuity

## Stewardship

AsDecided core is currently solo-maintained by Tom Ballard
([@tcballard](https://github.com/tcballard)). It is not governed by a
foundation, steering committee, or employer, and there is no governance board
to imply otherwise. The maintainer has final responsibility for accepting
changes and publishing releases.

Material product and engineering choices are recorded as reviewable artifacts
under [`decisions/`](decisions/). Contributions follow
[`CONTRIBUTING.md`](CONTRIBUTING.md), including the Developer Certificate of
Origin. Opening an issue or pull request does not create a commitment to merge
it or to respond within a particular time.

The language-neutral contract has its own governance policy in
[`asdecided/spec`](https://github.com/asdecided/spec/blob/main/GOVERNANCE.md).
Core is the reference implementation, but the published specification and
conformance fixtures are the compatibility authority.

## Support and releases

Support is best-effort. There is no paid support contract, guaranteed response
time, or fix-time SLA attached to this open-source repository. Security reports
are the exception to the general response posture: the
[`SECURITY.md`](SECURITY.md) policy targets acknowledgment within five business
days, while explicitly not promising a fix deadline.

Only the current stable release is supported. Releases are demand-led rather
than scheduled to a fixed calendar: a release is published when a coherent
change set has passed the repository's test, contract, and release gates. The
project may publish quickly when a protocol or security change requires it,
but does not promise a regular cadence.

## Continuity and adopter control

AsDecided is designed so adoption does not create a hosted-service dependency:

- the core implementation is available under the Apache-2.0 license;
- the specification, schemas, and conformance fixtures are published in
  [`asdecided/spec`](https://github.com/asdecided/spec);
- decision corpora remain ordinary Markdown and configuration in the adopter's
  own git repositories;
- the native engine runs locally, with no required account, hosted index, or
  outbound network service; and
- derived caches are disposable and can be rebuilt from the repository.

These properties provide practical continuity if maintenance slows or stops:
an adopter can pin a release, retain and read its corpus without AsDecided, or
fork the Apache-2.0 implementation and validate it against the public contract.
They are not a formal source-escrow arrangement and do not substitute for a
commercial support agreement.

## Changes to this policy

Changes to this file use the normal pull-request process. A change that alters
project stewardship, compatibility authority, licensing, or the support
posture should also be recorded in the decision corpus so the public statement
and the project's operating record change together.
