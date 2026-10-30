<!--
id: api/ontology-crews
tier: engineering
status: design draft for review · decided, not built
answers: how an ontology crew is set up; what each member profile — classifier, term creator, curator — calls over MCP and HTTP; the classify → curate loop with ratify and reject; retirement and self-heal after a deprecation; the refusals an ontology crew meets
-->

# Ontology crews over MCP and HTTP

**Status:** design draft for review · decided, not built

An ontology crew is a crew of kind `ontology` whose only business is its vocabularies: it adds
terms, curates them, and attaches them to memories other agents wrote, so that recall by term
finds those memories without anyone rewriting them. Its members are external agents or people,
each holding one key whose [member profile](../architecture/roles-and-keys.md#member-profiles)
fixes what it may do: a **term creator** proposes terms, a **classifier** proposes
classifications, a **curator** decides both. This page is the worked flow over the
[HTTP API](http-api.md) and the [agent plugin](mcp-plugin.md), and the single declaration site of
which calls each profile makes ([ADR-0027](../adr/ADR-0027-ontology-crews.md) d13); every rule it
relies on is declared on the page it links. Nothing below exists as code, no transcript has run,
and review throughput, for classifications as for everything else, is unmeasured —
[FR-2](../product/roadmap.md).

## Setting the crew up

An admin creates the crew, its vocabulary and one key per member. The crew's kind is fixed at
creation; the vocabulary workarea must name an ontology crew
([workareas](../architecture/workareas.md#crews-recorded-not-operated)):

```console
$ oml crews create metals-terms --kind ontology
$ oml workareas create universe metals-vocab --crew metals-terms --prefix metal
$ oml crews add-member metals-terms metals-term-creator-1 --as term-creator \
    --vocabulary metals-vocab --out ~/.config/oml/keys/metals-term-creator-1
$ oml crews add-member metals-terms metals-classifier-1 --as classifier \
    --vocabulary metals-vocab --workarea metals-crew --out ~/.config/oml/keys/metals-classifier-1
$ oml crews add-member metals-terms metals-curator-1 --as curator \
    --vocabulary metals-vocab --workarea metals-crew --out ~/.config/oml/keys/metals-curator-1
$ oml crews add-member metals-terms metals-curator-h --as curator --held-by human \
    --vocabulary metals-vocab --workarea metals-crew --out ~/.config/oml/keys/metals-curator-h
```

Each `add-member` is one `POST /v2/keys` carrying the profile's grants, and prints only the two
export lines that read the value from its `--out` file
([CLI](../cli/reference.md#oml-crews-crew-records)). The human-attested curator is there because
the shipped vocabulary defaults require a human verdict on a deprecation with `replaced_by`, an
`exactMatch` mapping and a release ([ontologies](../architecture/ontologies.md#where-the-humans-sit)).
A key whose grants depart from its profile is never issued: `access.profile_mismatch` names the
clause it failed.

## What each member profile calls

| Profile | MCP tools it uses | HTTP beyond MCP |
|---|---|---|
| `classifier` | `oml_vocab` (suggest, get), `oml_recall`, `oml_get`, `oml_graph`, `oml_events_tail`, `oml_workarea_info`, `oml_relate_propose` (`kind: "claim"`, `label: "oml:about"`) | `GET /v2/claim-edges` |
| `term_creator` | `oml_vocab`, `oml_remember` (kinds `term`, `term_mapping`, `vocab_release`), `oml_get`, `oml_my_work`, `oml_workarea_info`, `oml_events_tail` | `POST /v1/memories/{artifact_id}/supersede` for its edits and deprecations, since `oml_supersede` refuses vocabulary kinds |
| `curator` | `oml_vocab`, `oml_get`, `oml_graph`, `oml_recall`, `oml_events_tail`, `oml_workarea_info`, `oml_relate_propose` | `/v2/review/*` on held vocabulary claims; `POST /v2/claim-edges/{edge_id}/ratify` and `/reject`; `GET /v2/claim-edges`; supersede for edit-as-proposal |

The catalog stays at its twelve tools, and no profile reaches promote, sign, demote, delete or an
admin verb on any surface (R1): a curator's verdicts, ratifications and rejections live on `/v2`
behind the dreamer(review) checks, as a [reviewer crew](reviewer-crews.md)'s do. The term creator
is the crew's one writer, so it proposes every new vocabulary claim — terms, mappings, releases —
and supersedes the terms whose tip its key authored; a term another key authored is revised by a
curator's edit-as-proposal. The curator is the crew's one proposer of `is_a` and other hierarchy
edges, and another curator ratifies each.

## The classifier's loop

A classifier reads promoted content where it holds reader — `lifecycle.promote` events on its
[events tail](mcp-plugin.md#oml_events_tail) say what is new — and decides what each memory is
about. That interpretation is the crew's own work; the provider runs no model
([ontologies](../architecture/ontologies.md#what-this-is-not)).

**1 — Find the term.** `oml_vocab` `suggest` and then `get` return the term's served tip,
including the `artifact_id` a classification names:

```json
{ "action": "get", "curie": "metal:aluminium" }
```

```json
{
  "curie": "metal:aluminium",
  "workarea_id": "6a1f3c2e-8b4d-4e7a-9c15-2f0d8e7b3a91",
  "artifact_id": "art_01JB2Q3S5U7W9Y1A3C5E7G9J2L",
  "version": 2,
  "term_status": "active",
  "definition_state": "present",
  "successors": [],
  "releases": ["rel_01JB2Z2D4F6H8K1M3P5R7TVXZB"]
}
```

**2 — Propose the classification.** `oml_relate_propose` rides `POST /v2/claim-edges`:

```json
{
  "kind": "claim",
  "label": "oml:about",
  "from": "art_01JB2XF2R7M9W4K6T8Y1QZ3VH5",
  "to": "art_01JB2Q3S5U7W9Y1A3C5E7G9J2L",
  "note": "the quote prices aluminium for Q1-2027 delivery"
}
```

```json
{
  "edge_id": "ce_01JB2YB2D4F6H8K1M3P5R7T9VX",
  "from_artifact_id": "art_01JB2XF2R7M9W4K6T8Y1QZ3VH5",
  "to_artifact_id": "art_01JB2Q3S5U7W9Y1A3C5E7G9J2L",
  "label": "oml:about",
  "term_curie": "metal:aluminium",
  "state": "pending",
  "retired": false,
  "proposer_key_id": "0c8e5a52-6f0d-4b6e-9a3f-2d7e1c9b4a10",
  "ratifier_key_id": null
}
```

The edge is pending: it confers nothing, matches no recall, and leaves the memory exactly as its
author signed it. The classifier holds dreamer only on the vocabulary, narrowed to `oml:about`
([narrowed dreamer grants](../architecture/roles-and-keys.md#narrowed-dreamer-grants-edge_labels)),
so the same proposal with any other label, a call to review next, or an edit-as-proposal answers
`access.grant_refused` with `details.edge_labels: ["oml:about"]`. The rules the proposal is held
to — source and target, binding to the version, one live classification per version and CURIE —
are declared on [ontologies](../architecture/ontologies.md#classification-ontology-crews-attach-terms-to-memories).

## The curator's loop

**1 — List what waits.** The curator's worklist is the pending classifications of its vocabulary:

```
GET /v2/claim-edges?workarea_id=metals-vocab&label=oml:about&state=pending
Authorization: Bearer <curator key>
```

**2 — Read both endpoints.** `oml_get` on the memory and on the term. A classification is
decided by one act with no serve, so reading what it joins is the curator's own diligence; a
curator that cannot read an endpoint cannot decide the edge (`access.not_found`).

**3 — Ratify or reject.**

```
POST /v2/claim-edges/ce_01JB2YB2D4F6H8K1M3P5R7T9VX/ratify
Authorization: Bearer <curator key>
Idempotency-Key: metals-terms/curate-2211/ratify-1
```

```json
{
  "edge_id": "ce_01JB2YB2D4F6H8K1M3P5R7T9VX",
  "from_artifact_id": "art_01JB2XF2R7M9W4K6T8Y1QZ3VH5",
  "to_artifact_id": "art_01JB2Q3S5U7W9Y1A3C5E7G9J2L",
  "label": "oml:about",
  "term_curie": "metal:aluminium",
  "state": "ratified",
  "retired": false,
  "proposer_key_id": "0c8e5a52-6f0d-4b6e-9a3f-2d7e1c9b4a10",
  "ratifier_key_id": "7d2b9e41-3c5a-4f86-b0e2-91a4c6d8f357"
}
```

From this commit, a recall that filters on `metal:aluminium` — or on an ancestor with
`expand_terms: descendants` — returns the memory beside every memory that cites the term itself
([retrieval](../architecture/retrieval.md#the-serving-tier)). A wrong classification, pending or
already ratified, is rejected instead, with a reason; the edge stays, as state `rejected`, and its
pair may be proposed again:

```
POST /v2/claim-edges/ce_01JB2YB2D4F6H8K1M3P5R7T9VX/reject
Authorization: Bearer <curator key>
Idempotency-Key: metals-terms/curate-2211/reject-1
Content-Type: application/json

{ "reason": "the quote names aluminium only as a comparison; the memory is about copper pricing" }
```

The same curator reviews the crew's held terms, mappings and releases through the ordinary
[review loop](reviewer-crews.md#the-loop) — queue, next, fetch, verdict — on the vocabulary.

## Independence

The curator is never the classifier: no key carries two profiles, and the ratify refuses the
proposer's key or principal (`review.ratifier_is_proposer`) and any key or principal in either
endpoint's lineage closure (`review.author_in_lineage`) — so the memory's author never decides a
classification of its own memory. A classification ratifies under its vocabulary's review policy.
Under the shipped default its axis is the principal, so a curator of the same ontology crew
ratifies. Where the vocabulary's policy requires crew inequality, that curator's ratify is
refused and the edge stays pending until a curator of another crew decides it:

```json
{
  "error": {
    "code": "review.ratifier_is_proposer",
    "rule": "R2 extends to edges: proposal and ratification are two identities, as far apart as the vocabulary's policy requires",
    "message": "the vocabulary metals-vocab requires crew inequality; ratifier and proposer are both in crew metals-terms",
    "details": { "axis": "crew" },
    "retryable": false
  }
}
```

A tenant that wants out-of-crew curation runs two ontology crews and grants each one's curators
on the other's vocabulary; a tenant that wants human-curated classifications gives the narrowed
content-side grants only to curators attested `held_by: human`. Either way the evidence export
shows every classifier↔curator pair and each crew's in-crew share
([audit and lineage](../architecture/audit-and-lineage.md#the-evidence-export)).

## When a term is deprecated

The deprecation of `metal:aluminium`, with `replaced_by: metal:aluminium-alloy`, is ratified on the
verdict of the human-attested curator the shipped defaults require. From that commit every
ratified classification to
`metal:aluminium` is **retired**: nothing is written for it, it is listed with `retired: true`,
and it stops matching recall. The classifier finds them —

```
GET /v2/claim-edges?term=metal:aluminium&label=oml:about&state=ratified
Authorization: Bearer <classifier key>
```

— reads the successor from `oml_vocab get` (or from the `vocab.inactive_curie` details a proposal
naming the deprecated term receives), and proposes a classification of the same memory version to
`metal:aluminium-alloy`. For `replaced_by` the rewrite is mechanical; for `consider` the
classifier judges which successor, if any, the memory is about. A curator ratifies the new edge
like any other; recall never follows the successor on its own
([ontologies](../architecture/ontologies.md#classification-ontology-crews-attach-terms-to-memories)).

## Refusals

| Code | Where it fires in these loops | Handling |
|---|---|---|
| `access.profile_mismatch` (403) | Issuing or amending a member key whose grants, role or `held_by` depart from its profile. | Fix the issuance; `details.clause` and `grant_index` name what failed. |
| `access.grant_refused` (403) | A classifier proposing another label, reviewing or editing; a curator ratifying on a content workarea it holds no narrowed grant on. | A provisioning error; `details.edge_labels` names what the key does hold. |
| `access.not_found` (404) | An endpoint the key cannot read. | Indistinguishable from a nonexistent one by design; check the reader grants. |
| `vocab.classification_exists` (409) | The version already has a live classification to the term, or cites it in its own `terms`. | Nothing to do; `details` names the existing edge or the citation. |
| `vocab.inactive_curie` (422) | Proposing or ratifying a classification to a deprecated term. | Self-heal: propose to a successor from `details.successors`. |
| `review.ratifier_is_proposer` (403) | A curator deciding its own proposal; a ratify that cannot meet the vocabulary's crew or manager axis (`details.axis`). | Route to another curator — of another crew where `axis` says so. |
| `review.author_in_lineage` (403) | A curator in either endpoint's lineage closure — the memory's or the term's author or editor. | Route to a curator outside both closures. |
| `request.validation` (400) | A source that is not a promoted memory or derived fact outside every vocabulary, a target that is not a promoted term, a rejection of a non-`oml:about` edge. | A client defect. |

## Decision record

Ontology crews, member profiles, `edge_labels`, classification and this page's table of calls are
the proposed [ADR-0027](../adr/ADR-0027-ontology-crews.md)'s; vocabularies are
[ADR-0021](../adr/ADR-0021-ai-human-ontologies.md)'s and crew records
[ADR-0026](../adr/ADR-0026-names-are-keys.md)'s. Apart from that table, this page adds no
decision — it is the worked consumer flow over decisions those records own.
