<!--
id: adr/ADR-0035-a-deliverable-is-the-last-claim-in-its-chain
tier: engineering
status: proposed
answers: what a deliverable is and what its words and files hold; how remember, supersede, recall and get carry files and in which forms a credential may send them; how a file manifest names a claim's files and when it is refused; how attachment bytes are served and stored on one host; how several deliverables coexist, are listed and stay independent; who records a deliverable's chain and how it is followed both ways; how an author withdraws a held version; the build order after iteration 3
-->

# ADR-0035 — A deliverable is the last claim in its chain: remember, recall and get carry its files, a file manifest names them, and several coexist in one workarea

**Status:** proposed · 2026-10-31

**Amends:** ADR-0004 (d3, d4), ADR-0005 (d1), ADR-0012 (d5), ADR-0014 (d2), ADR-0018 (d4), ADR-0025 (d4, d6, d8, d11, d12, d14)

## Context

Agents finish work — a report, an analysis, a diagram, the evidence behind a finding — and OML
keeps only the memories that led to it. **The user's decisions**, not reopened here: a
**deliverable** (an `artifact_id` already names a claim version) is the **last claim in its
chain**: its words describe it, its files are [ADR-0004](ADR-0004-bodies-and-attachments.md)
attachments bound into its envelope by digest, `derives_from` names what led to it, and its
versions are supersessions under one `claim_key` ([ADR-0025](ADR-0025-the-agent-only-memory-surface.md)
d10), as a claude.ai artifact is republished to one URL; **several coexist in one workarea**; each
is reviewed like every claim (R2, R3; ADR-0004 d5); and (2026-10-31) they ride remember, recall
and get, the server describing the file forms it accepts and the harness choosing among them.

**What the build has**, at `02a385d`: no attachment substrate. Envelopes carry `"attachments": []`
(crates/oml-app/src/app.rs:1365, 2037); a submit naming a digest answers
`submission.attachment_missing` (crates/transports/oml-transport-http/src/lib.rs:1453-1466); no
migration lays `upload_sessions` or `artifact_keys` (migrations/0001_init.sql:9-11); the keyring
holds only the signing key, and no object-store or KMS port exists (crates/oml-app/src/ports.rs);
no lineage read is routed (lib.rs:172-189); `oml_attach` and `oml_graph` answer "not built"
(crates/transports/oml-transport-http/src/mcp.rs:57). ADR-0026 (iteration 3), ADR-0030
(in-progress workareas) and ADR-0031 (connections) are proposed on other branches, cited by number
and unlinked. Each decision is the maintainer's proposed reading, which the user confirms before
acceptance; d2 names the one that departs from the brief. Proposed, not built.

## Decision

- **d1 — A deliverable is a claim of kind `deliverable` whose words are a title line and a summary
  and which binds at least one file.** It is an `oml_remember` (`POST /v1/memories`, `verb:
  "remember"`) with `kind: "deliverable"`, journalled as `submission.remember`. Its NFC body is a
  non-empty first line, an empty line and a non-empty summary, else `request.validation` naming
  `body`; with no file beside its file manifest (d2) it is `submission.file_manifest_invalid`,
  reason `no_files`, so at most 63 files fit [`submission.max_attachments`](../api/http-api.md#limits).
  Recall ranks it by its words, never its files' text, and `kinds` and `include_own_held` admit it
  beside `memory` and `derived_fact` (amends ADR-0025 d4, d8). Its successors are deliverables and
  no other kind becomes one, a class of its own (amends ADR-0025 d11), so a memory never turns into
  a file carrier nor a deliverable sheds its files; its retirement version states why and binds no
  file. It suits reports, analyses, diagrams and evidence; code stays in git (the words may record
  a commit SHA), hosting with claude.ai artifacts, datasets with [ADR-0020](ADR-0020-dataset-onboarding.md).

- **d2 — Every claim that binds files names them in a file manifest, its first attachment, which
  the server builds and checks; one that does not name exactly the other bound digests is refused
  as `submission.file_manifest_invalid`.**

    ```json
    {"entry":"report.md","files":{"fig/cost.svg":{"digest":"sha256:…","media_type":"image/svg+xml"},"report.md":{"digest":"sha256:…","media_type":"text/markdown"}},"format":"oml-file-manifest/1"}
    ```

    Its bytes are that object's RFC 8785 (JCS) form, media type `application/json`; `entry`, the
    file to open first, defaults to the first item's path. A path is `/`-separated NFC segments,
    none empty, `.` or `..`, none holding `\` or a control character; no two paths are equal under
    Unicode default case folding or nest as directory and file, and no two files share an upload or
    a digest, so any filesystem can hold the set. The server builds it from the request's items
    (d4) with one `oml-domain` function, stores it as a completed upload of the calling key, binds
    it first, and before `BEGIN` ([ADR-0024](ADR-0024-performance-foundation.md) d1) checks that it
    is canonical and names exactly the other bound digests, each once, under each upload's recorded
    media type. A failure is `422` with `details.reason` ∈ `no_files`, `path`, `duplicate`,
    `entry`, `mismatch` (a builder defect, refused rather than bound) and `details.path` where one
    applies. The envelope shape is unchanged (ADR-0004 d2's additive rule), a version's identity
    commits to every path, and paths shred with the bytes
    ([ADR-0005](ADR-0005-erasure-by-crypto-shredding.md) d3). **Departing from the brief**, whose
    deliverable kind "adds only the manifest rule and listability": outside a file manifest a path
    has a clear column nothing shreds, or no home, so every kind that binds files carries one, and
    the deliverable kind adds the words rule, the file minimum and listability.

- **d3 — Every attachment fetch answers as a download a browser may not run, and nothing renders
  on the API origin.** `GET /v1/attachments/{digest}` answers `Content-Disposition: attachment`
  with a filename sanitized from its path's last segment, `X-Content-Type-Options: nosniff` and
  `Content-Security-Policy: sandbox` ([fetch semantics](../api/http-api.md#fetch-semantics)),
  widening to every fetch what [security](../architecture/security.md#hostile-content-after-the-gate)
  commits for reviewers. ADR-0031 puts the OAuth authorization page on this origin, where an HTML
  or SVG file rendered inline would run as the origin and could read or submit that page's form:
  stored XSS. Under `sandbox`, a document rendered anyway is an opaque origin without scripts. An
  inline preview is rejected; one would come from an origin sharing no registrable domain with it.

- **d4 — `oml_remember` and `oml_supersede`, and the bodies they ride, carry files in two forms,
  each ending as the same completed upload, binding and scan.** An `attachments` item is one of:
    - `{upload_session, path}`, a completed upload session of the calling key, for large or binary
      files; the CLI gains `oml attach put <file> [--media-type <type>]`, which uploads over HTTP
      under `OML_URL` and `OML_API_KEY` ([ADR-0029](ADR-0029-session-start-hook.md)) and prints the
      completed session's id, never the bytes;
    - `{path, media_type, text}`, inline UTF-8 text, for an agent with no HTTP upload: a
      connection's only file path, since upload parts need a key value a connector key lacks
      (ADR-0031 d7, d11).

    The ring's bare digest strings go (a pre-release rewrite), the request gains an optional
    `entry`, and the oneOf sits inside the items, so inputs stay flat (ADR-0025 d12).
    - **Inline text** has a `text/*` or `application/json` media type or a `+json` or `+xml` suffix
      (RFC 6839), SVG and Mermaid source included, else `request.validation`. Its UTF-8 bytes, kept
      unnormalized since a file's bytes are its identity, become a completed upload owned by the
      calling key, encrypted and written before `BEGIN`. So ADR-0004 d3's "the words' writer
      uploaded the bytes" holds, its session read as the record of an upload and not only its
      transport (amends d3). Words and inline texts total at most `submission.max_body_size` UTF-8
      bytes (shipped 1 MiB), else `413 request.payload_too_large`; a refused request's inline
      uploads stay unbound and expire.
    - **One write path.** Each form yields one completed-upload record, one `submission.attach`
      binding and one scan over the file's text where an extractor exists (ADR-0004 d6; the text
      itself when inline):
      `an_inline_text_and_an_uploaded_file_of_the_same_bytes_bind_the_same_digest_and_scan`.
    - **Versions and R1.** A successor re-sends every file (ADR-0025 d11; d10). `oml_remember`
      still answers only a held claim, its uploads bind into it or expire, and it promotes, signs,
      demotes and deletes nothing ([R1](../api/mcp-plugin.md#the-verb-ceiling-what-this-surface-can-never-do-r1)).

- **d5 — Reads serve file descriptors, never bytes; descriptors count toward `max_tokens` under
  counter `oml-tc/2`; get serves small text files inline on request.**
    - **Descriptors.** Every recall hit and get result that binds files carries `entry` and
      `attachments: [{digest, path, media_type, size_bytes}]` in path order: `AttachmentDescriptor`
      gains `path`, and hits gain the field ADR-0025 d12 gave get alone (amends it); listings stay
      summaries. The file manifest is decrypted with the body on the serving path; a row the budget
      cuts never is.
    - **The budget.** `tokens(row) = ⌈(body_bytes + file_manifest_bytes) ÷ 4⌉`, where
      `file_manifest_bytes` is the clear `size_bytes` of its file manifest's `submission.attach` (0
      without files): the descriptors re-serialize the file manifest, so its length stands for
      theirs, a definition like the divisor. A changed function mints a counter (ADR-0025 d6,
      amended): `oml-tc/2`, equal to `oml-tc/1` on rows without files, under d7's unchanged cut.
    - **Text on get.** `oml_get` gains `include_text` (default `false`; `GET
      /v1/memories/{artifact_id}?include_text=true`): in path order, each inline-type file carries
      `text` while the texts stay within `submission.max_body_size`, and the rest, binaries
      included, `text: null`. Binaries come by `GET /v1/attachments/{digest}` or `oml attach get
      <digest> --out <path>`, which checks the digest before writing. A connection's access token
      reaches `/mcp` alone (ADR-0031 d11), so it never reads a binary's bytes
      ([FR-42](../product/roadmap.md)).

- **d6 — The server describes the forms a credential can use, and the harness selects one;
  nothing is negotiated.** The set is closed: `upload_session` and `inline_text`. `/mcp`
  authenticates every call, `tools/list` included (mcp.rs:190-195), so `tools/list` serves
  `oml_remember` and `oml_supersede` with the calling key's forms: a connector key (ADR-0031 d7)
  the inline form, a bearer key both. Served descriptions state the deployment's caps, and the
  workarea read gains `attachment_forms` and `attachment_limits {max_body_size, max_attachments,
  max_attachment_size}` for `oml_workarea_info` (ADR-0014 d1; amends ADR-0025 d14). A form the key
  cannot use is `403 oauth.attach_refused`, ADR-0031 d11's code for `oml_attach` `open`, before any
  session lookup; without ADR-0031 every key is a bearer key. The table below moves to the
  [agent plugin](../api/mcp-plugin.md) page when built; the `oml-memory` skill and `initialize`'s
  `instructions` (MCP 2025-06-18, lifecycle, fetched 2026-10-31), which the build does not serve
  yet (mcp.rs:251-257), name the form to prefer, and `clientInfo` may tailor that text, never what
  is accepted.

    | Harness | Reaches | Prefers |
    |---|---|---|
    | Claude Code with the `oml` plugin | MCP, and the `oml` CLI over HTTP under the same bearer key | `oml attach put` for any file on disk, any binary, anything large: its bytes never pass through the model; inline for a short text it is writing anyway |
    | claude.ai through a connection (ADR-0031) | MCP alone | inline text; no binary file goes in or comes back ([FR-42](../product/roadmap.md)) |
    | a generic MCP client with a shell | MCP, and HTTP from the shell under a bearer key | as Claude Code: `oml attach put`, or the [HTTP API](../api/http-api.md#attachments)'s three upload calls |

- **d7 — Several deliverables coexist in one workarea, each its own claim, and every act names one
  version.**
    - **Naming.** All kinds share the workarea's claim keys (ADR-0025 d18): a new deliverable on a
      `claim_key` a held, promoted or demoted version holds answers `409
      submission.claim_key_conflict` ([FR-31](../product/roadmap.md)'s residual), so the agent
      names each apart, as `q4-cost-report` and `q4-cost-deck`.
    - **Shared chains.** One memory may feed several deliverables, and one deliverable may derive
      from another, as a deck from its report; R2's closure ([ADR-0001](ADR-0001-the-constitution.md)
      d2) keeps a shared memory's author off everything built from it. Demoting or erasing a
      shared memory leaves every deliverable's bytes, which are keyed apart (d10).
    - **Listing, the tip of each.** `GET /v1/memories?kind=deliverable`, an existing filter, lists
      each deliverable's promoted served tip in scope. On MCP, `oml_my_work` gains `kind` and
      `oml_recall` `kinds`, both ring filters, so an agent lists its own and finds others' by a
      query. Missing: no MCP tool rides the scoped listing ([FR-41](../product/roadmap.md)).
    - **Independence.** A serve binds one version's digests
      ([ADR-0012](ADR-0012-admission-and-review.md) d5), ratification compare-and-sets one
      (`workarea_id`, `claim_key`) tip (ADR-0025 d10), rival exclusion is per target, and a
      withdrawal names one `artifact_id` (d9): none touches another deliverable.
    - **In an in-progress workarea** (ADR-0030), the commit releases every deliverable inside
      together and is refused while any version inside is held (ADR-0030 d12), so deliverables
      that ship apart live in sibling in-progress workareas. No claim reads into another subtree
      (ADR-0030 d2), so a memory both share is promoted outside them, and a deck in one cites a
      report in the other only after that sibling's commit.

- **d8 — The agent declares the chain, the server checks each link and infers none, and the
  lineage read follows it back and, with `direction=descendants`, forward.** `oml_learn` and
  `derives_from` name each step's parents; the server checks each is readable (ADR-0025 d3) and
  adds no link from what recall served, since served is not used and an inferred link would put a
  recalled memory's author into R2's closure under a provenance nobody declared (ADR-0001 d6). The
  `oml-memory` skill has the agent keep each used hit's `artifact_id` and resolve the claim keys
  the session hook prints (ADR-0029 d6) through `oml_my_work`. Back: `GET
  /v2/memories/{artifact_id}/lineage` and `oml_graph {roots}`, unreadable nodes redacted (FR-31).
  Forward, "what was built from X": the same read with `direction=descendants`, and `oml_graph`
  with `direction`, bounded by `depth`, an unreadable descendant absent rather than redacted, so
  no one learns that unreadable work exists. Neither is built: no lineage read is routed, and
  `oml_graph` answers "not built".

- **d9 — An author may withdraw its own held version: one `lifecycle.withdraw`, state `withdrawn`,
  bytes kept, served to its author alone, out of review.** `POST
  /v2/memories/{artifact_id}/withdraw`, no body, `Idempotency-Key` scoped as supersede's (ADR-0025
  d10), and a new tool, `oml_withdraw {artifact_id, idempotency_key}` (amends ADR-0014 d2, ADR-0025
  d12). Refusals, in order: a role admitting neither writer nor dreamer, `access.verb_ceiling`,
  before any lookup; unreadable, `access.not_found`; another key's, `access.author_only`; not held,
  `409 lifecycle.not_held` with `details.state`, naming the retire-proposal for a promoted claim
  (ADR-0025 d11). The commit journals `lifecycle.withdraw {artifact_id, version, workarea_id,
  claim_key}` and invalidates its open serves, so a later verdict answers `review.serve_invalid`
  (amends ADR-0012 d5, whose serves ended only by a change of content): review, reopen and recall
  never reach it again; get, the own-work listing, the fetch and the tail's author clause serve it
  to its author alone; only an admin's erase follows. A withdrawn version 1 leaves the
  `memories_claim_version` index and releases its `claim_key` as a never-promoted rejection does
  (ADR-0025 d18); a withdrawn successor stops excluding its author from rivals' review; and no
  withdrawn version blocks an in-progress commit (ADR-0030 d12). R1: it promotes, signs and
  deletes nothing, and it is no demotion, an admin's act on a ratified version. R2: it records no
  verdict. R3: visibility only narrows. R4: it is journalled and keeps the bytes and their keys,
  which only `lifecycle.erase` destroys, so it is no deletion.

- **d10 — File bytes live behind the object-store port under one DEK per stored object, wrapped by
  a tenant KEK through the KMS port.** This slice builds the ports and crates that exist on paper
  ([ADR-0019](ADR-0019-hexagonal-architecture-and-plugins.md) d2; [ADR-0006](ADR-0006-pure-rust.md)'s
  `oml-objectstore`, `oml-kms`) in pure Rust: `object_store`'s `LocalFileSystem` under the
  standalone lead's data directory ([ADR-0023](ADR-0023-install-profiles-and-cluster-mode.md) d3,
  NFS and SMB refused), and `AmazonS3` against S3 or MinIO for docker, kubernetes and cluster mode,
  whose followers share no disk, chosen by the object-store URI (ADR-0023 d8).
    - **Names.** ADR-0004 d4's `oml-bodies/<tenant>/<digest>` and its quarantine twin end in the
      upload session's id or a body's `artifact_id`; else one key re-uploading unchanged bytes for
      a next version would put two ciphertexts at one name (amends ADR-0004 d4).
    - **DEKs.** A session's DEK is minted when it opens and each part is encrypted on arrival, so no
      part rests in plaintext; completion decrypts the parts to check the digest, and the binding
      capture moves the wrapped DEK into `artifact_keys` under its version. An upload binds into
      one version only, so erasing a version destroys its body's and files' DEKs in one commit and
      no other version's: ADR-0005 d1's "per artifact" becomes per stored object (amends it).
    - **The KEK, at its minimum.** One KEK per tenant, its id in `artifact_keys.kek_id`: in the
      `kms-file` keyring beside the signing key (ADR-0023 d4), and through ADR-0023 d8's external
      adapter once the kubernetes profile lands. Custody and rotation stay ADR-0005's and
      [security](../architecture/security.md)'s; this slice ships no rotation, so ADR-0005 d4's
      backup bound lacks its rotation period ([FR-17](../product/roadmap.md)), and the keyring
      mechanism is [RS-12](../product/roadmap.md).

- **d11 — Three vocabulary rows** (amends ADR-0018 d4):

    | Term | Means | Never |
    |---|---|---|
    | deliverable | a claim of kind `deliverable`: finished work, the last claim in its chain, whose words are a title and a summary and whose files its file manifest names | "artifact" (an `artifact_id` names a claim version; a claude.ai artifact is a hosted page), "output", "document" as the kind's name |
    | file manifest | a claim's first attachment when it binds files: the canonical JSON naming each file's path, digest and media type, and the entry path | "manifest" unqualified beside ADR-0020's import manifest, "index", "bundle" |
    | withdrawn | a held version its author took out of review by a journalled act; kept, and served to its author alone | "deleted", "rejected", "retired", "abandoned" as a state |

- **d12 — The build follows iteration 3 in seven steps, substrate first, each closed by its check.**
  Each step changes its contract pages (OpenAPI, errors, HTTP API, `mcp-tools.json`, the agent
  plugin) with its code; migrations number after iteration 3's 0006.

    | Step | Lands | Check, tests named as sentences |
    |---|---|---|
    | S1 | object-store and KMS ports; `oml-objectstore` (filesystem, S3); `oml-kms` (`kms-file`); `upload_sessions`, per-object `artifact_keys` | `the_filesystem_and_s3_adapters_pass_one_object_store_contract`, `no_part_or_completed_upload_rests_as_plaintext_under_the_data_directory`, `one_key_uploading_the_same_bytes_twice_stores_two_objects_under_two_deks` |
    | S2 | upload sessions, binding, `submission.attach`, serves with every attachment, d3's fetch, file DEKs at erase | `a_verdict_counts_only_when_every_attachment_was_served_to_its_reviewer`, `every_attachment_fetch_answers_as_a_download_with_scripts_disabled`, `a_completed_upload_binds_into_one_version_only`, `erasing_a_version_destroys_every_dek_its_files_hold_and_no_other` |
    | S3 | d4–d6: both forms on remember and supersede, the file manifest, descriptors, `oml-tc/2`, `include_text`, the per-key schema and limits | `an_inline_text_and_an_uploaded_file_of_the_same_bytes_bind_the_same_digest_and_scan`, `inline_texts_past_the_body_bound_are_refused_as_payload_too_large`, `a_file_manifest_that_does_not_name_exactly_the_other_bound_digests_is_refused`, `a_connector_key_is_served_only_the_inline_form_and_refused_an_upload_session`, `recall_counts_a_hits_file_descriptors_under_oml_tc_2`, `every_property_oml_remember_advertises_is_accepted_by_its_argument_struct` |
    | S4 | kind `deliverable`, its class, the `kind` and `kinds` filters | `a_deliverable_without_a_title_a_summary_and_a_file_is_refused`, `a_new_deliverable_on_a_taken_claim_key_answers_claim_key_conflict`, `two_deliverables_in_one_workarea_keep_their_own_tips_when_one_is_superseded`, `the_scoped_listing_by_kind_lists_one_tip_per_deliverable` |
    | S5 | withdrawal: state, event, route, `oml_withdraw`, `lifecycle.not_held` | `a_withdrawn_version_is_served_to_its_author_alone_and_never_by_review_next`, `withdrawing_a_promoted_version_answers_not_held`, `a_withdrawn_first_version_releases_its_claim_key` |
    | S6 | the lineage read both ways; `oml_graph` served | `the_descendants_read_lists_a_deck_built_from_a_report`, `a_descendant_the_caller_cannot_read_is_absent_from_the_descendants_read` |
    | S7 | `oml attach put` and `get`; the `oml-memory` skill, the harness table, `instructions`; the session below | `attach_put_prints_the_session_id_and_never_the_bytes` and `attach_get_writes_a_file_only_after_its_digest_matches` in `cli_e2e.rs`; `scripts/check_plugin.py` fails when the skill names a tool or form the catalog lacks; `deliverables.rs` runs the session below in `cargo test --workspace` |

## Consequences

**One session, end to end**, in workarea `q4`: `agent-a` is a Claude Code key with writer and
reader, `agent-c` a claude.ai connector key with reader, `rev-1` and `rev-2` dreamer keys, and `M`
a promoted supplier quote another principal's key wrote. Two deliverables are built at once and
share `M`; idempotency keys are omitted.

| # | Key | Call | Answer |
|---|---|---|---|
| 1 | `agent-a` | `oml_recall {query: "supplier quote"}`; `oml_remember {claim_key: "q4-ask", text: "Asked for a Q4 cost report and a deck."}`; `oml_learn {claim_key: "q4-delta", text: …, derives_from: [M]}` | `M`, whose `artifact_id` it keeps; held `R`; held `F` |
| 2 | `agent-a` | the report, inline: `oml_remember {kind: "deliverable", claim_key: "q4-cost-report", text: "Q4 cost report\n\n…", entry: "report.md", attachments: [{path: "report.md", media_type: "text/markdown", text: …}, {path: "fig/cost.svg", media_type: "image/svg+xml", text: …}], derives_from: [R, F, M]}` | held `D1` |
| 3 | `agent-a` | the deck, at once: `oml attach put deck.pdf`, then `oml_remember {kind: "deliverable", claim_key: "q4-cost-deck", text: "Q4 board deck\n\n…", attachments: [{upload_session: S1, path: "deck.pdf"}], derives_from: [D1, M]}` | `S1`; held `D2` |
| 4 | `agent-a` | its chart is wrong before any review: `oml_withdraw {artifact_id: D2}`, a fresh upload, and the same call under the released `claim_key` | `D2` withdrawn; held `D3` |
| 5 | `rev-1`, `rev-2` | `oml review next`: `R`, `F`, then `D1` with its file manifest and both files, digest-checked; `D3` | approving verdicts ratify all four; no key of `M`'s author's principal is served `F`, `D1` or `D3`, and no one `D2` |
| 6 | `agent-c` | `oml_recall {query: "Q4 cost", kinds: ["deliverable"]}`; `oml_get {artifact_id: D1, include_text: true}` | `D1` and `D3` with descriptors; both report files' text; `deck.pdf`'s bytes reach `agent-a` by `oml attach get`, never `agent-c` (FR-42) |
| 7 | `agent-c` | `oml_graph {roots: [D3]}`; `oml_graph {roots: [M], direction: "descendants"}` | `D3 → D1, M`, `D1 → R, F, M`, `F → M`; `F`, `D1`, `D3`; `D2` in neither |
| 8 | `agent-a` | report version 2: `oml_supersede {artifact_id: D1, …}` with both files re-sent | held; `D1` stays the tip until it is ratified, and `D3` is untouched; in an in-progress workarea the commit releases `D1` and `D3` together |

**Costs, stated.** Review scales with file count and size: every file of every version, file
manifest included, is served before a verdict counts (ADR-0004 d5), unchanged files are uploaded
and served again (d10), and only comparing file manifests narrows the work
([FR-2](../product/roadmap.md)). So drafts should not be submitted: an agent drafts in its working
tree or git and submits when done, as the `oml-memory` skill says; a withdrawal stops further
review, not the review spent. Inline text costs the agent's output once per version, and whether
claude.ai accepts a call near the 1 MiB cap is unverified (FR-42). Wrapped DEKs multiply by file
count and each served hit with files decrypts its file manifest, unmeasured against
[FR-25](../product/roadmap.md); `oml-tc/2` changes every count with files, the session hook's too.

**Opened.** [FR-40](../product/roadmap.md), deliverables, closes when S7's scenario passes in
continuous integration; FR-41 and FR-42 as above; FR-31 gains no residual, since the forward read
omits what the backward read redacts. New surface: codes `submission.file_manifest_invalid` (422)
and `lifecycle.not_held` (409), state `withdrawn`, event `lifecycle.withdraw`, tool `oml_withdraw`
and the fields above; no contract page changes now, so `info.version` stays `0.2.0-design`.

**Process obligations.** With the proposal: this file, a [register](index.md) row, a `mkdocs.yml`
nav entry, `llms.txt`, FR-40 to FR-42 on the [roadmap](../product/roadmap.md) marked proposed, one
[changelog](../changelog.md) entry. On acceptance, not before: this record and its register row
become `accepted design` with the date; the Status lines of ADR-0004, ADR-0005, ADR-0012,
ADR-0014, ADR-0018 and ADR-0025 gain "· amended by ADR-0035"; the D3, D4, D11, D12 and D13 cells
name it; concepts gains a deliverables section and map row, and its and CONTRIBUTING's vocabulary
pointers name d11; the spec tag is cut. With each step's code, the pages that own what it builds
take it.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **d1 — The file as the body, or recall indexing the files' text** | Words and bytes keep separate identities (ADR-0004): a re-issued file with the same words would claim a false supersession. Indexing would put up to 63 files of 64 MiB per version into the index, whose count would stop describing what recall serves. |
| **d2 — A file manifest for deliverables only** | The brief's wording. Another kind's paths would need a clear column the erasure inventory never shreds, or be dropped; the user may still choose it, with `path` null on other kinds. |
| **d2 — A client-built file manifest, or paths in the signed envelope** | An agent cannot compute digests reliably, and a second way to name files is a second write path; file names can be personal data, and the journal never erases (ADR-0005 d5). |
| **d3 — Inline rendering on the API origin** | Stored XSS against ADR-0031's authorization page; a separate-origin preview reopens only under its own record. |
| **d4 — A separate `oml_deliver` tool** | More surface, and a second write path to keep equal to `oml_remember`; one extension serves every kind. |
| **d4 — A URL-fetch form, `{url}`** | The server fetching bytes brings SSRF and breaks ADR-0004 d3: the writer must have uploaded them. |
| **d4 — Inline base64 for small binaries** | A third larger than the bytes, through the model's output; a model holds no screenshot's bytes to emit; a scan sees an opaque binary as provenance and digest only (ADR-0004 d6); the harnesses that hold bytes upload over HTTP. Reopens when a harness without HTTP can pass bytes into a call the model does not generate. |
| **d5 — Descriptors left uncounted under `oml-tc/1`** | A hit with 63 files would serve far more than its count, misleading the agent that set the budget. |
| **d6 — A negotiation handshake**, such as a capability under `initialize`'s `experimental` | `/mcp` is stateless (ADR-0014 d2), with no session to hold a choice; `clientInfo` is the client's own statement and cannot decide what a server accepts; a oneOf schema already lets the client choose. |
| **d8 — Lineage inferred from recall** | Served is not used, and every inferred link widens R2's exclusion with a provenance nobody declared. |
| **d9 — Withdrawal as the author's rejection, or of a promoted version** | The first is a verdict by the author on its own row, against R2; the second, a writer removing reviewed content, is an agent's demote, against R1, where the retire-proposal is reviewed. |
| **d10 — Re-encrypting each file under the version's DEK at bind** | A second pass over up to 64 MiB per file before `BEGIN`, on the write path ADR-0024 d1 keeps short. |
