# DecisionGrounding graph-federation fixture

## inherits

```yaml
version: 2
parents:
  - alias: application
    source: eval/child
    root: child
    corpus: decisions
    digest: sha256-v2:989abefa424afc0cd3353b30fdcd61460868be9255a4dee3207e76601f672901
  - alias: policies
    source: eval/policy-tree
    root: policy-tree
    corpus: decisions
    digest: sha256-v2:de108483b57c703c901e0095f133036bf8c8622d18f228b428dd23a7d6bf10b8
  - alias: audit
    source: eval/audit
    root: audit
    corpus: decisions
    digest: sha256-v2:b6c3e3b33d73d0c281498cb55b6996a80080e4c6e2da3c16d547c481f3b0c1e4
```

## overrides

```yaml
version: 2
items: []
```
