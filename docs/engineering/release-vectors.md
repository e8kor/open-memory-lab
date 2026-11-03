<!--
id: engineering/release-vectors
tier: engineering
status: design draft for review · the vectors reproduce in oml-domain and outside Rust; the vocab_release ratification that would build a release is decided, not built
answers: the test vectors for a vocabulary release's canonical document and digest and for the diff between two releases — each vector's term tips and edges, its RFC 8785 bytes and SHA-256 — how the domain's Rust test and the independent Python check each reproduce them, and which shape the diff vectors use
-->

# Vocabulary release vectors

**Status:** design draft for review · the vectors reproduce in oml-domain and outside Rust; the vocab_release ratification that would build a release is decided, not built

[ADR-0021](../adr/ADR-0021-ai-human-ontologies.md) d4 has a `vocab_release` ratification
materialize the `is_a` closure and serve the release as a diff against the previous one
([ontologies](../architecture/ontologies.md#releases-and-the-materialized-closure)). The domain
builds that snapshot as one RFC 8785 document with its SHA-256 digest, and diffs two snapshots,
in `crates/oml-domain/src/vocab_release.rs` (`build_release`, `diff_releases`); the canonical
form and the `sha256:` spelling are [chain format](../data/chain-format.md)'s. This page holds
the vectors those functions must reproduce byte for byte, and it is the fixture both checks read:

- `crates/oml-domain/tests/release_vectors.rs` reads this page with `include_str!`, builds each
  release from its input with `build_release`, diffs with `diff_releases`, and compares the bytes,
  the byte count and the digest. It keeps no copy of any vector.
- `scripts/check_release_vectors.py` rebuilds every vector with code of its own: an all-pairs
  shortest-path closure where the Rust code walks breadth first, `rfc8785` 0.1.4 for the
  canonical bytes and `hashlib` for SHA-256, pinned in `scripts/check_release_vectors.py.lock`.
  CI job `release-vectors` in `.github/workflows/m0-spikes.yml` runs it as
  `uv run --python 3.13 --locked --script scripts/check_release_vectors.py`.

Each fenced block of a vector follows an HTML comment that names the vector and the block, such
as `vector: R2 input`, which the rendered page does not show; edit a vector's blocks together, or
both checks fail.

## The release document

The document is the RFC 8785 form of
`{"closure": [{"ancestor", "depth", "descendant"}, …], "edges": [{"from", "label", "to"}, …],
"terms": [{"curie", "successors": [{"curie", "relation"}, …], "term_status"}, …]}`, as
`ReleaseSnapshot` documents it: closure rows by `ancestor` then `descendant`, edges by `label`,
`from` then `to`, terms by `curie`, and a `consider` term's successors by `curie`, each in
code-point order and once each. Only `oml:is_a` and `oml:replaced_by` edges enter it, a CURIE
pair once however many version pairs carry it, and each closure row's `depth` is the fewest
ratified `is_a` edges joining its pair. The digest is not the Merkle root ADR-0021 d4 pins a
release by, whose construction is RS-4's and still open ([roadmap](../product/roadmap.md)).

Each input lists `tips` and `edges` in an order that is deliberately not canonical, with a
repeated edge, a repeated `consider` successor and edges of labels the release drops, so that
the canonicalizer and the snapshot do the sorting and the deduplication.

### Vector R1 — the empty release

No tips and no edges.

<!-- vector: R1 input -->
```json
{"tips": [], "edges": []}
```

<!-- vector: R1 canonical -->
```json
{"closure":[],"edges":[],"terms":[]}
```

<!-- vector: R1 digest -->
```text
bytes  = 36
sha256 = sha256:fcb0a09c7fa866f4aaed080d36ac384d22ec22131144a7f8f49907e33062d04a
```

### Vector R2 — a hierarchy with a shortcut

Four active terms. `biz:voluntary-churn` is_a `biz:churn-risk` is_a `biz:risk`, and a ratified
shortcut puts `biz:voluntary-churn` directly below `biz:risk` too, so that row has depth 1.
`biz:churn-risk` is_a `biz:risk` is carried twice, as two version pairs carry it, and an
`oml:part_of` edge is not released.

<!-- vector: R2 input -->
```json
{
  "tips": [
    {"curie": "biz:voluntary-churn", "term_status": "active"},
    {"curie": "biz:risk", "term_status": "active"},
    {"curie": "biz:attrition", "term_status": "active"},
    {"curie": "biz:churn-risk", "term_status": "active"}
  ],
  "edges": [
    {"label": "oml:is_a", "from": "biz:voluntary-churn", "to": "biz:risk"},
    {"label": "oml:is_a", "from": "biz:churn-risk", "to": "biz:risk"},
    {"label": "oml:part_of", "from": "biz:attrition", "to": "biz:risk"},
    {"label": "oml:is_a", "from": "biz:voluntary-churn", "to": "biz:churn-risk"},
    {"label": "oml:is_a", "from": "biz:attrition", "to": "biz:risk"},
    {"label": "oml:is_a", "from": "biz:churn-risk", "to": "biz:risk"}
  ]
}
```

<!-- vector: R2 canonical -->
```json
{"closure":[{"ancestor":"biz:churn-risk","depth":1,"descendant":"biz:voluntary-churn"},{"ancestor":"biz:risk","depth":1,"descendant":"biz:attrition"},{"ancestor":"biz:risk","depth":1,"descendant":"biz:churn-risk"},{"ancestor":"biz:risk","depth":1,"descendant":"biz:voluntary-churn"}],"edges":[{"from":"biz:attrition","label":"oml:is_a","to":"biz:risk"},{"from":"biz:churn-risk","label":"oml:is_a","to":"biz:risk"},{"from":"biz:voluntary-churn","label":"oml:is_a","to":"biz:churn-risk"},{"from":"biz:voluntary-churn","label":"oml:is_a","to":"biz:risk"}],"terms":[{"curie":"biz:attrition","successors":[],"term_status":"active"},{"curie":"biz:churn-risk","successors":[],"term_status":"active"},{"curie":"biz:risk","successors":[],"term_status":"active"},{"curie":"biz:voluntary-churn","successors":[],"term_status":"active"}]}
```

<!-- vector: R2 digest -->
```text
bytes  = 825
sha256 = sha256:1b18ba891e672c559b8495b2ef9b6a9f1b28418f8897b7df617c57c865ffdeef
```

### Vector R3 — the next release: a deprecation, terms added, the shortcut removed

`biz:attrition` is deprecated, `replaced_by` `biz:churn-risk`, with the `oml:replaced_by` edge
its deprecation carries. `biz:involuntary-churn` is added below `biz:churn-risk`.
`biz:flight-risk` is added already deprecated, `consider` `biz:voluntary-churn` and
`biz:churn-risk`, the first named twice; its `oml:consider` edge is not released. The shortcut
is gone, so `biz:voluntary-churn` lies two edges below `biz:risk`.

<!-- vector: R3 input -->
```json
{
  "tips": [
    {"curie": "biz:risk", "term_status": "active"},
    {"curie": "biz:flight-risk", "term_status": "deprecated",
     "successors": {"relation": "consider",
                    "curies": ["biz:voluntary-churn", "biz:churn-risk", "biz:voluntary-churn"]}},
    {"curie": "biz:voluntary-churn", "term_status": "active"},
    {"curie": "biz:attrition", "term_status": "deprecated",
     "successors": {"relation": "replaced_by", "curies": ["biz:churn-risk"]}},
    {"curie": "biz:involuntary-churn", "term_status": "active"},
    {"curie": "biz:churn-risk", "term_status": "active"}
  ],
  "edges": [
    {"label": "oml:replaced_by", "from": "biz:attrition", "to": "biz:churn-risk"},
    {"label": "oml:is_a", "from": "biz:involuntary-churn", "to": "biz:churn-risk"},
    {"label": "oml:consider", "from": "biz:flight-risk", "to": "biz:voluntary-churn"},
    {"label": "oml:is_a", "from": "biz:voluntary-churn", "to": "biz:churn-risk"},
    {"label": "oml:is_a", "from": "biz:churn-risk", "to": "biz:risk"},
    {"label": "oml:is_a", "from": "biz:attrition", "to": "biz:risk"}
  ]
}
```

<!-- vector: R3 canonical -->
```json
{"closure":[{"ancestor":"biz:churn-risk","depth":1,"descendant":"biz:involuntary-churn"},{"ancestor":"biz:churn-risk","depth":1,"descendant":"biz:voluntary-churn"},{"ancestor":"biz:risk","depth":1,"descendant":"biz:attrition"},{"ancestor":"biz:risk","depth":1,"descendant":"biz:churn-risk"},{"ancestor":"biz:risk","depth":2,"descendant":"biz:involuntary-churn"},{"ancestor":"biz:risk","depth":2,"descendant":"biz:voluntary-churn"}],"edges":[{"from":"biz:attrition","label":"oml:is_a","to":"biz:risk"},{"from":"biz:churn-risk","label":"oml:is_a","to":"biz:risk"},{"from":"biz:involuntary-churn","label":"oml:is_a","to":"biz:churn-risk"},{"from":"biz:voluntary-churn","label":"oml:is_a","to":"biz:churn-risk"},{"from":"biz:attrition","label":"oml:replaced_by","to":"biz:churn-risk"}],"terms":[{"curie":"biz:attrition","successors":[{"curie":"biz:churn-risk","relation":"replaced_by"}],"term_status":"deprecated"},{"curie":"biz:churn-risk","successors":[],"term_status":"active"},{"curie":"biz:flight-risk","successors":[{"curie":"biz:churn-risk","relation":"consider"},{"curie":"biz:voluntary-churn","relation":"consider"}],"term_status":"deprecated"},{"curie":"biz:involuntary-churn","successors":[],"term_status":"active"},{"curie":"biz:risk","successors":[],"term_status":"active"},{"curie":"biz:voluntary-churn","successors":[],"term_status":"active"}]}
```

<!-- vector: R3 digest -->
```text
bytes  = 1355
sha256 = sha256:d258395015e31f05681ddf37b8c96e3b0c29f2c77a7c87e4cf2d180b765ebd24
```

## The diff

`diff_releases` returns a `ReleaseDiff` value, and no page declares a serialized form of it: the
diff artifacts that enter the served digest set ([admission](../architecture/admission.md)) are
decided, not built. So that a diff vector can be compared byte for byte, the vectors write a diff
in a shape of this page's own, used by the two checks and by nothing else: the RFC 8785 form of
`{"closure_added", "closure_removed", "deprecations", "edges_added", "edges_removed",
"terms_added"}`, whose closure rows, edges and terms are the release document's objects, each
array in the release document's order, and whose `deprecations` are
`{"curie", "successors": [{"curie", "relation"}, …]}` by `curie`. A pair whose depth changed is
removed at the old depth and added at the new. A diff vector's input names its two releases.

### Vector D1 — from the empty release to R2

Everything R2 holds is added.

<!-- vector: D1 input -->
```json
{"previous": "R1", "next": "R2"}
```

<!-- vector: D1 canonical -->
```json
{"closure_added":[{"ancestor":"biz:churn-risk","depth":1,"descendant":"biz:voluntary-churn"},{"ancestor":"biz:risk","depth":1,"descendant":"biz:attrition"},{"ancestor":"biz:risk","depth":1,"descendant":"biz:churn-risk"},{"ancestor":"biz:risk","depth":1,"descendant":"biz:voluntary-churn"}],"closure_removed":[],"deprecations":[],"edges_added":[{"from":"biz:attrition","label":"oml:is_a","to":"biz:risk"},{"from":"biz:churn-risk","label":"oml:is_a","to":"biz:risk"},{"from":"biz:voluntary-churn","label":"oml:is_a","to":"biz:churn-risk"},{"from":"biz:voluntary-churn","label":"oml:is_a","to":"biz:risk"}],"edges_removed":[],"terms_added":[{"curie":"biz:attrition","successors":[],"term_status":"active"},{"curie":"biz:churn-risk","successors":[],"term_status":"active"},{"curie":"biz:risk","successors":[],"term_status":"active"},{"curie":"biz:voluntary-churn","successors":[],"term_status":"active"}]}
```

<!-- vector: D1 digest -->
```text
bytes  = 901
sha256 = sha256:0accb36393277b8e72c876aa1819e45e3ae9983cf26487fdbf6bc2ccb09c5256
```

### Vector D2 — from R2 to R3

Two terms added, one deprecated with its successor, two edges added and one removed, and the
shortcut's closure row removed at depth 1 and added at depth 2.

<!-- vector: D2 input -->
```json
{"previous": "R2", "next": "R3"}
```

<!-- vector: D2 canonical -->
```json
{"closure_added":[{"ancestor":"biz:churn-risk","depth":1,"descendant":"biz:involuntary-churn"},{"ancestor":"biz:risk","depth":2,"descendant":"biz:involuntary-churn"},{"ancestor":"biz:risk","depth":2,"descendant":"biz:voluntary-churn"}],"closure_removed":[{"ancestor":"biz:risk","depth":1,"descendant":"biz:voluntary-churn"}],"deprecations":[{"curie":"biz:attrition","successors":[{"curie":"biz:churn-risk","relation":"replaced_by"}]}],"edges_added":[{"from":"biz:involuntary-churn","label":"oml:is_a","to":"biz:churn-risk"},{"from":"biz:attrition","label":"oml:replaced_by","to":"biz:churn-risk"}],"edges_removed":[{"from":"biz:voluntary-churn","label":"oml:is_a","to":"biz:risk"}],"terms_added":[{"curie":"biz:flight-risk","successors":[{"curie":"biz:churn-risk","relation":"consider"},{"curie":"biz:voluntary-churn","relation":"consider"}],"term_status":"deprecated"},{"curie":"biz:involuntary-churn","successors":[],"term_status":"active"}]}
```

<!-- vector: D2 digest -->
```text
bytes  = 944
sha256 = sha256:24e28c7ce19d6f603c0e0ac2539bb622cbf3979275f69e4969d677fb9c4ac84c
```
