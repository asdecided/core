# Corpus Federation

Corpus federation lets one repository inherit several independently owned
AsDecided corpora without flattening their identity or moving policy into a
hosted service. A root may have up to 32 direct parents; parents may have their
own parents, including shared ancestors reached through a diamond.

The contract is deliberately local and deterministic:

- every source is materialised inside the repository that declares it;
- every edge names a stable `corpus.source` and an exact content pin;
- the complete bounded graph is verified before any artifact is exposed;
- inherited sources and every physical route are read-only;
- no validation, lookup, enforcement, export, or MCP path performs networking;
- the root repository remains the only writable authoring layer.

## Manifest version 2

The root declares its direct parents in `.decided/corpus.md`:

````markdown
# Corpus

## inherits

```yaml
version: 2
parents:
  - alias: platform
    source: example/platform
    root: vendor/platform
    corpus: decisions
    digest: sha256-v2:<64 lowercase hexadecimal characters>
  - alias: security
    source: example/security
    root: vendor/security
    corpus: decisions
    digest: sha256-v2:<64 lowercase hexadecimal characters>
```
````

Each `root` is relative to the repository containing that manifest. The parent
must declare the same `corpus.source` in its own `.decided/config.yaml`. Parent
order grants no precedence.

A version-2 digest authenticates the source identity, exact config bytes,
nested-manifest presence and bytes, and every owned Markdown path and body. It
excludes nested materialisation subtrees, so graph updates are pinned from the
leaves upward:

```bash
decided corpus digest --version 2 \
  --root vendor/platform/vendor/shared \
  --corpus decisions

decided corpus digest --version 2 \
  --root vendor/platform \
  --corpus decisions
```

AsDecided never performs either materialisation or repinning automatically.

## Inspect the verified closure

`corpus status` verifies the complete closure from current local bytes before
rendering anything:

```bash
decided corpus status decisions/
decided corpus status decisions/ --json
```

The report includes:

- every logical source, layer, canonical pin, manifest version, and artifact count;
- the canonical source route and exact physical-route count;
- every declared edge, alias, pin, and materialisation path;
- catalog, root-effective, and root-local projection counts;
- override count, graph depth, and every read-only boundary.

The stable JSON form has `schema_version: "1"`. A successful report states
`pins_verified: true`, `network_access: false`, and
`immutable_snapshots: true`. If either side of a same-source diamond is
tampered, verification fails before stdout is emitted.

## Explain a resolution

`corpus explain` shows why a reference selects its answer:

```bash
decided corpus explain example/shared::SHR-01K000000001 decisions/
decided corpus explain SHR-01K000000001 decisions/ --json
decided corpus explain shared::SHR-01K000000001 decisions/ \
  --from example/platform
```

The report separates immutable historical candidates from the effective
terminal. It identifies whether the reference was qualified, lists visible
sources and direct aliases in the selected source context, and retains every
explicit override's owner, target, replacement, rationale, and provenance
state. Ambiguous and missing results still emit a structured explanation but
return exit code `1`.

Aliases are local to the source that declares the edge. A root cannot use a
transitive parent's alias as if it were global; use the durable
`source::canonical-id` form when crossing that boundary.

## Diamonds and overrides

Every physical edge is independently verified. If two routes reach the same
source with the same canonical pin, AsDecided retains both verified routes but
deduplicates their content into one logical source. Different pins for the same
source fail as a divergent graph instead of silently choosing one copy.

Equal artifact IDs in different sources remain different records. Bare lookup
is legal only when the visible historical candidates resolve to one effective
terminal. Policy replacement is never implicit: a manifest must map a globally
qualified inherited target to a same-type local artifact and cite a live local
Decision as its rationale.

## Git, not GitHub

Federation consumes a working tree; it does not depend on a particular forge,
remote name, default branch, pull-request API, or `.git` directory. A corpus can
be local-only or cloned from any standard Git remote. GitHub integrations such
as Actions and Code Scanning are optional delivery conveniences, not corpus
storage or resolution authority.

[Cursor Origin exposes ordinary Git clone, pull, and push](https://cursor.com/docs/origin/git),
so an Origin-native repository can hold exactly the same corpus without a core
engine adapter. Future forge-specific operations—creating repositories,
reviews, or checks—belong in optional integrations outside federation and must
not change local resolution.

## Runnable example

[`examples/federation/`](https://github.com/asdecided/core/tree/main/examples/federation)
contains a complete four-source diamond with two direct parents, two physical
copies of one shared source, and an explicit application override. It is
already materialised and pinned, so the status, explain, and validation commands
can be run directly after cloning the repository.

## Compatibility and limits

Repositories without a manifest retain single-corpus behavior. Manifest
version 1 retains its exact one-parent format, `sha256:` digest, and released
semantics. Version 2 is bounded to a depth of 16, 256 inherited sources, 1,024
edges, 4,096 overrides, and fixed file, byte, path, and YAML limits. Cycles,
duplicate direct sources, escaping paths, unsupported filesystem boundaries,
invalid inherited artifacts, stale pins, and limit excesses fail closed.
