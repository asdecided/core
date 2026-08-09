#!/usr/bin/env node
/**
 * make-corpus.mjs — generate the viewer's sample corpora. No dependencies.
 *
 * Emits two files:
 *   1. src/viewer/sample/lore-export.sample.json
 *      A hand-authored 30-artifact decision corpus for "ledgerline", a
 *      fictional mid-size Python billing service. Clearly labelled
 *      SAMPLE DATA. Committed; the dev server and the single-file
 *      artifact both consume it.
 *   2. /tmp/lore-export-500.json
 *      A deterministic 500-artifact synthetic corpus for performance
 *      testing. NOT committed.
 *
 * Schema: reconciled v1 — see rac-localview/VIEWER_CONTRACT.md. Matches the
 * shape of `rac export --json`: string schema_version, opaque artifact
 * ids with human aliases, source paths, authored-case statuses, and
 * untyped `relates-to` edges only.
 */

import { createHash } from 'node:crypto';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');

/* ------------------------------------------------------------------ */
/* helpers                                                             */
/* ------------------------------------------------------------------ */

const html = (paragraphs) => paragraphs.map((p) => `<p>${p}</p>`).join('\n');

/** "Replace Celery with Dramatiq" -> "replace-celery-with-dramatiq". */
const slug = (text) =>
  text
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');

/** "accepted" -> "Accepted" — RAC statuses are authored capitalised. */
const cap = (status) => status.charAt(0).toUpperCase() + status.slice(1);

/**
 * Deterministic opaque id in the Core shape (RAC-<12 uppercase
 * alphanumerics>), derived from a stable seed so regeneration is
 * byte-identical.
 */
function opaqueId(seed) {
  const digest = createHash('sha256').update(seed).digest();
  const alphabet = 'ABCDEFGHJKMNPQRSTVWXYZ0123456789';
  let out = '';
  for (let i = 0; i < 12; i++) out += alphabet[digest[i] % alphabet.length];
  return `RAC-${out}`;
}

/* ------------------------------------------------------------------ */
/* 1. hand-authored 30-artifact sample corpus — "ledgerline"           */
/* ------------------------------------------------------------------ */

// Each entry: [alias key, type, status, title, [paragraph, ...]].
// The alias key ("ADR-001") becomes the short human alias; the emitted
// id is an opaque deterministic RAC-style id. Bodies cite other
// artifacts by alias ("ADR-021"), which the viewer linkifies
// case-insensitively.
const SAMPLE = [
  [
    'ADR-001', 'adr', 'accepted', 'Record architecture decisions as ADRs',
    [
      'Ledgerline has grown past the point where decisions survive in chat history. New engineers re-litigate settled questions because the reasoning behind the current shape of the system is not written down anywhere durable.',
      'We will record every architecturally significant decision as a numbered ADR in the repository, in typed Markdown with YAML frontmatter, reviewed through the same pull-request flow as code.',
      'Consequence: decisions are versioned with the code they govern, and superseding a decision is an explicit, reviewable act rather than silent drift.',
    ],
  ],
  [
    'ADR-002', 'adr', 'superseded', 'Use Flask for the HTTP layer',
    [
      'The original prototype needed an HTTP layer quickly and the team knew Flask well. The service surface was three endpoints and a health check.',
      'We chose Flask with blueprints, gunicorn workers, and marshmallow for request validation.',
      'This decision was superseded by ADR-014 when the endpoint count and the cost of hand-rolled validation outgrew the framework. The migration notes live with ADR-014.',
    ],
  ],
  [
    'ADR-003', 'adr', 'accepted', 'PostgreSQL as the primary datastore',
    [
      'Ledgerline is a ledger: correctness under concurrent writes matters more than write throughput. We evaluated PostgreSQL, MySQL and DynamoDB against our consistency and reporting requirements.',
      'PostgreSQL wins on transactional semantics, mature tooling, and the team’s operational experience. Serializable isolation is available where the ledger invariants demand it.',
      'All durable state lives in a single PostgreSQL cluster until measurement, not intuition, proves we need otherwise. Reporting load is addressed separately in ADR-023.',
    ],
  ],
  [
    'ADR-004', 'adr', 'accepted', 'Schema migrations with Alembic',
    [
      'Schema changes were being applied by hand against staging and production, and the two had drifted. We need migrations that are ordered, reviewable and reversible.',
      'We adopt Alembic, with one migration per pull request and a CI check that the migration history applies cleanly to an empty database. Autogenerate output is a starting point, never committed unreviewed.',
      'Destructive migrations (drops, type narrowing) require a two-step expand/contract sequence across releases, so deploys stay backwards compatible per ADR-028.',
    ],
  ],
  [
    'ADR-005', 'adr', 'superseded', 'Celery with a Redis broker for background work',
    [
      'Invoice generation and webhook delivery are too slow for the request path. We need a task queue.',
      'We chose Celery with Redis as the broker, since Redis was already deployed for caching (ADR-006) and Celery was the de facto standard for Python at the time.',
      'In practice Celery’s configuration surface and prefork worker model caused recurring operational pain. Superseded by ADR-021, which records the move to Dramatiq.',
    ],
  ],
  [
    'ADR-006', 'adr', 'accepted', 'Redis caching with explicit TTLs only',
    [
      'Tariff lookups and customer entitlement checks dominate read load and change rarely. A cache is justified; an incoherent cache in a billing system is not.',
      'We cache in Redis with an explicit TTL on every key and no cache-invalidation-by-event anywhere. If a value cannot tolerate being stale for its TTL, it must not be cached.',
      'Keys follow the pattern documented in STD-002, and every cached read path must degrade correctly when Redis is unavailable: the cache is an optimisation, never a source of truth.',
    ],
  ],
  [
    'ADR-007', 'adr', 'accepted', 'Authenticate via OIDC against the corporate IdP',
    [
      'Ledgerline must not own passwords. Operators authenticate with their corporate identity; service-to-service callers use workload identities.',
      'Human sessions use the OIDC authorization-code flow against the corporate identity provider. Machine callers present short-lived JWTs minted by the platform’s workload identity service; we validate issuer, audience and expiry on every request.',
      'No local user table, no API keys in headers, no long-lived shared secrets. Authorization is a separate concern, recorded in ADR-008.',
    ],
  ],
  [
    'ADR-008', 'adr', 'accepted', 'Role-based authorization enforced in the service layer',
    [
      'With authentication settled in ADR-007, we need a consistent answer for what a caller may do. Endpoint-by-endpoint ad hoc checks have already produced one near-miss.',
      'Authorization is role-based, evaluated in the service layer rather than in HTTP handlers, so the same rules govern the API, background tasks and the admin CLI. Roles are declared in code; assignments live in the database.',
      'Every privileged operation must name its required role explicitly. A missing declaration fails closed and fails loudly in CI.',
    ],
  ],
  [
    'ADR-009', 'adr', 'accepted', 'Structured logging with structlog',
    [
      'Grepping interleaved free-text logs across gunicorn workers does not survive contact with a real incident.',
      'All logs are structured key-value events emitted through structlog, JSON-rendered in production, with request_id and customer_id bound automatically at the edge of every request and task.',
      'Log messages are lowercase event names, not sentences; context goes in fields. Nothing sensitive (PANs, tokens, raw payloads) may be logged — enforced by a processor that redacts known field names.',
    ],
  ],
  [
    'ADR-010', 'adr', 'accepted', 'Prometheus metrics, Grafana dashboards',
    [
      'We need quantitative answers to "is it slow" and "is it failing" that do not require reading logs (ADR-009).',
      'The service exposes Prometheus metrics: RED metrics per endpoint and per task type, plus domain counters such as invoices_issued_total. Dashboards live in Grafana, provisioned from JSON in this repository.',
      'Every alert must page on symptoms (error rate, latency, queue age), not causes. Cause-level metrics exist for diagnosis, not alerting.',
    ],
  ],
  [
    'ADR-011', 'adr', 'accepted', 'Distributed tracing with OpenTelemetry',
    [
      'A single invoice touches the API, two background tasks and three external calls. Logs (ADR-009) and metrics (ADR-010) tell us something is slow; they do not tell us where.',
      'We instrument with OpenTelemetry, propagating W3C trace context across HTTP calls and through the task queue, and export to the platform’s collector. Sampling is head-based at 10% with errors always sampled.',
      'Spans are added at service boundaries and around external calls only; we do not trace every function. Trace IDs are bound into log events so logs, metrics and traces cross-reference.',
    ],
  ],
  [
    'ADR-012', 'adr', 'accepted', 'Ship a single Docker image per release',
    [
      'API, worker and scheduler were drifting apart as three separately built artefacts with three dependency snapshots.',
      'One Docker image per release contains the whole application; the entrypoint argument selects the role (api, worker, scheduler). The image is built once in CI and promoted unchanged through staging to production.',
      'Configuration is environment-only, per twelve-factor practice. Secrets are injected at runtime as recorded in ADR-027, never baked into the image.',
    ],
  ],
  [
    'ADR-013', 'adr', 'superseded', 'Pin dependencies with pip-tools',
    [
      'Unpinned dependencies made builds unreproducible: two images built an hour apart differed.',
      'We adopted pip-tools: requirements.in declares direct dependencies, requirements.txt is the compiled lockfile, and CI fails if the two disagree.',
      'Superseded by ADR-024. pip-tools served well, but uv subsumes it with much faster resolution and a single tool for environments and locking.',
    ],
  ],
  [
    'ADR-014', 'adr', 'accepted', 'Move the HTTP layer from Flask to FastAPI',
    [
      'This decision supersedes ADR-002. The API has grown to forty-plus endpoints; hand-written marshmallow schemas duplicate type information that already exists in our domain models, and the duplication has caused real validation bugs.',
      'We move to FastAPI. Pydantic models give us request and response validation from type annotations, an OpenAPI document for free, and async handlers where outbound calls dominate.',
      'Migration is incremental: new endpoints are FastAPI-only, existing Flask blueprints are ported as they are next touched, and the OpenAPI document becomes the API contract of record under ADR-015.',
      'The Flask stack was removed entirely in release 2.9 after eleven months of coexistence.',
    ],
  ],
  [
    'ADR-015', 'adr', 'accepted', 'Version the public API by URL prefix',
    [
      'External integrators need a stability promise. We must be able to change the API without breaking them.',
      'The public API is versioned by URL prefix (/v1/, /v2/). Within a version, changes must be strictly additive; anything else requires a new version. At most two versions are live at once, with a published deprecation window of twelve months.',
      'Internal endpoints carry no version prefix and no stability promise. The OpenAPI document from ADR-014 is generated per version.',
    ],
  ],
  [
    'ADR-016', 'adr', 'accepted', 'Cursor-based pagination for list endpoints',
    [
      'This refines ADR-015. Offset pagination over the transactions table both performs badly at depth and silently skips or repeats rows under concurrent writes — unacceptable for a ledger.',
      'All list endpoints paginate with opaque cursors encoding the last-seen sort key. Cursors are signed so clients cannot forge or decompose them, and page size is capped at 200.',
      'Offset parameters on the two endpoints that already shipped with them remain accepted in /v1/ but are documented as deprecated and absent from /v2/.',
    ],
  ],
  [
    'ADR-017', 'adr', 'accepted', 'Idempotency keys on all mutating endpoints',
    [
      'Clients retry. Without idempotency, a retried POST /v1/payments charges a customer twice; this has happened once in staging and must never happen in production.',
      'Every mutating endpoint requires an Idempotency-Key header. The key, the request hash and the response are stored for 48 hours; a replay with the same key and body returns the stored response, and the same key with a different body is rejected with 422.',
      'The storage piggybacks on PostgreSQL (ADR-003) rather than Redis, because losing idempotency records is a correctness failure, not a performance one.',
    ],
  ],
  [
    'ADR-018', 'adr', 'accepted', 'Represent money as integer minor units',
    [
      'Floating-point money is forbidden. The question is decimals versus integers.',
      'All amounts are integers in minor units (pence, cents) paired with an ISO 4217 currency code, in the database, in the API and in code. A Money value object owns arithmetic and rounding; raw integer arithmetic on amounts is rejected in review.',
      'Rounding policy is banker’s rounding, applied only at explicitly marked boundaries such as tax calculation. Conversion to display strings happens at the edge, never in the domain.',
    ],
  ],
  [
    'ADR-019', 'adr', 'accepted', 'UTC everywhere; timezones at the edge',
    [
      'A billing-period bug traced to a naive datetime crossing a DST boundary cost two days. Time handling needs one rule, not judgement calls.',
      'All timestamps are timezone-aware UTC in the database (timestamptz), in the API (RFC 3339 with Z), and in code. Naive datetimes are banned and rejected by a lint rule. Customer-local times exist only at presentation, derived from an explicit per-account timezone.',
      'Billing period boundaries are defined in the customer’s account timezone and converted to UTC instants when periods are materialised — this is the one place local time enters the domain, and it is heavily tested.',
    ],
  ],
  [
    'ADR-020', 'adr', 'proposed', 'Database-backed feature flags',
    [
      'We need to ship the new dunning flow dark and enable it per customer cohort. A SaaS flag service is overkill for our needs and adds a hard runtime dependency.',
      'Proposal: a feature_flags table read through the existing cache (ADR-006) with a 60-second TTL, exposing boolean and percentage rollout flags keyed optionally by account. Flags are declared in code with an owner and an expiry date; CI warns on expired flags.',
      'Open questions for review: whether percentage rollout hashes on account or on actor, and whether flag reads belong in domain code or only at entry points.',
    ],
  ],
  [
    'ADR-021', 'adr', 'accepted', 'Replace Celery with Dramatiq',
    [
      'This supersedes ADR-005. Three incidents in two quarters traced to Celery worker lockups and lost acks, and per-task configuration had become folklore.',
      'We move background work to Dramatiq with the Redis broker retained. Middleware gives us retries with exponential backoff and dead-letter queues by default; task signatures are plain functions, which makes ADR-025’s strict typing actually enforceable on task boundaries.',
      'Delivery remains at-least-once. Every task must therefore be idempotent, reusing the patterns from ADR-017 where tasks have external effects.',
      'Migration ran queue-by-queue behind the abstraction layer introduced for the purpose; the last Celery worker was decommissioned in release 3.4.',
    ],
  ],
  [
    'ADR-022', 'adr', 'accepted', 'Transactional outbox for outbound events',
    [
      'Webhook and event-bus publishes were performed inside request handlers after the database commit, so a crash between commit and publish silently dropped events. Relates to the at-least-once delivery posture of ADR-021.',
      'Domain events are written to an outbox table in the same transaction as the state change (ADR-003 gives us this for free). A Dramatiq relay task drains the outbox and publishes, marking rows only after the broker acknowledges.',
      'Consumers must deduplicate on event ID, since the relay guarantees at-least-once. Event payload schemas are versioned and documented alongside the API contract from ADR-015.',
    ],
  ],
  [
    'ADR-023', 'adr', 'proposed', 'Read replicas for reporting queries',
    [
      'Month-end reporting queries now contend with transactional load on the primary (ADR-003), and the finance team’s dashboards time out on the first of the month.',
      'Proposal: add one streaming read replica and route explicitly marked reporting queries to it through a separate session factory. Replication lag is surfaced as a Prometheus metric (ADR-010) with reporting queries refusing to run if lag exceeds five minutes.',
      'Explicitly out of scope: any general read/write splitting of transactional traffic. The ledger’s read-your-writes guarantees stay on the primary.',
    ],
  ],
  [
    'ADR-024', 'adr', 'accepted', 'Manage dependencies and environments with uv',
    [
      'This supersedes ADR-013. CI spent four minutes per run resolving and installing dependencies with pip-tools and pip; uv does the same work in seconds and replaces two tools with one.',
      'We adopt uv for lockfile management (uv.lock committed), local environments and CI installs. The Docker build (ADR-012) uses uv sync against the lockfile, keeping images reproducible byte-for-byte with respect to Python dependencies.',
      'pyproject.toml remains the single declaration of direct dependencies. requirements.in and requirements.txt are deleted.',
    ],
  ],
  [
    'ADR-025', 'adr', 'accepted', 'mypy --strict gates merges',
    [
      'Type annotations are only trustworthy if they are checked. Unchecked annotations rot into documentation that lies.',
      'mypy runs in strict mode over the whole package in CI and failures block merge, as encoded in STD-001. Third-party gaps are bridged with typed stubs or narrow, documented overrides in pyproject.toml; blanket ignores are banned.',
      'The Pydantic and SQLAlchemy plugins are enabled so model definitions (ADR-014, ADR-003) check end to end. New code lands fully typed; the legacy allowlist shrank to empty in release 3.1 and may not grow again.',
    ],
  ],
  [
    'ADR-026', 'adr', 'rejected', 'GraphQL gateway in front of the public API',
    [
      'Two integrators asked for more flexible queries, and a GraphQL gateway over the REST API (ADR-015) was prototyped during an innovation week.',
      'Rejected. The gateway added a second API contract to version and secure, resolver fan-out reintroduced exactly the N+1 query patterns ADR-016 was designed to prevent, and field-level authorization would duplicate the rules from ADR-008 in a second place.',
      'The underlying need is met more cheaply with sparse fieldsets and the expanded filter parameters shipped in /v2/. Revisit only if a concrete integrator need cannot be expressed in REST.',
    ],
  ],
  [
    'ADR-027', 'adr', 'accepted', 'Secrets from the platform vault at runtime',
    [
      'Database credentials and signing keys were arriving as plain environment variables set by hand in the deploy tooling, with no rotation story.',
      'Secrets are fetched at container start from the platform vault by a sidecar that renders them to a tmpfs file the application reads on boot; the application itself holds no vault credentials beyond its workload identity (ADR-007). Rotation is a restart, which ADR-028’s deployment model makes routine.',
      'Environment variables remain for non-secret configuration only. CI scans for secret-shaped strings in the image as a release gate alongside the checks in STD-001.',
    ],
  ],
  [
    'ADR-028', 'adr', 'deprecated', 'Blue-green deployments',
    [
      'Releases needed an instant rollback story while the schema expand/contract discipline from ADR-004 was still bedding in.',
      'We ran two full environments, blue and green, switching traffic at the load balancer after smoke tests, with rollback being a switch back.',
      'Deprecated since the platform team’s rolling-deploy primitive matured: it gives the same backwards-compatibility guarantees at half the standing cost. Kept for the historical record; the expand/contract migration discipline it enforced remains in force via ADR-004.',
    ],
  ],
  [
    'STD-001', 'standard', 'accepted', 'CI merge gates',
    [
      'Implements the enforcement halves of ADR-025 and ADR-004. A pull request may merge only when every gate passes; no human may override a red gate.',
      'Gates, in order: ruff lint and format check; mypy --strict (ADR-025); pytest with the coverage floor at 85% on changed files; Alembic migration check against an empty database (ADR-004); secret scan of the built image (ADR-027).',
      'The gate list changes only by amending this standard. CI configuration that drifts from this document is a bug in the configuration, not in the document.',
    ],
  ],
  [
    'STD-002', 'standard', 'accepted', 'Naming conventions for cache keys and events',
    [
      'Cache keys (ADR-006) and event names (ADR-022) are public-ish namespaces shared across services; collisions and renames are expensive.',
      'Cache keys are colon-separated: ledgerline:{domain}:{entity}:{id}:{version}, with the version segment bumped when the cached shape changes — there is no in-place invalidation. Event names are dotted past-tense facts: ledgerline.invoice.issued, versioned with a trailing .v2 only on breaking change.',
      'Both namespaces are catalogued in the repository, and review rejects additions that bypass the catalogue. A worked example and the reserved-word list live alongside this standard.',
    ],
  ],
];

// Untyped edges, [from alias, to alias] — Core emits only `relates-to`.
// The supersession/refinement stories stay told in body text. The last
// edge points at an alias that is not in the corpus, exercising the
// unresolved-target ("(not in corpus)") rendering the contract keeps.
const SAMPLE_EDGES = [
  ['ADR-014', 'ADR-002'],
  ['ADR-021', 'ADR-005'],
  ['ADR-024', 'ADR-013'],
  ['ADR-016', 'ADR-015'],
  ['ADR-008', 'ADR-007'],
  ['STD-002', 'ADR-006'],
  ['STD-001', 'ADR-025'],
  ['STD-001', 'ADR-004'],
  ['ADR-022', 'ADR-021'],
  ['ADR-006', 'ADR-005'],
  ['ADR-011', 'ADR-009'],
  ['ADR-011', 'ADR-010'],
  ['ADR-023', 'ADR-003'],
  ['ADR-017', 'ADR-021'],
  ['ADR-027', 'ADR-012'],
  ['ADR-028', 'ADR-004'],
  ['ADR-026', 'ADR-015'],
  ['ADR-020', 'ADR-006'],
  ['ADR-019', 'runbook-billing-period-rollover'],
];

const SAMPLE_DIRS = { adr: 'decisions', standard: 'standards' };

const sampleIdByAlias = new Map(
  SAMPLE.map(([key]) => [key, opaqueId(`ledgerline:${key}`)]),
);

const sampleExport = {
  schema_version: '1',
  corpus: {
    // The name itself carries the SAMPLE DATA label so any surface
    // that prints it is self-labelling.
    name: 'ledgerline — SAMPLE DATA (fictional service)',
    source: 'asdecided/ledgerline-sample',
    rac_version: '0.0.0-sample',
    artifact_count: SAMPLE.length,
    sample: true,
  },
  artifacts: SAMPLE.map(([key, type, status, title, paragraphs]) => {
    const short = key.toLowerCase();
    const long = `${short}-${slug(title)}`;
    return {
      id: sampleIdByAlias.get(key),
      aliases: [short, long],
      type,
      status: cap(status),
      title,
      path: `lore/${SAMPLE_DIRS[type]}/${long}.md`,
      body_html: html(paragraphs),
    };
  }),
  relationships: SAMPLE_EDGES.map(([from, to]) => ({
    from: sampleIdByAlias.get(from) ?? from,
    // Unresolved aliases are preserved verbatim, as Core emits them.
    to: sampleIdByAlias.get(to) ?? to,
    type: 'relates-to',
  })),
};

/* ------------------------------------------------------------------ */
/* 2. deterministic 500-artifact synthetic corpus                      */
/* ------------------------------------------------------------------ */

// Small LCG so the output is identical on every run.
function lcg(seed) {
  let s = seed >>> 0;
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 0x100000000;
  };
}

const rand = lcg(0x10ce55);
const pick = (arr) => arr[Math.floor(rand() * arr.length)];

const TOPICS = [
  'connection pooling', 'retry budgets', 'rate limiting', 'tenant isolation',
  'audit logging', 'payload compression', 'schema registry', 'bulk import',
  'webhook signing', 'session storage', 'index strategy', 'queue priorities',
  'circuit breaking', 'config reloads', 'PDF rendering', 'sandbox accounts',
  'data retention', 'currency conversion', 'batch windows', 'error taxonomy',
];
const VERBS = ['Adopt', 'Standardise', 'Constrain', 'Defer', 'Centralise', 'Split'];
const STATUSES = ['Accepted', 'Accepted', 'Accepted', 'Accepted', 'Proposed', 'Superseded', 'Deprecated', 'Rejected'];
const SENTENCES = [
  'The previous approach produced inconsistent behaviour across workers and made incidents slow to diagnose.',
  'We compared three options against operational cost, blast radius and the team’s existing experience.',
  'The decision applies to all new code immediately; existing call sites migrate as they are next touched.',
  'Rollout is gated behind a flag and monitored through the standard dashboards before becoming the default.',
  'A lint rule enforces the convention so review does not have to.',
  'The trade-off accepted here is additional operational surface in exchange for predictable failure modes.',
  'Measurements from the staging soak test informed the thresholds recorded below.',
  'Exceptions require a written waiver linked from the code in question.',
];

// Bodies cite other artifacts by alias ("ADR-042"); edges resolve to
// the opaque ids, all `relates-to` — the only type Core emits.
const synthId = (n) => opaqueId(`synthetic-500:${n}`);
const synthAlias = (n) => `adr-${String(n).padStart(3, '0')}`;

const synthArtifacts = [];
const synthEdges = [];
for (let i = 1; i <= 500; i++) {
  const topic = pick(TOPICS);
  const paragraphs = [];
  const n = 2 + Math.floor(rand() * 3);
  for (let p = 0; p < n; p++) {
    const refs = i > 3 && rand() < 0.4
      ? ` See ADR-${String(1 + Math.floor(rand() * (i - 1))).padStart(3, '0')} for background.`
      : '';
    paragraphs.push(`${pick(SENTENCES)} ${pick(SENTENCES)}${refs}`);
  }
  const title = `${pick(VERBS)} ${topic} (${i})`;
  const long = `${synthAlias(i)}-${slug(title)}`;
  synthArtifacts.push({
    id: synthId(i),
    aliases: [synthAlias(i), long],
    type: rand() < 0.85 ? 'adr' : 'standard',
    status: pick(STATUSES),
    title,
    path: `lore/decisions/${long}.md`,
    body_html: html(paragraphs),
  });
  if (i > 1) {
    const edgeCount = 1 + Math.floor(rand() * 2);
    for (let e = 0; e < edgeCount; e++) {
      const to = synthId(1 + Math.floor(rand() * (i - 1)));
      synthEdges.push({ from: synthId(i), to, type: 'relates-to' });
    }
  }
}

const synthExport = {
  schema_version: '1',
  corpus: {
    name: 'synthetic-500 — SAMPLE DATA (performance test corpus)',
    source: 'asdecided/synthetic-500',
    rac_version: '0.0.0-sample',
    artifact_count: synthArtifacts.length,
    sample: true,
  },
  artifacts: synthArtifacts,
  relationships: synthEdges,
};

/* ------------------------------------------------------------------ */
/* write                                                               */
/* ------------------------------------------------------------------ */

const samplePath = resolve(root, 'src/viewer/sample/lore-export.sample.json');
mkdirSync(dirname(samplePath), { recursive: true });
writeFileSync(samplePath, JSON.stringify(sampleExport, null, 2) + '\n');

const synthPath = '/tmp/lore-export-500.json';
writeFileSync(synthPath, JSON.stringify(synthExport));

console.log(`wrote ${samplePath} (${sampleExport.artifacts.length} artifacts, ${sampleExport.relationships.length} edges)`);
console.log(`wrote ${synthPath} (${synthExport.artifacts.length} artifacts, ${synthEdges.length} edges) — do not commit`);
