<!--
id: adr/ADR-0027-ontology-crews
tier: engineering
status: proposed
answers: what an ontology crew is and how a crew records its kind; which member profiles an ontology crew's keys carry over the existing roles, and whether issuance enforces them; the edge_labels grant attribute, where it lives, how it composes with effective access and what it refuses; what a classification is, how it is proposed, ratified, rejected and retired, and whether it is derivation; whether oml:about joins the oml: upper vocabulary and how many terms one memory may carry; how recall's terms filter matches classifications; how a curator's independence is held; what each member profile reaches on MCP and HTTP; the CLI verbs that create an ontology crew and issue its members' keys
-->

# ADR-0027 — An ontology crew classifies memories through curator-ratified `oml:about` edges, and its members' keys carry member profiles over the existing roles

**Status:** proposed · 2026-10-30

**Amends:** ADR-0008 (d4), ADR-0010 (d5), ADR-0012 (d3, d4), ADR-0018 (d4), ADR-0021 (d7, d9, d11), ADR-0025 (d1, d2), ADR-0026 (d5, d8)

## Context

**A vocabulary has curators but no crew shape.** [ADR-0021](ADR-0021-ai-human-ontologies.md)
hosts vocabularies as claims: a vocabulary is a workarea with a prefix, a term is a claim of kind
`term` whose `claim_key` is its CURIE, hierarchy is ratified claim edges typed from `oml:`, and
d11 makes a vocabulary workarea name "a curator crew". That crew is a crew record like any other
([ADR-0026](ADR-0026-names-are-keys.md) d8), so nothing says what its members hold, and the grants
its keys carry are whatever an admin happened to issue
([ontologies](../architecture/ontologies.md#a-vocabulary-is-a-workarea-with-a-prefix)).

**Only the author can attach a term to a memory.** A claim cites terms through its own `terms`
field, which enters `signed_bytes` (ADR-0021 d8). A memory written without a citation, or before
the term existed, is invisible to recall's `terms` filter for as long as it stays the tip, and no
other identity can change that without rewriting another author's bytes, which R2 and the
supersession rules forbid ([audit and lineage](../architecture/audit-and-lineage.md#supersession-chains-claim_key-and-the-tip)).

**A dreamer grant is all or nothing.** A dreamer grant confers propose and review together
([roles and keys](../architecture/roles-and-keys.md#the-dreamer-split)): a key that should only
propose one kind of edge also receives verdicts on every held item of the workarea, edit-as-proposal
and workarea-edge ratification. The claim-edge label vocabulary is open
([HTTP API](../api/http-api.md#claim-edges)), and a claim edge has no decided-against state.

**The maintainer's decisions.** The maintainer fixed these, and this record designs around them
and does not reopen them: a new crew kind, the ontology crew, beside today's crews, whose only
business is maintaining and enriching vocabularies; its members take profiles over the existing
five roles, with no new role — a **classifier** (dreamer, propose class, reader grants on content
workareas, and a journalled grant attribute that limits its proposals to `oml:about` claim edges),
a **term creator** (a writer granted only on vocabulary workareas) and a **curator** (dreamer,
review class, on the vocabulary and content workareas, always a key separate from the proposers,
with ADR-0021 d7's human-gate defaults still applying); a classification is a reviewed edge,
memory —`oml:about`→ term, that a classifier proposes and a curator ratifies, never touching the
memory's signed bytes; recall's `terms` filter and `expand_terms` match both the author's own
citations and ratified `oml:about` edges; and "mapping" stays reserved for term↔term
`term_mapping`, memory→term work being "classification".

**What this record holds itself to.** R1–R4 ([ADR-0001](ADR-0001-the-constitution.md)) without
exception; ADR-0010's five roles unchanged; ADR-0026's open crew shape extended by optional fields,
renaming nothing; the [refusal order](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order)
and its fail-closed absence untouched for every key that carries no new attribute; and
ADR-0021's provider floor — a tenant with no vocabulary pays nothing new. The PoC holds none of
this: it refuses a workarea `prefix` (crates/transports/oml-transport-http/src/lib.rs:619-623),
stores a workarea's crew as a JSON object (migrations/0004_grants.sql:54) and has no claim-edge
table. Line citations refer to the tree at commit `1bbdaee`, the base this record was written
against. Decided, not built.

## Decision

- **d1 — A crew record carries a kind, `general` or `ontology`, given at creation and never
  changed.** `POST /v2/crews` takes `{name, kind?, charter_uri?, external_manager?}`, and
  `crew.created` journals `kind`; an absent kind is `general`. No update carries the kind, as none
  carries the name. Every crew record that exists before this record — every `crew.created`
  without the field, and every crew `system.names_assigned` makes from workarea crew objects
  (ADR-0026 d11) — reads `general`. A `general` crew is today's crew record exactly, writer crews
  and reviewer (dreamer) crews alike, and nothing below changes for it. An `ontology` crew is a
  crew whose business is its vocabularies: a vocabulary workarea names one (d2), and every key
  that names one carries a member profile (d3, d4). The record is declared on
  [workareas](../architecture/workareas.md#crews-recorded-not-operated).

    This is the extension point ADR-0026's Consequences left open — a new optional field on the
    record, its creation body, `crew.created` and the `crews` table — and it amends ADR-0026 d8
    only by adding it. The kind never changes because member profiles depend on it: a crew whose
    kind moved would leave keys whose profiles describe a crew that no longer admits them. The cost
    is that a crew that changes its business is a new crew record, and its members hold new keys.

- **d2 — A vocabulary workarea names an ontology crew.** A workarea create carrying `prefix`
  already requires `crew` (ADR-0021 d11); the crew it names must be of kind `ontology`, and a
  crew-metadata update of a vocabulary workarea (`POST /v2/workareas/{id}/crew`) must name one
  too, else `request.validation` with `details.field: "crew"`. Below, a **vocabulary workarea** is
  a workarea that registered a prefix, and a **vocabulary node** is any workarea in a vocabulary
  workarea's subtree.

    ADR-0021 d11's "named curator crew" thereby means one thing: the ontology crew whose curators
    review the vocabulary. Grants stay ordinary (ADR-0021 d8): any dreamer key an admin grants on
    the vocabulary still reviews there, whatever its crew, so the ontology crew is the vocabulary's
    designated crew, not its only possible reviewer. The cost is one crew record for a tenant whose
    existing reviewer crew curates a vocabulary. This amends ADR-0021 d11.

- **d3 — A key that names an ontology crew carries one member profile — `classifier`,
  `term_creator` or `curator` — a fixed grant shape over one of the five roles.** `key.issued`
  journals `profile`; one profile per key, fixed for the key's life, carried to a rotation's
  successor with the grants rotation reproduces. A profile is not a role: the key's `role` stays
  one of ADR-0010's five, and the profile names which grants a key of that role may carry while it
  is a member of an ontology crew. The shapes, declared once on
  [roles and keys](../architecture/roles-and-keys.md#member-profiles):

    | Profile | Key role | Must carry | May also carry | Never carries |
    |---|---|---|---|---|
    | `classifier` | `dreamer` | a dreamer grant on a vocabulary node with `edge_labels: ["oml:about"]` (d5) | reader grants on any node | a dreamer grant without `edge_labels`, or any dreamer grant off a vocabulary node |
    | `term_creator` | `writer` | a writer grant on a vocabulary node | reader grants on vocabulary nodes | any grant off a vocabulary node |
    | `curator` | `dreamer` | a dreamer grant without `edge_labels` on a vocabulary node | reader grants on any node; dreamer grants with `edge_labels: ["oml:about"]` off vocabulary nodes | a dreamer grant without `edge_labels` off a vocabulary node, or one with `edge_labels` on a vocabulary node |

    Every profile's key is attested `held_by: agent` or `held_by: human`, never `service`. Deny
    masks inside grants and global per-key denials are admitted on every profile: they only narrow.

    What each profile does follows from its shape. A **classifier** reads content and vocabularies
    and proposes classifications (d6). A **term creator** proposes terms, mappings and releases —
    it is the crew's one writer, and only a writer submits a new claim — and supersedes the term
    claims whose tip its key authored, edits and deprecations included; a term whose tip another
    key authored is revised by a curator's edit-as-proposal, under the
    [supersede rule](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order). A
    **curator** records verdicts on held vocabulary claims — a deprecation's successor edge
    included, since it rides the deprecation's serve (ADR-0021 d3) — proposes and ratifies
    hierarchy edges, ratifies and rejects classifications, and proposes edits; it is the crew's one
    proposer of hierarchy, since a claim edge needs dreamer(propose) and the classifier's is
    narrowed. R2 keeps every one of its own proposals for another identity to decide.

    The classifier's propose-only class is structural, not a promise. Its only dreamer grants are
    narrowed (d5), so it receives no serve and records no verdict on a held term, proposes no edit
    and ratifies no term edge; and they sit on vocabulary nodes, while a classification's source
    lies outside every vocabulary subtree (d6), so it never holds dreamer(review) on a
    classification's source and can ratify or reject none. The curator is always a key separate
    from the proposers because no key carries two profiles, and R2 refuses it its own proposals
    in any case. This amends ADR-0010 d5 (a dreamer key's grants may confer one class without the
    other) and ADR-0026 d8 (a key's crew of kind `ontology` brings a profile with it).

- **d4 — Issuance enforces the profile: any other shape is refused with `access.profile_mismatch`,
  at issuance and at every grant amendment.** The [issuance order](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order)
  gains a step 7, after the default-workarea check and before `name.taken`, which becomes step 8
  and stays after every other check (ADR-0026 d5): when the key names a crew of kind `ontology`, or
  the body carries `profile`, the crew must be of kind `ontology`, a profile must be sent, the key
  role must be the profile's, `held_by` must be `agent` or `human`, and the grants must match the
  profile's row in d3 — every "must" present, every grant within "must" or "may", none within
  "never". The first failure answers `access.profile_mismatch` (403) with `details` `{profile,
  clause, grant_index}`: `clause` one of `crew_kind`, `profile_required`, `key_role`, `held_by`,
  `grant` or `required_grant`, and `grant_index` the first offending grant's 0-based index, or
  null. A `profile` without `crew` is `request.validation`, checked with the rest of the body
  before step 0.

    A grant added to a profiled key (`POST /v2/keys/{key_id}/grants`) is checked the same way, the
    key's grants and the new one together, and refused with clause `grant`, journalling nothing.
    Revoking a grant, setting a denial and revoking the key are never refused by a profile: each
    only removes authority, and a profiled key left without its required grant fails closed by
    doing less. A key that names a `general` crew, or no crew, sends no profile and is checked
    exactly as before.

    The code is new and lands on the three contract surfaces in one change
    ([errors](../api/errors.md), the OpenAPI `ErrorCode` enum, [HTTP API](../api/http-api.md)). It
    is an `access` code for the reason `access.role_exclusive` is: it refuses an authority shape on
    a key, at issuance and at amendment, where no body field is malformed. A profile issuance does
    not enforce would be a label that the grants history could contradict; enforced, "this key is a
    classifier" is a fact the journal proves. The cost is one step in the issuance order, and an
    admin who wants a broader key for an ontology crew's agent issues it under a `general` crew,
    which the export shows. This amends ADR-0025 d2 (the body gains `profile` and `edge_labels`,
    and the order gains step 7) and ADR-0026 d5, whose `name.taken` check on key issuance moves
    from after step 6 to after step 7 and so stays the last check.

- **d5 — A dreamer grant may carry `edge_labels`, which narrows the grant to claim edges carrying
  a listed label; it attenuates its own grant before the union, and a narrowed key meets
  `access.grant_refused` for everything else.** `edge_labels` is a non-empty list of distinct
  labels from the `oml:` upper vocabulary (d7), admitted on a dreamer grant only — on any other
  grant, or with a label outside `oml:`, `request.validation`. It is journalled in
  `grant.issued`, stored on the grant row, reproduced by rotation and served on every key read.

    - **Where it lives: on the grant.** The curator needs an unnarrowed dreamer grant on its
      vocabularies and a narrowed one on content workareas, on one key; a key attribute could say
      only one of the two.
    - **What it confers.** Over its subtree, a narrowed grant confers proposing, ratifying and
      rejecting (d8) claim edges whose label it lists, and nothing else of the dreamer row: no
      serve and no verdict on a held item, no reopen, no edit-as-proposal, no workarea-edge
      proposal or ratification, no status-transition act and no dead-letter re-enqueue.
    - **How it composes.** `effective_roles(N)` keeps its definition: take the grants on N or an
      ancestor, drop each whose own deny list covers N, union, and answer the empty set under a
      global denial covering N. Of the dreamer grants that survive at N, dreamer is **unnarrowed**
      if any carries no `edge_labels`, and otherwise **narrowed** to the union of their lists. A
      narrowed grant never narrows another grant, so composing grants still never makes one mean
      less than it says ([workareas](../architecture/workareas.md#effective-access-union-of-attenuated-grants-then-deny-wins)),
      and no denial and no narrowing widens anything.
    - **What it refuses.** Step 2 of the refusal order allows a dreamer-class verb on N where
      dreamer is effective and, if narrowed, the verb is a proposal, ratification or rejection of a
      claim edge with a listed label. Anything else falls to step 3, `access.grant_refused`, whose
      `details` gain `edge_labels` — the narrowed set at N — beside `{workarea_id, needed_role}`,
      or to step 4, `access.not_found`, where reader is not effective. A classifier proposing
      `oml:is_a`, calling review next on its vocabulary or proposing an edit therefore answers
      `access.grant_refused` naming `["oml:about"]`, the labels it holds. The code exists and
      already says what this is — effective access does not confer the verb here — so no code is
      added for it.
    - **What it reports.** A narrowed dreamer contributes `propose` and `review` to
      `my_effective_verbs`, as an unnarrowed one does, and the workarea read gains
      `my_edge_labels`: the narrowed set where dreamer is narrowed, null where it is unnarrowed or
      absent ([effective verbs](../architecture/roles-and-keys.md#effective-verbs)). A narrowed
      grant is not review capacity for held items: review availability and the orphaned-vocabulary
      flag count unnarrowed dreamer grants only.

    The attribute is a second attenuation inside a grant, beside its deny masks: a deny cuts a
    subtree out of a grant, `edge_labels` cuts verbs out of it. That keeps ADR-0008's one access
    computation and ADR-0025 d1's pure function, at the cost of one more input to both. It is not
    the service-key narrowing [FR-32](../product/roadmap.md) asks for, which this record leaves
    open. This amends ADR-0008 d4 and ADR-0025 d1.

- **d6 — A classification is a claim edge labelled `oml:about` from a promoted memory version to a
  term, proposed under dreamer(propose) on one endpoint and reader on both, ratified under
  dreamer(review) and reader on both; it never touches the memory.** The rules, declared on
  [ontologies](../architecture/ontologies.md#classification-ontology-crews-attach-terms-to-memories):

    - **Shape.** `from_artifact_id` names a promoted version of a claim of kind `memory` or
      `derived_fact` in a workarea that is not a vocabulary node; `to_artifact_id` names a promoted
      version of a claim of kind `term`. Anything else is `request.validation`, `details.field`
      naming the endpoint. The edge records the term's CURIE as `term_curie`. The term's served tip
      must be active, at proposal and at ratification, else `vocab.inactive_curie` with its
      `successors` and `relation` — the code a citation of a deprecated term meets, with the same
      one-hop recovery.
    - **Binding.** The edge binds the memory version it names, as every claim edge binds versions:
      a successor version does not inherit it, because the curator judged the bytes it read, not
      the bytes a later version holds; the successor is classified anew, by its author's `terms` or
      by a new classification. On the term side the edge binds the CURIE, so an edit of the term
      leaves it in force.
    - **The memory is untouched.** No version is written, `signed_bytes`, `terms` and the tip
      stay as they were, and the memory's author is neither asked nor notified: the claim stays its
      author's claim.
    - **Proposal.** dreamer(propose) effective for `oml:about` on at least one endpoint's workarea,
      and reader on both — the workarea-edge authority rule
      ([workareas](../architecture/workareas.md#edge-authority)) extended to classifications, as
      ADR-0021 d6 extends it to mappings. An endpoint the key cannot read is `access.not_found`.
    - **Ratification.** One act, `POST /v2/claim-edges/{edge_id}/ratify`, by a key with
      dreamer(review) effective for `oml:about`, and reader, on both endpoints' workareas; refused for
      the proposer's key or principal (`review.ratifier_is_proposer`) and for any key or principal
      in either endpoint's lineage closure (`review.author_in_lineage`). The policy it runs under is
      d12's.
    - **Status.** A classification is admitted whatever the status of either endpoint's workarea:
      `validated` freezes claim content and promotions, and a classification changes neither — the
      reasoning that admits the crew-metadata update on a frozen workarea.
    - **Visibility.** An `oml:about` edge, in any state, is served by claim-edge reads and
      `oml_graph` only to a key that can read both endpoints, so a classification of a simulation
      row is fenced with its source.

    A classification needs two endpoints, a proposer and a ratifier, which is what a claim edge
    already carries; this adds a label and its rules, not a new kind of record. Binding the
    version costs re-classification after every supersession of a classified memory; the purchase
    is that no ratified classification ever describes bytes its curator did not read.

- **d7 — `oml:about` joins the provider-published `oml:` upper vocabulary; one memory version
  carries any number of distinct classified terms, and a duplicate is refused with
  `vocab.classification_exists`.** `oml:about` is shipped product data versioned with the release,
  like `is_a` and `replaced_by` (ADR-0021 d2), and enters [RS-10](../product/roadmap.md)'s register
  with its definition: the one `oml:` relation whose source is not a term. Because `oml:` is
  reserved in every tenant, no tenant prefix can spell a colliding label.

    No cap on the number of classifications per memory version is stated; the bound on a flood is
    the one ADR-0021 d11 already names — journalled rate and quota grant attributes — and the
    curators' throughput, unmeasured under [FR-2](../product/roadmap.md). A memory version has at
    most one live classification per CURIE: a proposal naming a version and a CURIE that a pending
    or ratified `oml:about` edge already joins, or a CURIE that version's own `terms` already
    cites, is refused with `vocab.classification_exists` (409), `details` `{source: "edge",
    edge_id, state}` or `{source: "terms"}`. A unique index is the authority, so a proposal that
    loses a race answers the same code and journals nothing. A rejected pair may be proposed again
    (d8). The code is new and lands on the three contract surfaces in one change; it is a 409, not
    `request.validation`, for ADR-0026 d5's reason — it reports a state conflict that retrying the
    same request can never resolve, as `vocab.prefix_conflict` does.

- **d8 — A curator rejects a classification, pending or ratified, by one journalled act; the edge
  is kept, and the pair may be proposed again.** `POST /v2/claim-edges/{edge_id}/reject
  {reason}` — `reason` required, journalled as a digest — journals `claim_edge.rejected` with the
  edge's prior state. It needs the authority ratification needs (d6) and meets its refusals, except
  d12's crew and manager requirements, because a rejection makes nothing visible; a `held_by:
  service` key is refused it, as it is every review-class verb (ADR-0025 d1). The edge row
  stays, with state `rejected`; its proposal, any ratification and the rejection stay in the
  journal; it confers nothing, matches no recall, and leaves the duplicate test of d7. A rejection
  of an edge with any other label is `request.validation`: other claim edges keep
  propose-then-ratify, and this record decides a decided-against state only for the label whose
  volume needs one.

    Without it a wrong proposal would wait forever in the curators' worklist and block its own
    correction, and a wrong ratified classification could never be withdrawn; nothing would be
    deleted either way, so R4 holds.

- **d9 — A classification is not derivation: `oml:about` edges stay out of the lineage closure and
  out of taint.** The [lineage closure](../architecture/audit-and-lineage.md#the-derivation-graph)
  is the closure over `derives_from` and claim edges other than `oml:about`. Classifying a memory
  therefore never removes the term's authors from the memory's future reviewers, never removes the
  memory's author from the term's future reviewers, and never makes a term sim-derived
  ([simulations](../architecture/simulations.md)). R2 on the classification itself is unchanged:
  its ratifier and rejecter are checked against both endpoints' closures (d6).

    ADR-0001 d2 already speaks of the *derivation* lineage closure, and a classification says what
    a memory is about, not what it was built from; this record names the one label that is not
    derivation and amends ADR-0010 d5's "pending and ratified edges alike" by that exclusion only.
    Counted as derivation, a term with many classified memories would exclude every one of their
    authors from reviewing its next edit, and a classified simulation row would taint the term. An
    excluded edge changes no authorship and promotes nothing, so it opens no laundering path.

- **d10 — A ratified classification is retired while its term's served tip is deprecated, by
  derivation, never by a write; the classifier self-heals it with a new, reviewed classification.**
  Retirement is read from the term's `term_status` at the moment of each read. The deprecation's
  ratification, already journalled with its successor edge (ADR-0021 d3), is the only act: nothing
  is written to the edge, the memory or the journal for any classification it retires, so the
  deprecation of a widely used term adds nothing to its commit
  ([ADR-0024](ADR-0024-performance-foundation.md) d1). A retired classification stays `ratified`
  in the journal and the fold, is served by claim-edge reads with `retired: true`, and stops
  matching recall's `terms` filter (d11); nothing is deleted.

    Self-heal rides the existing pointers. The classifier reads the successor from `oml_vocab get`
    or from the `vocab.inactive_curie` details of a refused proposal, and proposes a classification
    of the same memory version to the successor — mechanical for `replaced_by`, a judgment for
    `consider` — which a curator ratifies like any other. Recall never follows `replaced_by` on its
    own: the human verdict on a deprecation attests that the successor replaces the term, not that
    each memory classified under the term is about the successor.

- **d11 — Recall's `terms` filter matches a hit's own citations and its ratified, unretired
  classifications alike.** A hit matches when the CURIEs it carries — the `terms` its version
  cites, and the `term_curie` of every ratified, unretired classification of that version — meet
  the filter set, after `expand_terms: descendants` widens the filter set exactly as ADR-0021 d9
  decides. A citation matches by its literal CURIE whatever its term's status, because it is the
  author's signed bytes; a classification matches only while unretired, because it is the crew's
  maintained statement. A CURIE the key cannot read still matches nothing. Steering, `k`,
  `max_tokens`, the rank order and the read log's `query_digest` are unchanged: the filter narrows
  the candidates before retrieval and the logged ids record what matched. No claim item gains a
  field; classifications are read through the claim-edge reads and `oml_graph`. Declared on
  [retrieval](../architecture/retrieval.md#the-serving-tier). This amends ADR-0021 d9's filter,
  which matched citations only.

- **d12 — A classification ratifies under its vocabulary's review policy, with the classifier
  standing where an author stands; a curator may ratify its own crew's classification unless that
  policy requires crew inequality, and the evidence export shows every pair.**

    - **Which policy.** The review policy of the term's vocabulary workarea, snapshotted into
      `review.ratify` (`subject_kind: "claim_edge"`), which gains `policy_workarea_id`. One ratify
      act decides: the policy contributes its independence axis, not `k`, since a claim edge has
      no serve to count (ADR-0025 d1).
    - **Independence.** R2 always (d6). Where the vocabulary's policy requires crew or manager
      inequality ([admission](../architecture/admission.md#review-policy-objects)), a ratify act
      whose key's crew — or that crew's external manager — equals the proposer's, or where either
      is unestablished (ADR-0026 d8), is refused with `review.ratifier_is_proposer`, `details.axis`
      `crew` or `manager`, journalling nothing; the edge stays pending — failing closed by holding,
      as a verdict that cannot establish the axis leaves a claim held. A curator therefore may
      ratify a classification its own crew's classifier proposed under the shipped default, whose
      axis is the principal, and may not where the policy requires crew inequality. A tenant that
      wants that runs two ontology crews that curate each other's classifications; d3 admits a
      curator's grants on any vocabulary, not only on its own crew's.
    - **Humans.** A classification is AI-only. `human_required_classes` names claim kinds, scan
      finding classes and provenance, and a single ratify act has no verdict count for a human
      verdict to complete. A tenant that wants human-curated classifications issues the narrowed
      content-side grants only to curators attested `held_by: human`, a choice the grants history
      shows. ADR-0021 d7's defaults table gains the row "`oml:about` classification — AI-only;
      human curation by grant shape". Its HUMAN rows still bind the vocabulary's claims, so under
      the shipped defaults an ontology crew needs a curator attested `held_by: human`, which
      `oml doctor`'s existing warning names.
    - **Evidence.** The reciprocal-approval statistics already cover edge ratifications
      (ADR-0021 d7). They cover classification ratifications and rejections as proposer↔decider
      pairs, slice by edge label and by member profile, and the export adds, per ontology crew, the
      share of its members' classifications and vocabulary claims decided by keys of the same crew
      ([audit and lineage](../architecture/audit-and-lineage.md#the-evidence-export)). They are
      evidence, not a verdict: no threshold acts on them.

    The vocabulary's policy governs because a classification is vocabulary structure applied to
    content, and the content workarea's governance already speaks through the grant it gave the
    curator (d3). Reusing `review.ratifier_is_proposer` keeps one code for "proposal and decision
    are not far enough apart", which is the rule that code names. This amends ADR-0012 d3 (which
    workarea's policy governs a classification) and d4 (the proposer of an edge stands where the
    author of a claim stands, and a single ratify act that cannot establish the axis is refused),
    ADR-0021 d7 (the defaults row and the statistics' slices) and ADR-0026 d8 (the axis read for a
    classification).

- **d13 — Every member profile works through the existing MCP catalog and HTTP verbs; no profile
  gains a tool, and none gains promote, sign, demote, delete or an admin verb.** Declared on the
  [ontology crews](../api/ontology-crews.md#what-each-member-profile-calls) page:

    | Profile | MCP tools it uses | HTTP beyond MCP |
    |---|---|---|
    | `classifier` | `oml_vocab` (suggest, get), `oml_recall`, `oml_get`, `oml_graph`, `oml_events_tail`, `oml_workarea_info`, `oml_relate_propose` (`kind: "claim"`, `label: "oml:about"`) | `GET /v2/claim-edges` |
    | `term_creator` | `oml_vocab`, `oml_remember` (kinds `term`, `term_mapping`, `vocab_release`), `oml_get`, `oml_my_work`, `oml_workarea_info`, `oml_events_tail` | `POST /v1/memories/{artifact_id}/supersede` for its edits and deprecations, since `oml_supersede` refuses vocabulary kinds (ADR-0025) |
    | `curator` | `oml_vocab`, `oml_get`, `oml_graph`, `oml_recall`, `oml_events_tail`, `oml_workarea_info`, `oml_relate_propose` | `/v2/review/*` on held vocabulary claims; `POST /v2/claim-edges/{edge_id}/ratify` and `/reject`; `GET /v2/claim-edges`; supersede for edit-as-proposal |

    The catalog stays at twelve tools ([ADR-0025](ADR-0025-the-agent-only-memory-surface.md) d12)
    and no tool's input schema changes. A curator's verdicts, ratifications and rejections stay on
    `/v2` behind the dreamer(review) checks, as every reviewer crew's do
    ([reviewer crews](../api/reviewer-crews.md)): the agent-safe subset still carries no path to a
    verdict (ADR-0014 d2, R1). The term read gains the served tip's `artifact_id`, so a classifier
    names the term it found by `oml_vocab get` without a second lookup.

- **d14 — `oml crews create --kind` and `oml crews add-member` compose existing calls, and
  add-member writes the key value only to its `--out` file.** Declared on the
  [CLI reference](../cli/reference.md#oml-crews-crew-records):

    - `oml crews create <name> [--kind general|ontology] [--charter-uri <uri>] [--external-manager
      <text>]` — one `POST /v2/crews`; `list` and `show` print the kind.
    - `oml crews add-member <crew> <key-name> --as classifier|term-creator|curator --vocabulary
      <workarea> [--vocabulary …] [--workarea <workarea> …] --out <path> [--held-by agent|human]
      [--write-only] [--principal <text>] [--expires <duration>] [--label <text>]` — one `POST
      /v2/keys` whose role, grants and default workarea follow the profile: a classifier gets a
      reader grant on each `--workarea` and, on each `--vocabulary`, a dreamer grant with
      `edge_labels: ["oml:about"]` and a reader grant, and its first `--workarea` as default; a
      term creator a writer and a reader grant on each `--vocabulary` (`--write-only` drops the
      readers, as ADR-0026 d13 does); a curator a dreamer and a reader grant on each `--vocabulary`
      and, on each `--workarea`, a dreamer grant with `edge_labels: ["oml:about"]` and a reader
      grant; both default to their first `--vocabulary`. `crew`, `profile`, `held_by` (default
      `agent`) and the principal (the key name unless `--principal` names another) ride the same
      body. `term-creator` is the CLI's spelling of `term_creator`.
    - **`--out` is required.** It is ADR-0026 d12's discipline: the file is created exclusively,
      mode 0600, before anything is sent; an existing path, or one containing a quote or a newline,
      is refused; a refused issuance removes the empty file; a value that cannot be written once
      the key is issued is lost, and the CLI prints the key's name and id on stderr and exits `2`.
      The verb prints only the two export lines that read the value from the file, and `--json` is
      refused, since the `201` body carries the value. No key value reaches stdout, stderr or a
      transcript.
    - Argument shapes the profile cannot take — a classifier without `--workarea` or
      `--vocabulary`, a term creator with `--workarea`, `--write-only` on another profile — exit
      `2` before anything is sent; every refusal from the server is the issuance's own.
      `oml grants add <key> <workarea>:dreamer --edge-label <label> …` attaches `edge_labels`;
      `oml keys issue` sends no profile, so add-member is the CLI's one path to a profiled key.
      `oml workareas create --prefix` requires `--crew` naming an ontology crew.

    One call keeps every refusal the issuance's, as `issue-agent` does; a mandatory `--out` means
    an operator issuing a whole crew never has a value in a terminal or a transcript, at the cost
    of one key file per member, deleted once the key is in place. The `oml-admin` skill is
    unchanged and runs `issue-agent` alone (ADR-0026 d14).

- **d15 — Three vocabulary rows.** This amends ADR-0018 d4 with:

    | Term | Means | Never |
    |---|---|---|
    | classification | a claim edge labelled `oml:about` from a memory version to a term, proposed by a classifier and ratified by a curator; it never changes the memory | "mapping" (reserved for term↔term `term_mapping`), "tagging", "annotation" for the edge (annotation is the author's own `terms` citation) |
    | classifier | the member profile that proposes classifications | "mapper", "tagger" |
    | member profile | the grant shape — `classifier`, `term_creator` or `curator` — that a key naming an ontology crew carries over one of the five roles | "role", "sub-role" (a profile is never a sixth role) |

    One word per concept keeps "mapping" meaning one thing across two vocabularies and the content
    they classify. "Sub-role" would read as an extra role, which ADR-0018 d4's roles row forbids.

## Consequences

**What callers see**

- An admin runs `oml crews create metals-terms --kind ontology`, creates the vocabulary with
  `oml workareas create universe metals-vocab --crew metals-terms --prefix metal`, and issues each
  member with `oml crews add-member metals-terms … --out …`; the member's agent starts from the two
  printed lines.
- A classifier finds a promoted memory, looks up `metal:aluminium` with `oml_vocab`, and proposes
  `oml_relate_propose {kind: "claim", label: "oml:about", from: <memory>, to: <term>}`; a curator
  ratifies or rejects it on `/v2`; from then on recall with `terms: ["metal:aluminium"]` returns the
  memory, whose bytes no one changed.
- A classifier that tries any other label, a review, or an edit gets `access.grant_refused` naming
  the labels it holds; a duplicate classification gets `vocab.classification_exists`; a key whose
  grants depart from its profile is never issued (`access.profile_mismatch`).
- When `metal:aluminium` is deprecated with `replaced_by`, its classifications stop matching recall
  at that ratification, are listed with `retired: true`, and the classifier re-proposes them to the
  successor for a curator to ratify.

**Surface and versioning**

- New error codes: `access.profile_mismatch` (403) and `vocab.classification_exists` (409),
  together on errors.md, the OpenAPI `ErrorCode` enum and http-api.md. Extended: the `details` of
  `access.grant_refused` (`edge_labels`) and `review.ratifier_is_proposer` (`axis`), and the
  conditions of `vocab.inactive_curie` and `request.validation`.
- New endpoint: `POST /v2/claim-edges/{edge_id}/reject`. New query filters on `GET
  /v2/claim-edges`: `label`, `state` and `term`. New fields: `kind` on the crew record and its
  creation body; `profile` on the issuance body and key reads; `edge_labels` on grant requests and
  grants; `term_curie` and `retired` on claim edges, whose `state` gains `rejected`;
  `my_edge_labels` on the workarea read; `artifact_id` on the term read.
- Events: `crew.created` gains `kind`, `key.issued` gains `profile`, `grant.issued` gains
  `edge_labels`, `claim_edge.proposed` gains `term_curie`, `review.ratify` gains
  `policy_workarea_id` for claim-edge subjects, and `claim_edge.rejected` is new; retirement is
  deliberately not an event ([event catalog](../data/event-catalog.md)).
- Schema: `crews.kind`, `api_keys.profile`, `grants.edge_labels`, and on `claim_edges` the
  `term_curie` and `rejecter_key_id` columns, the `rejected` state, the one-live-classification
  unique index and the recall lookup index ([PostgreSQL schema](../data/postgres-schema.md)).
- The MCP catalog and every tool's input schema are unchanged, so `mcp-tools.json` is untouched;
  `oml_relate_propose` and `oml_vocab` change only in prose-normative behaviour and output on
  [the agent plugin](../api/mcp-plugin.md).
- `info.version` stays `0.2.0-design`. The one `/v1` change is the meaning of recall's `terms`
  filter, which no build serves yet (ADR-0025 d16), and it lands in the same unreleased revision as
  ADR-0025 and ADR-0026, whose bump covers it.
- The lake is untouched: `gold.lineage` already materializes ratified claim edges with their label
  ([Delta tables](../data/delta-tables.md)), `oml:about` among them, and the fact that they are not
  derivation is the label's to say.

**Costs that are not measured**

- Recall's `terms` filter gains a lookup of ratified classifications and their terms' status;
  its cost is unmeasured and tracked against [FR-20](../product/roadmap.md) and
  [FR-25](../product/roadmap.md). No number is stated.
- Classification multiplies the curators' work by every memory a vocabulary can describe; their
  throughput is [FR-2](../product/roadmap.md)'s to measure. No rate is stated.

**What stays unbuilt** — everything above, with the FR-19 vocabulary slice it rides on: the PoC
has no vocabulary, no claim edge and no crew record, and the record is
[ADR-0026](ADR-0026-names-are-keys.md)'s, decided and not built. No roadmap row opens or closes;
RS-10's register gains `oml:about` when it is written.

**Process obligations**

- With the proposal: the record registers at its three points (this file, a
  [register](index.md) row and a `mkdocs.yml` nav entry); the D7, D9, D11, D12, D13 and D14 cells
  of the D-register name ADR-0027; `llms.txt` moves to ADR-0001–ADR-0027 and lists the
  [ontology crews](../api/ontology-crews.md) page; `CONTRIBUTING.md` names d15's rows; one
  [changelog](../changelog.md) entry records the change.
- On acceptance, and not before: this record's Status line and frontmatter `status` become
  `accepted design` with the acceptance date, its register row follows, the Status lines of
  ADR-0008, ADR-0010, ADR-0012, ADR-0018 and ADR-0021 gain "· amended by ADR-0027", and ADR-0025's
  and ADR-0026's gain it once each is itself accepted.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **d1 — Call today's crews `review`** | Today's crews include writer crews — the `redline` crew's agents write ([event catalog](../data/event-catalog.md)) — so the word would mislabel every working crew; `general` says "no profile" and nothing more. |
| **d1 — No kind; profiles without a crew kind** | Nothing would say which crews admit profiled keys, and a vocabulary's "curator crew" would stay a crew of any shape. |
| **d1 — A verb that changes a crew's kind** | Every profiled key of the crew would then describe a crew that no longer admits it. |
| **d2 — Let a vocabulary name any crew** | ADR-0021 d11's curator crew would keep meaning nothing, and the doctor could not tell a vocabulary's curators from any crew's. |
| **d3 — Classifier, term creator and curator as new roles** | ADR-0010 fixes five roles and ADR-0018 d4 forbids extra ones; every surface that enumerates roles would change, for shapes the existing roles already express. |
| **d3 — One key carrying several profiles** | A key that classifies and curates would ratify its fellow classifiers' work, and the journal could not show which capacity acted — the two-key rule's argument. |
| **d3 — A term creator as a dreamer** | A new term is a new claim, and only a writer submits one; a dreamer only edits what exists. |
| **d3 — A classifier as a writer that supersedes memories to add `terms`** | It rewrites another author's signed bytes, which the maintainer forbade, and a writer supersedes only its own claims (`access.author_only`). |
| **d3 — Term creators that read content workareas** | The maintainer granted the term creator only on vocabulary workareas; the classifier is the crew's reader of content. |
| **d4 — Profiles as unenforced labels** | The label would promise a shape the grants history could contradict, and the statistics would slice by a word, not a fact. |
| **d4 — `request.validation` for a profile mismatch** | It refuses an authority shape at issuance and at amendment, where no field is malformed — `access.role_exclusive`'s case, not a contract violation. |
| **d4 — Refuse a grant revocation that leaves a profiled key short of its required grant** | Removing authority must never be refusable; a key that holds less fails closed. |
| **d5 — `edge_labels` on the key** | The curator needs an unnarrowed dreamer grant on vocabularies and a narrowed one on content; one key attribute cannot say both. |
| **d5 — A "propose only" flag instead of labels** | A classifier could then propose every claim-edge label, `oml:is_a` among them — the term structure curators guard — and the curator's content-side shape could not be expressed. |
| **d5 — A narrowed grant narrows every grant on its node** | A grant would mean less because another grant exists, against ADR-0008 d4; and an admin could cut a key's authority by adding a grant. |
| **d5 — A new code for a refused label (`access.label_refused`)** | `access.grant_refused` already says that effective access does not confer the verb here; its details carry the labels. |
| **d6 — A classification as a claim of a new kind** | A second artifact per classification, held, served and reviewed with a serve, to carry what an edge already carries: two endpoints, a proposer and a ratifier. |
| **d6 — Bind the claim (`claim_key`) rather than the version** | A curator would vouch for every later version's bytes, which it never read. |
| **d6 — Classify memories inside vocabulary workareas** | A classifier's dreamer grant on the vocabulary would then reach a classification's source, and it could ratify its fellow classifiers' work. |
| **d6 — Accept a held or deprecated term as target** | A held term is unreadable to the classifier, so accepting it would leak its existence; a deprecated one is already retired (d10). |
| **d7 — A cap on terms per memory** | No measurement could set it, and rate and quota grant attributes already bound a flood. |
| **d7 — Admit duplicates, or answer an existing edge with `200`** | Duplicates double the curators' work for no recall difference; a `200` for a proposal that created nothing would be a refusal downgraded to a non-error. |
| **d7 — `about` as a tenant-registered label** | A tenant prefix could collide with it, and one tenant's classifications would mean something else in the next; `oml:` is reserved everywhere. |
| **d8 — No rejection; a classification stays pending until ratified** | Wrong proposals would fill the worklist forever and block their own correction, and a wrong ratified classification could never be withdrawn. |
| **d8 — Rejection for every claim-edge label** | It would decide what a rejected derivation edge does to the lineage closure, which this record does not need and has not weighed. |
| **d9 — Keep `oml:about` in the lineage closure** | A widely used term would exclude every classified memory's author from reviewing its edits, and a classified simulation row would taint the term. |
| **d10 — Journal a retirement for each classification at deprecation** | A deprecation of a widely used term would write one row per classification inside its commit, against ADR-0024 d1, and every retirement would be a fact the term's status already states. |
| **d10 — Recall follows `replaced_by` for retired classifications** | It would serve classifications no curator ratified; the human verdict on a deprecation attests the term's successor, not each memory's meaning. |
| **d10 — Retired classifications keep matching** | The curated layer would keep pointing recall at deprecated meanings, and nothing would prompt the crew to re-classify. |
| **d11 — A classified-terms field on the claim item** | The `/v1` response would carry lab-ring structure that the claim-edge reads already serve. |
| **d12 — The memory's workarea's policy, or both policies** | A content workarea's policy is about content and carries no vocabulary context; requiring both doubles every refusal path for one act whose content side already speaks through the curator's grant. |
| **d12 — `human_required_classes` naming an edge label** | One ratify act has no verdict count for a human verdict to complete, so it would need a new refusal; the grant shape already says who may ratify. |
| **d12 — Never let a curator ratify its own crew's classification** | Every vocabulary would need two ontology crews; the policy's crew axis already lets a tenant demand it where the stakes warrant. |
| **d12 — Exempt classifications from the crew axis** | A tenant whose vocabulary policy demands out-of-crew review would get it for terms and silently not for classifications. |
| **d13 — An `oml_classify` tool, or curator tools on MCP** | `oml_relate_propose` already proposes the edge, and a ratification on MCP would give the agent-safe subset a path to a decision (ADR-0014 d2). |
| **d14 — add-member printing the value when `--out` is absent** | An operator issuing a crew of agents would have each value in a terminal or a transcript; ADR-0026 d12 made `--out` the way out of that, and add-member has no reason to be the exception. |
| **d14 — A server-side crew-member endpoint** | A second issuance path with its own validation order and refusals, which ADR-0026 d12 rejected for `issue-agent`. |
| **d15 — "Sub-role" as the word** | It reads as a sixth role, which ADR-0018 d4 forbids. |
