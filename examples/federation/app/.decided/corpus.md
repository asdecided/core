# Application corpus

## inherits

```yaml
version: 2
parents:
  - alias: platform
    source: example/platform
    root: vendor/platform
    corpus: decisions
    digest: sha256-v2:9c0a54055aa8417089d100645d3e4e2288b6e9040ddb78b8797c8a70df5d3f63
  - alias: security
    source: example/security
    root: vendor/security
    corpus: decisions
    digest: sha256-v2:f9f225328f14138998246f91ea753a9771f8c903879f97ac9e4ba4707de60746
```

## overrides

```yaml
version: 2
items:
  - target: example/shared::SHR-01K000000001
    with: APP-01K000000001
    rationale: ADR-01K000000001
```
