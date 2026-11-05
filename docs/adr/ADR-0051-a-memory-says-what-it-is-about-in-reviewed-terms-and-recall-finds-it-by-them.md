<!--
id: adr/ADR-0051-a-memory-says-what-it-is-about-in-reviewed-terms-and-recall-finds-it-by-them
tier: engineering
status: proposed
answers: how OML attaches metadata to memories without a model on the server; whether that metadata is free keywords per category or terms of a reviewed vocabulary; what a facet is; what a term carries (labels, synonyms, a stable id) and where it lives; which parts of the parked ADR-0027 this unparks and which stay parked; how an agent chooses terms at write time and how the server helps it choose; how a new concept becomes a term; how an about link is reviewed; how recall uses terms (synonyms, facet filters, a term list fused with the keyword lists) and why it does not expand the query text; what a recall says about a term match; who sees which vocabulary (a user root's own, universe's curated one) and what stays anonymous; why the constitution holds, R3 in particular; how a crew starts with no vocabulary; the doctor lines; the FR rows; the build increments and their gates; what the offline measurement found
-->

# ADR-0051 — A memory says what it is about in reviewed terms, and recall finds it by them

**Status:** proposed · 2026-11-04

**Release:** not assigned ([releases](../product/releases.md#decisions))

**Amends:** ADR-0021 (d8: `terms` on a claim gives way to `about` edges; d9: no `oml_vocab`
tool, the read rides `oml_read`, and recall's term filter takes a facet; d10: a client-side SKOS
import), ADR-0027 (proposed, on branch `feat/ontology-crews`: d6, d7, d9, d10 and d11 unparked,
with a classification proposed by the memory's own author at write time and ratified with the
memory; d1–d5, d8 and d12–d14 stay parked), ADR-0044 (d3: a user key's reader grant on
`universe` reaches universe's vocabulary nodes), ADR-0046 (d5 admits an `oml:about` edge from a user root's memory to a universe term;
d6's refusal of a standalone `oml:about` proposal stands), ADR-0049 (proposed: a `term` retriever kind and the pipelines that fuse it), ADR-0050 (proposed,
on branch `feat/remember`: `oml_write` gains `about`; the answer gains `candidate_terms`)

Mockups: [ontology mockups](../design/ontology/mockups.html) (a write that is offered terms and
chooses them, a recall that finds `secrets` through `credentials`, the vocabulary page, the
reviewer's view, the measured chart and the build).

This record takes the number after ADR-0050, the highest on any branch on 2026-11-04 (ADR-0049
is the search lane's, `feat/search-port`); its roadmap rows start at FR-95, after ADR-0050's
FR-90 to FR-94.

## Context

**The operator's question, 2026-11-04.** "Should we introduce some smart way to extract metadata
from claims and memories — can we use our ontology, or should each document have a few sets of
keywords, one per ontology category it belongs to?" A memory today carries its text, its kind and
its `claim_key`. ADR-0050 d5 (proposed) adds a title and up to eight free keywords, written by the
author and reviewed with the text. Nothing says what a memory is *about* in words two writers
share: one agent writes "credentials", another "secrets", and a recall for either misses the
other unless both words are in the text.

**OML already designed shared words, and parked them.** [ADR-0021](ADR-0021-ai-human-ontologies.md)
(accepted design, not built) hosts vocabularies as claims: a vocabulary is a workarea with a
prefix, a term is a claim of kind `term` whose `claim_key` is a CURIE, deprecation is supersession
with a successor edge. It gave a claim a `terms` field inside its signed bytes and an `oml_vocab`
MCP tool. ADR-0027 (proposed 2026-10-30, parked) added ontology crews with three member profiles
and a classification: a claim edge `oml:about` from a memory version to a term, proposed by a
classifier and ratified by a curator. Neither fits today's surface: ADR-0039 d8 allows no fourth
MCP tool, `oml_relate_propose` and `oml_remember` are gone, and ADR-0046 d6 (proposed) refuses
`oml:about` as a standalone proposal. The domain code for CURIEs, term edges, releases and suggest
exists on `feat/vocab-domain` with no route; ADR-0046 replays `vocab` and `vocab_edges` for its
claim edges. Term labels are not designed: the suggest module records that "the docs give a
suggestion a `label` but name no field of a term".

**What others do, and what it costs them.** The retrieval-heavy memory products extract entities
with a model at write time. Mem0's graph variant lost to its plain variant on single-hop (65.71
against 67.13) and multi-hop questions (47.19 against 51.15) while doubling p95 latency (2.59 s
against 1.44 s) ([Mem0 paper](https://arxiv.org/html/2504.19413)), and Mem0 later dropped its
external graph store for "entity search" as one signal fused with semantic and BM25 search, the
graph's "standalone `relations` field … no longer populated"
([Mem0 graph memory](https://docs.mem0.ai/open-source/features/graph-memory),
[Mem0 evaluation](https://docs.mem0.ai/core-concepts/memory-evaluation), VENDOR). A study of
GraphRAG found "about 65.8% of answer entities appear in the constructed KG" for HotPotQA, and
that "GraphRAG is not free: it often incurs higher construction cost, retrieval latency, and
storage footprint" ([RAG vs GraphRAG](https://arxiv.org/html/2502.11371v3)). MemoryAgentBench
measured Mem0's construction at 10,804 s and Cognee's at 11,890 s on one multi-hop setting,
against 85–433 s for MemGPT, and scored BM25 at 41.5 overall against Zep, Mem0 and Cognee at 24.0,
21.1 and 20.6 ([MemoryAgentBench](https://arxiv.org/html/2507.05257)). LongMemEval raised BM25's
recall@5 from 0.634 to 0.683 by indexing short extracted facts beside each session
([LongMemEval](https://arxiv.org/html/2410.10813v2)). Read together: extraction costs a model call
per write and loses facts; an entity match used as one ranking signal is the form that survived.

**OML's recall eval measured the alternative, offline.** On 2026-11-04, on this branch, a
vocabulary of 80 terms in four facets, each term with a preferred label and alternative labels
(541 labels), and 0 to 3 terms on each of the eval's 200 memories, were written blind to the 121
queries and committed before the queries were read
([eval README](https://github.com/e8kor/open-memory-lab/blob/design/ontology-metadata-r2/eval/recall/ontology/README.md),
`eval/recall/ontology/`). Every number is k = 10, 95% intervals paired over queries:

| arm | success@1 | success@5 | nDCG@10 | paraphrase success@5 |
|---|---:|---:|---:|---:|
| `pg_simple_or` (ADR-0048 S1), reproduced exactly | 0.49 | 0.73 | 0.59 | 0.55 |
| + memories tagged with a term the query names, fused by reciprocal rank | 0.60 | 0.85 | 0.74 | 0.64 |
| BM25 (S2) | 0.79 | 0.92 | 0.84 | 0.64 |
| BM25 + every label of a matched term added to the query text | 0.72 | 0.93 | 0.83 | 0.82 |
| **BM25 + the term list, fused** | **0.79** | **0.94** | **0.88** | **0.82** |
| the same, tags a label matcher attaches with no judgement | 0.76 | 0.93 | 0.86 | 0.82 |
| the same, a third of the tags dropped and a fifth replaced at random | 0.54 | 0.91 | 0.74 | 0.64 |
| dense, `bge-small-en-v1.5` (committed) | 0.79 | 0.95 | 0.85 | 0.82 |
| hybrid, BM25 and dense fused (committed) | 0.79 | 0.97 | 0.88 | 0.82 |

The term list over BM25 gains +0.032 nDCG@10 (interval +0.003 to +0.061; 30 queries better, 11
worse) and ties hybrid (−0.03 to +0.03). Expanding the query text with the same labels made 44
queries worse and 26 better. Noisy tags cost −0.106 (−0.147 to −0.064; 55 worse). The queries the
term list hurt matched a broad term through an ordinary word: "planned", "committed", "invoices".
One model wrote the corpus, the queries, the vocabulary and the tags, and five query fragments
were known before the vocabulary was written, so the paraphrase gain is an upper bound; 11
queries a category move a cell by 0.09; no unanswerable query is in the set.

**What a reviewed vocabulary is, honestly.** No memory product found curates its vocabulary: the
others extract entities, unreviewed, from each write. A vocabulary whose terms are reviewed claims,
and whose links from memories are reviewed with the memory, is something OML can offer that they
do not, and the eval says it buys roughly what an embedding index buys on this set. It costs a
curator's time per term, a reviewer's attention per link, and a crew that starts with no terms and
gains nothing until it has them; the eval also says it helps only while the links are right.
Decided, not built.

## Decision

- **d1 — A facet is a vocabulary; a crew's metadata is a few small vocabularies, and no model
  extracts it.** A **facet** is an [ADR-0021](ADR-0021-ai-human-ontologies.md) vocabulary — a
  workarea with a prefix — whose terms answer one question about a memory: which component
  (`cmp:`), which technology (`tech:`), which topic (`topic:`), which process (`proc:`), which team
  (`team:`). A crew uses one to six facets; a workarea's `vocabulary_policy` (ADR-0021 d8) names
  the list it uses, which this record amends from one vocabulary to `vocabularies: [...]`. A facet
  is a vocabulary workarea and nothing new: its prefix, review policy, grants, deprecation and
  permanence are ADR-0021's. No server component reads a memory's text to decide its terms, and
  no model runs: the server matches labels (d4), and the writing agent, which already read and
  wrote the text, chooses. The cost is that a facet's prefix is permanent once registered
  (ADR-0021 d2), so a crew chooses its facets before its first term.

- **d2 — A term carries a preferred label, alternative labels and a definition; its CURIE is
  its stable id.** A term claim's body gains `pref_label` (one, NFC, at most 60 characters),
  `alt_labels` (at most 24, each at most 60 characters: synonyms, abbreviations, product names,
  plural and colloquial forms) and `definition` (at most 500 characters), reviewed with the term
  like any claim's text and erased with it. The CURIE is the term's `claim_key` (ADR-0021 d2) and
  never changes; a label edit is an edit of the term, a new version under the same CURIE. Within
  one facet a label, case-folded and NFC, names one term: a term whose label another active term
  of the facet holds is refused `vocab.label_conflict` (409, `details: {label, curie}`); across
  facets a label may repeat (`topic:deployment` and `tech:kubernetes` may both say "deploy"), and
  d4 reports both. These are SKOS's `skos:prefLabel`, `skos:altLabel` and `skos:definition`, so
  d10's import maps one to one. This fills the gap the suggest module recorded and amends
  ADR-0021 d1's term body.

- **d3 — An `about` link is an `oml:about` claim edge from the memory version to a term,
  proposed by the memory's author in the write and ratified by the verdict that promotes the
  memory.** This unparks ADR-0027 d6's edge (shape, version binding, the memory untouched), d7
  (`oml:about` in the `oml:` upper vocabulary, one live link per version and CURIE), d9 (not
  derivation: out of the lineage closure and out of taint) and d10 (a link to a deprecated term
  retires by derivation and stops matching). What changes is who proposes and who ratifies: the
  author proposes, in the write, as ADR-0050 d9 proposes `links`; the edges are held with the
  claim and ratified by the verdict that promotes it, whose reviewer is never the author (R2), and
  dropped with its rejection. A reviewer approves or rejects the memory with its terms; no verdict
  adds, removes or changes a term, so review never changes what a memory is found by. A
  supersession without `about` re-proposes the replaced version's terms, shown to the reviewer as
  carried; `about: []` proposes none. ADR-0021 d8's `terms` field inside the signed bytes is
  dropped for memories: one way to say what a memory is about, not two that must agree. The
  vocabulary scanner of ADR-0021 d8 checks the `about` targets instead, under the same
  `require | annotate | off`. Still parked from ADR-0027: crew kinds and member profiles (d1–d4),
  narrowed `edge_labels` grants (d5), the curator's reject (d8), classification of existing
  memories by a classifier and its policy (d12–d14). A wrong term on a promoted memory is fixed by
  its author's supersession until those are unparked (FR-97).

- **d4 — `oml_write` takes `about`, at most three terms, and the server offers candidates by
  matching labels, never by a model.** `about` is a list of at most 3 items, each a CURIE, a label
  that names exactly one readable active term, or a new-term proposal `{label, facet,
  alt_labels?, definition}` (d5). A label naming no term answers `vocab.unknown_curie` (422,
  ADR-0021) with `details.candidates`; a label naming terms in two facets answers
  `request.validation` with `details.candidates`, so the agent resends the CURIE. Three is the
  eval's cap, chosen not measured (FR-97). Two reads help the agent choose, both lexical:
  `oml_read {terms_for: <text>}` returns up to 8 readable active terms whose labels occur in the
  text as contiguous runs of stemmed words, best first (more labels matched, then a multi-word
  label, then the preferred label, then CURIE), each with its facet, the label that matched and
  its definition; and every `oml_write` answer carries `candidate_terms`, the same list for the
  written text, minus the terms chosen. An agent that skipped the read revises its held version
  (ADR-0050 d3) to add a term it missed. The server never attaches a candidate: on the eval, tags
  a label matcher attaches with no judgement gained +0.012 nDCG@10, indistinguishable from zero
  (−0.017 to +0.040), against +0.032 for chosen ones. `oml_read` and `oml_write` gain fields, not
  tools (ADR-0039 d8). The workarea read (`oml_workarea`) serves `vocabularies`: each facet's
  prefix, name and term count, so an agent knows which facets apply where it writes.

- **d5 — A concept with no term is proposed as a term, held until a curator promotes it, and
  the link waits with it.** A new-term item in `about` writes a held claim of kind `term` into the
  named facet, authored by the writing key, which therefore needs writer on the facet's workarea,
  ADR-0021 d8's ordinary grant shape ("a vocabulary workarea grants writer to the crews it
  serves"); without it, `access.grant_refused`. Its CURIE is `<prefix>:<slug of the label>` unless
  the item names one, minted at first promotion (ADR-0021 d2). The term is reviewed under the
  facet's review policy by its curators, never by its author; the memory is reviewed under its own
  workarea's policy, and the two verdicts are independent. The link from the memory is ratified
  with the memory and matches recall only once the term is promoted and active, since an edge is
  served only to a key reading both endpoints (ADR-0046 d5). A rejected term leaves the link
  inert, kept, and listed on the author's write as `about_inert`. The answer of a write that
  proposes a term says so: `proposed_terms: [{curie, state: "held"}]`.

- **d6 — Recall finds memories by the terms a query names, through a `term` retriever fused with
  the keyword lists, and does not add synonyms to the query text.** ADR-0049's `Retriever` port
  gains the retriever kind `term`: the core hands it the candidate scope as for every retriever
  (ADR-0049 d2); it matches the query against the labels of the active terms the key may read,
  by d4's matcher, and returns the memories in scope whose ratified, unretired `oml:about` edges
  join a matched term, ordered by the number of matched terms, then by the first keyword list's
  order, then by id. Two pipelines join ADR-0049 d4's closed set, enabled and not default:
  `bm25-term` (`bm25` and `term`, reciprocal rank 60) and `bm25-trigram-term` (`bm25`, trigrams
  and `term`). Synonyms work at the term: "credentials" matches `topic:secrets` through its
  alternative label and the term list brings the memories tagged `topic:secrets`, whatever their
  text says. The query text is never rewritten with labels: on the eval that made 44 queries
  worse and 26 better over BM25 and cost success@1 0.79 → 0.72. An additive score boost per
  matched term is not served either: over the expanded query it gained nothing with curated tags
  (−0.004 nDCG@10). A
  pipeline with `term` becomes a default only by ADR-0049 d9's gate, on eval v2 with a vocabulary
  and tags written independently of its queries (FR-95).

- **d7 — A recall may filter by term or by facet, inside the scan.** `oml_read` in recall mode
  and `POST /v1/memories/recall` take `about`: up to 8 CURIEs or facet prefixes (`tech:`), a hit
  matching any of them. A hit matches a CURIE through a ratified, unretired `oml:about` edge of
  its version (ADR-0027 d11, without the dropped `terms` citations), and a facet through any such
  edge to a term of that facet. It is ADR-0048 S4's filter: a predicate in the candidate scope
  before ranking, so a filtered recall never under-returns. `expand_terms: descendants` (ADR-0021
  d9) stays with the `is_a` closure, which this record does not unpark.

- **d8 — Every hit says which term matched and through which label.** ADR-0049 d6's `matched`
  gains `terms`: for each matched term the CURIE, the preferred label and the query words that
  matched one of its labels (`{"curie": "topic:secrets", "pref_label": "secrets", "via":
  "credentials"}`), and `via` gains `term`. A reader sees why a memory that does not contain
  "credentials" answered a query that did, and a curator sees which labels draw false matches
  ("committed" → `proc:git-workflow` on the eval). The recall result's `search.terms` lists the
  terms the query matched, empty when none did.

- **d9 — Terms, links and vocabularies are read under the grants that read claims; a user root
  may hold its own facets, and universe's curated facets are served without their authors.**
  A term is a claim: read under reader effective access on its facet's workarea, held terms only
  by their author. A link is served only to a key reading both endpoints (ADR-0046 d5), so no
  read of a vocabulary says which memories, or how many, use a term outside what the key may read:
  the vocabulary page's counts are "memories you can read". A **user root** (ADR-0044) may create
  facets under itself, its user's keys writing terms and its user's other keys reviewing them
  with ADR-0046 d2's `same_user` mark. **`universe`** may hold curated facets as vocabulary
  workareas directly below it, written by curator keys and reviewed by a second identity
  (ADR-0044's rule for universe claims) and served under ADR-0044 d9: no author, no lineage. This
  amends ADR-0044 d3: a user key's issuance-time reader grant on the `universe` node reaches the
  vocabulary workareas that are its children, for claims of kind `term`, and nothing else below
  it. And it amends ADR-0046 d5 for one label: a user key's `oml:about` edge from a memory in its
  root to a universe term is admitted, because a term is a shared word, the edge confers no
  access, and it is served only to keys that read the memory; every other edge into `universe`
  stays refused. A universe term's label cannot reveal a user's memory: labels are curator-written
  and reviewed like any universe claim, and FR-73's open check would cover them as it covers curated text.

- **d10 — A crew starts from a seed or an import, each term still reviewed.** OML ships seed
  facets as data files, versioned with the release like the `oml:` upper vocabulary (ADR-0021 d2):
  one per workarea type it documents, starting with `software-project` (component, technology,
  topic, process, at most 80 terms). `oml vocab seed <seed> --under <workarea>` creates the facets
  and writes every term held for a curator, previewing the count first (ADR-0021 d2's
  confirmation rule). `oml vocab import <file>` reads SKOS in JSON-LD or Turtle client-side —
  `skos:ConceptScheme` → facet, `skos:Concept` → term, `prefLabel`, `altLabel` and `hiddenLabel` →
  labels, `definition` → definition, `broader` → an `oml:is_a` edge held for the curator — and
  writes held terms through the API under the importing key. It runs no server-side import, so
  ADR-0021 d10's decline of auto-import and RS-11's server-side profile stand; this amends d10 by
  the client-side path. A crew with no terms loses nothing: the `term` retriever returns an empty
  list and fusion serves the keyword order unchanged.

- **d11 — The constitution holds, and the term list is ranking by reviewed content, not by a
  verdict.**

    | | R1, the verb ceiling | R2, never the author | R3, visibility not ranking | R4, no silent deletion |
    |---|---|---|---|---|
    | d3 `about` | an edge proposed with a write; the agent promotes nothing | the memory's non-author reviewer ratifies | no verdict adds or removes a term | a rejected or retired link is kept |
    | d5 new term | a held claim; the agent promotes nothing | the facet's curators, never the author | the term is invisible until promoted | a rejected term stays in the journal |
    | d6 `term` retriever | a read | — | ranks only candidates the key may read, by edges the author wrote | — |
    | d7 filter | a read | — | a predicate before ranking | — |
    | d9 visibility | — | `same_user` marked | an edge served only to a reader of both ends | — |
    | d10 seed, import | held writes under the importing key | every term reviewed | nothing visible before review | — |

    R3 says the gate changes visibility, not ranking, and that review never reorders results. The
    term list reads which ratified `oml:about` edges a memory version has and which labels the
    query matches. Ratification decides whether an edge is visible, as promotion decides whether
    a memory is; once visible, an edge ranks by what it says — this memory is about this term —
    which its author wrote and its reviewer could only accept or reject whole with the memory. No
    ranking input reads a verdict, a reviewer, a review count, a `held_by` attestation or
    `promoted_at`; a term's weight does not depend on who curated it; and a deprecation retires
    links by the term's status, which is the term's reviewed content, not by an act on the link.
    This is ADR-0050 d5's argument for keywords, and it holds for terms because no verdict can
    author one. The development exception (ADR-0001 d5) applies to terms and links as to any claim
    in a development workarea.

- **d12 — Every new failure has a doctor line.** `oml doctor` (admin) names: a facet with no
  dreamer able to review its held terms (ADR-0021 d11's orphan check, per facet); held terms
  waiting longer than seven days, with their count (seven is chosen, not measured; FR-96); a label that
  appears in more than 5% of the readable memories' texts of the workareas using the facet, the
  broad-label risk the eval found, named with its term (a function of memory texts, never of
  queries, which no log keeps); two terms of one facet whose labels collide after a release that
  changed folding; promoted memories in a workarea with a vocabulary policy whose share with no
  ratified link over the last 30 days exceeds 50%, the adoption signal (FR-97); and links
  retired by a deprecation whose memory is still a served tip, the self-heal backlog. Each line
  counts and names; none acts.

- **d13 — The build, in increments, each closed by named tests and its gate.**
    1. **O1, terms with labels.** The ADR-0021 FR-19 slice this needs and no more: prefixes,
       term claims with d2's fields, deprecation with its successor edge, `vocab.label_conflict`;
       `oml vocab create|list|show|seed|import`; `oml_read {terms_for}` and the workarea read's
       `vocabularies`. Releases, mappings and the closure stay unbuilt. Tests:
       `a_label_names_one_active_term_in_its_facet`, `terms_for_matches_stemmed_label_runs_and_never_attaches`,
       `a_skos_concept_imports_as_a_held_term_with_its_labels`. Gate: the seed's import round-trips
       the eval vocabulary byte for byte.
    2. **O2, `about` at write.** After ADR-0046's claim edges are served and with ADR-0050's
       `oml_write` fields: `about`, new-term proposals, `candidate_terms`, `proposed_terms`,
       carried terms on supersession; the reviewer's serve shows the terms (d14). Tests:
       `about_edges_ratify_with_the_memory_and_never_by_its_author`,
       `no_verdict_adds_or_removes_a_term`, `a_link_to_a_held_term_matches_nothing_until_the_term_promotes`,
       `a_user_key_links_a_universe_term_and_no_other_universe_edge`. Gate: the R1–R4 table's
       tests pass in `oml-port-contracts` against both stores.
    3. **O3, recall by term.** The `term` retriever, `bm25-term` and `bm25-trigram-term` enabled
       and not default, `about` as a filter, `matched.terms`. Tests:
       `the_term_retriever_reads_only_the_candidate_scope`, `a_retired_link_stops_matching`,
       `recall_never_rewrites_the_query_with_labels`. Gate: the eval job runs
       `eval/recall/ontology/` and the served pipeline reproduces the offline `bm25+term` within
       0.01 nDCG@10; becoming a default waits for FR-95.
    4. **O4, the doctor lines** of d12, each with a test that plants its condition.

    Each increment ships its doctor lines with it; O1 and O3 need no ADR-0046 edge and may build
    before it, O3 then serving no hit through a term until O2 exists.

- **d14 — The reviewer sees the terms as part of what it judges.** The serve of a held memory
  (ADR-0012 d5, as ADR-0050 d11 extends it) lists its proposed terms with each term's facet,
  preferred label, definition and state (active, held, deprecated with its successor), and marks
  carried terms. The reviewer approves or rejects the whole; a reviewer who disagrees with a term
  rejects with a reason that names it, and the author revises (ADR-0050 d3). The cost is that one
  wrong term costs a whole round trip; the purchase is that a verdict never authors a term.

## Consequences

- **What it costs, measured and not.** On the eval, curated tags gained +0.032 nDCG@10 and noisy
  ones lost −0.106, so the value rests on review catching wrong terms; how often reviewers do is
  FR-97. Curation (writing and reviewing terms) and the review of links add work no number
  states; FR-96 counts it on a real crew. A crew with no vocabulary gains nothing until it has
  one; d10's seeds shorten that and still need every term reviewed. `oml_write` gains one field
  and one answer field, `oml_read` one mode, the workarea read one field; no tool is added. The
  term retriever adds a label match and an edge lookup to each recall in its pipelines, its cost
  unmeasured (FR-98).
- **Roadmap rows.** FR-95 opens (the term pipeline on eval v2 with an independent vocabulary and
  independent queries, including unanswerable ones), FR-96 (curation and link-review load on a
  real crew), FR-97 (the share of links reviewers reject or authors revise, the adoption share,
  and the cap of three), FR-98 (the term retriever's latency and the label matcher's cost at
  stated vocabulary and corpus sizes) and FR-99 (false matches through broad labels on real
  queries, which needs consented query text, ADR-0048 d12). FR-19 narrows to the O1 slice for
  this record; FR-20 keeps expansion over `is_a` and mappings.
- **Schema and events, when built.** Term bodies gain three fields inside the envelope (erased
  with the term); `claim_edges` holds `oml:about` edges with `term_curie` (ADR-0027 d6) and a
  `ratified_with` artifact id for edges ratified by a claim's verdict; the term retriever needs
  an index on (`term_curie`, `from_artifact_id`) for ratified edges. No new event family:
  `claim_edge.proposed` and the verdict's `review.*` events carry the edges; `vocab.label_conflict`
  joins the `vocab` family on the three contract surfaces in O1's change.
- **Erasure.** A term's labels and definition are body content, crypto-shredded with the term
  (ADR-0005), while its CURIE and lineage row are permanent (ADR-0021 d2); an `oml:about` edge
  holds CURIEs and artifact ids only and survives a memory's erasure as edges do.
- **At acceptance** ADR-0021, ADR-0044 and, once each is accepted, ADR-0027, ADR-0046, ADR-0049
  and ADR-0050 gain `· amended by ADR-0051` on their Status lines; ADR-0027's status line says
  which of its decisions this record unparked.

## Open questions for the maintainer

| # | Question | Options | Recommendation |
|---|---|---|---|
| 1 | Who may propose a term from a write? | a. any writer with writer on the facet (ADR-0021 d8); b. only ADR-0027's term creators | a: an agent meets the missing concept while writing; the curator still decides |
| 2 | Does a user root's memory link universe terms? | a. yes, `oml:about` only (d9); b. no, a user root keeps its own facets | a: shared words are what universe is for, and the edge confers nothing |
| 3 | Is the cap of three right? | a. 3; b. 5; c. none | a until FR-97 measures it: the eval used 3, and more terms dilute the term list |
| 4 | Should `about` replace ADR-0021 d8's signed `terms` field? | a. yes, for memories (d3); b. keep both | a: two ways to say one thing must agree, and the edge retires with its term |
| 5 | Should a reviewer be able to drop one term and approve? | a. no, reject and revise (d14); b. yes | a: a verdict that removes a term changes what the memory is found by (R3) |
| 6 | Which seed ships first? | a. `software-project`; b. none, import only | a: OML's own users are software crews; the eval vocabulary is its first draft |
| 7 | Which release takes O1–O3? | a. after ADR-0046's edges and ADR-0049's port; b. O1 and O3 earlier | a: O3 serves nothing until O2 links exist |

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **A server-side model that extracts entities or keywords at write time** | The provider runs no model at admission ([ADR-0021](ADR-0021-ai-human-ontologies.md) rejected an annotation service), extraction costs a model call per write (Mem0 10,804 s of construction on one MemoryAgentBench setting) and loses facts (65.8% of answer entities in GraphRAG's graph), and it would put the server in the author's seat for metadata the reviewer then reviews. On the eval, terms attached with no judgement gained nothing measurable (+0.012, −0.017 to +0.040). The writing agent is a capable model that has just read the text: it chooses, for free. |
| **Free keywords per category (a few keyword sets, one per facet)** | Two writers still spell one concept two ways, so a recall for "credentials" still misses "secrets"; nothing reviews a category's words as a set, and there is no stable id to filter, deprecate or count by. ADR-0050 d5's keywords already cover a writer's own extra words; a facet of free keywords would be a second, unreviewed vocabulary beside them, which ADR-0050 rejected as "free-form tags". |
| **Embeddings only** | Dense matched the term list on this set (nDCG@10 0.85 against 0.88; paraphrase 0.82 both) at the cost of an indexer, a vector index, held embeddings and erasure rows (ADR-0048 S7), and it says nothing a person can filter by, curate or read as a reason. They are not exclusive: S7 stays gated on its own measurement, and a term list fuses with it like any list. |
| **A global, uncurated tag cloud** | Tags anyone adds and no one reviews drift into synonyms and noise; on the eval noisy tags cost −0.106 nDCG@10 with 55 queries worse. A tenant-wide cloud would also leak, through counts and suggestions, what other users' memories are about. |
| **Expanding the query text with every label of a matched term** | Measured: 44 queries worse and 26 better over BM25, success@1 0.79 → 0.72; labels are many and some are common words that BM25 then treats as evidence. It helped over the any-word index (+0.09 success@5), which no pipeline keeps as a default under ADR-0049 d9. The term list keeps the query's words as they are. |
| **An additive boost per matched term** | Measured over the expanded query: −0.004 nDCG@10 against BM25 with curated tags; a constant added to scores from lists that do not compare. Reciprocal rank fusion is ADR-0049 d3's one fusion. |
| **Counting only rare terms** | Measured: nDCG@10 0.86 against 0.88 when terms tagging more than 10 memories were ignored; broad terms carry true matches too. d12 names broad labels for a curator to fix instead. |
| **An `oml_vocab` tool** (ADR-0021 d9) | ADR-0039 d8 allows no fourth tool; `oml_read {terms_for}` and the write answer's `candidate_terms` carry what an agent needs at the moment it writes. |
| **Classifiers and curators first** (ADR-0027 in full) | Crew kinds, member profiles and narrowed grants serve classifying existing memories at volume; a write that names its own terms needs none of them. They stay parked until FR-97 shows authors' terms are not enough. |
| **A reviewer who edits terms while approving** | A verdict would then author what a memory is found by, which R3 forbids; ADR-0050 d5 refused the same for keywords. |
| **One facet as a field on the term rather than a vocabulary** | A facet field would be a second grouping beside vocabularies, with its own grants and review rules to define; a vocabulary already has prefix, grants, policy and deprecation. |
