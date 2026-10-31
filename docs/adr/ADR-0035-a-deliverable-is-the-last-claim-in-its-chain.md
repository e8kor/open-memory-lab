<!--
id: adr/ADR-0035-a-deliverable-is-the-last-claim-in-its-chain
tier: engineering
status: proposed
answers: what a deliverable is and what its words and files hold; how remember, supersede, recall and get carry files and in which forms a key may send them; how a file manifest names a claim's files and when it is refused; how attachment bytes are served by version, encrypted, bound and expired; where every stored object and lake table lives, and how one setting moves them from the local disk to a cloud store; how several deliverables coexist, are listed and stay independent; who records a deliverable's chain and how it is followed both ways; the build order after iteration 3
-->

# ADR-0035 — A deliverable is the last claim in its chain: remember, recall and get carry its files, a file manifest names them, and several coexist in one workarea

**Status:** proposed · 2026-10-31

**Amends:** ADR-0004 (d3, d4), ADR-0005 (d1, d4), ADR-0018 (d4), ADR-0022 (d6), ADR-0023 (d3, d8), ADR-0024 (d1), ADR-0025 (d4, d6, d8, d11, d12, d13, d14)

## Context

Agents finish work — a report, an analysis, a diagram, the evidence behind a finding — and OML
keeps only the memories that led to it. **The user's decisions**, not reopened here: a
**deliverable** is the **last claim in its chain**: its words describe it, its files are
[ADR-0004](ADR-0004-bodies-and-attachments.md) attachments bound by digest, `derives_from` names
what led to it, and its versions are supersessions under one `claim_key`
([ADR-0025](ADR-0025-the-agent-only-memory-surface.md) d10); **several coexist in one workarea**;
each is reviewed like every claim (R2, R3; ADR-0004 d5); and, on 2026-10-31, they ride remember,
recall and get, the server describing the file forms it accepts and the harness choosing, while
storage should "keep our basic installation simple but … allow us to scale in future", its
"underneath solution" replaceable "with cloud or local host, with local host enabled by default".

**What the build has**, at `02a385d`: no attachment substrate. Envelopes carry `"attachments": []`
(crates/oml-app/src/app.rs:1365, 2037), and a submit naming a digest answers
`submission.attachment_missing` (crates/transports/oml-transport-http/src/lib.rs:1453-1466); no
migration lays `upload_sessions` or `artifact_keys` (migrations/0001_init.sql:9-11); the keyring
holds only the signing key; no object-store or KMS port (crates/oml-app/src/ports.rs), `object_store`
or `deltalake` dependency (Cargo.toml), or object store in CI or compose (docker-compose.yml:7-9)
exists; no lineage read is routed (lib.rs:172-189); `oml_attach` and `oml_graph` answer "not
built" (mcp.rs:57). ADR-0026 (iteration 3),
ADR-0030 (in-progress workareas) and ADR-0031 (connections) are proposed on other branches, cited
by number and unlinked. d10 is the user's decision; d2 departs from the brief, as the maintainer
confirmed on 2026-10-31; each other decision is the maintainer's reading, which the user confirms
before acceptance. Proposed, not built.

## Decision

- **d1 — A deliverable is a claim of kind `deliverable`: finished work its words describe.** It is
  an `oml_remember` (`POST /v1/memories`, `verb: "remember"`) with `kind: "deliverable"`,
  journalled as `submission.remember`. Recall ranks it by its words, never its files' text; hits
  include served tips of kind `deliverable` whether or not `kinds` is sent, the session hook's
  (ADR-0029 d2) too, and `kinds` and `include_own_held` admit it beside `memory` and
  `derived_fact` (amends ADR-0025 d4, d8). Its successors are deliverables and no other kind
  becomes one (amends ADR-0025 d11). It suits reports, analyses, diagrams and evidence; code stays
  in git, hosting with claude.ai artifacts, datasets with [ADR-0020](ADR-0020-dataset-onboarding.md).

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
    (d4) with one `oml-domain` function, checks before anything is written that it is canonical and
    names exactly the other bound digests, each once, under each upload's recorded media type, and
    then writes it with the inline texts (d4) as a completed upload of the calling key, bound first,
    so at most 63 files fit [`submission.max_attachments`](../api/http-api.md#limits). A failure is
    `422` with `details.reason` ∈ `path`, `duplicate`, `entry`, `mismatch` (a builder defect) and
    `details.path` where one applies. The envelope shape is unchanged (ADR-0004 d2's additive rule),
    a version's identity commits to every path, and paths shred with the bytes
    ([ADR-0005](ADR-0005-erasure-by-crypto-shredding.md) d3).

- **d3 — Every attachment fetch names one version and answers as a download a browser may not run,
  and nothing renders on the API origin.** One digest may be stored once per version that binds it
  (d9), so `GET /v1/attachments/{digest}` gains a required `artifact_id` query parameter and `oml
  attach get` takes `<artifact_id> <digest>`: admission, the stored object and its DEK, the
  filename and the read-log row all come from that version, a `serve_id` admits only for the
  version it serves, and a version that does not bind the digest answers `access.not_found`. The
  fetch answers `Content-Disposition: attachment` with a filename sanitized from its path's last
  segment (`file-manifest.json` for the file manifest), `X-Content-Type-Options: nosniff` and
  `Content-Security-Policy: sandbox` ([fetch semantics](../api/http-api.md#fetch-semantics)),
  widening to every fetch what [security](../architecture/security.md#hostile-content-after-the-gate)
  commits for reviewers, since ADR-0031 puts its OAuth authorization page on this origin.

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
    `entry`, and the oneOf sits inside the items, so inputs stay flat (ADR-0025 d12). This and d5
    to d8 reshape tools whose shapes ADR-0022 d6 keeps (amends it).
    - **Inline text** has a `text/*` or `application/json` media type or a `+json` or `+xml` suffix
      (RFC 6839), SVG and Mermaid source included, else `request.validation`; its UTF-8 bytes stay
      unnormalized, since a file's bytes are its identity. Inline texts and the file manifest are
      encrypted and written, as completed uploads of the calling key, only after the request passes
      validation, grant admissibility (the [refusal order](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order))
      and the scan, immediately before `BEGIN`: a request refused before `BEGIN` writes nothing,
      and one refused inside the transaction leaves them unbound to expire (d9). So ADR-0004 d3's
      "the words' writer uploaded the bytes" holds, its session read as the record of an upload
      and not only its transport (amends d3). Words, inline texts and the file manifest total at
      most `submission.max_body_size` bytes (shipped 1 MiB), else `413 request.payload_too_large`.
    - **One write path.** Each form yields one completed-upload record, one `submission.attach`
      binding and one scan over the file's text where an extractor exists (ADR-0004 d6; the text
      itself when inline), and the file manifest is scanned as inline text. An uploaded file of an
      inline type whose bytes are not UTF-8 is vetoed, `submission.scan_veto` with finding
      `format.encoding` ([admission](../architecture/admission.md#the-rule-set-registry)), so no read decodes one.
    - **Versions and R1.** A successor re-sends every file it keeps (ADR-0025 d11; d9); a write
      still answers only a held claim, and promotes, signs, demotes and deletes nothing ([R1](../api/mcp-plugin.md#the-verb-ceiling-what-this-surface-can-never-do-r1)).

- **d5 — Reads serve file descriptors, never bytes; descriptors count toward `max_tokens` under
  counter `oml-tc/2`; get serves small text files inline on request.**
    - **Descriptors.** Every recall hit and get result that binds files carries `entry` and
      `attachments: [{digest, path, media_type, size_bytes}]` in path order, the file manifest not
      among them, and `AttachmentDescriptor` gains `path`. On the ring both move from
      `MemorySummary` to `Memory`, so listings carry neither (amends ADR-0025 d13); on MCP the claim
      item carries them on recall and get, where ADR-0025 d12 gave get alone `attachments` (amends
      it). The file manifest is decrypted on the serving path, never for a row the budget cuts.
    - **The budget.** `tokens(row) = ⌈(body_bytes + file_manifest_bytes) ÷ 4⌉`, where
      `file_manifest_bytes` is the clear `size_bytes` of its file manifest's `submission.attach` (0
      without files), folded to `memories.file_manifest_bytes`, standing for the descriptors that
      re-serialize it, a definition like the divisor. A changed function mints a counter (ADR-0025
      d6, amended): `oml-tc/2`, equal to `oml-tc/1` on rows without files, under ADR-0025 d7's
      unchanged cut.
    - **Text on get.** `oml_get` gains `include_text` (default `false`; `GET
      /v1/memories/{artifact_id}?include_text=true`): in path order, each inline-type file carries
      `text` while the texts stay within `submission.max_body_size`, the rest `text: null`;
      binaries come by d3's fetch or `oml attach get <artifact_id> <digest> --out <path>`, which
      checks the digest before writing.

- **d6 — The server describes the forms a key can use, and the harness selects one; nothing is
  negotiated.** The set is closed: `upload_session` and `inline_text`. One catalog serves every
  key, its `attachments` items the oneOf of both forms (d4). The workarea read gains
  `attachment_forms`, the calling key's forms, and `attachment_limits {max_body_size,
  max_attachments, max_attachment_size}` for `oml_workarea_info` (ADR-0014 d1; amends ADR-0025
  d14): a connector key (ADR-0031 d7) reads `["inline_text"]`, a bearer key both. A connector key
  opens no session (ADR-0031 d11) and no one is served an inline upload's id, so its
  `{upload_session}` item answers `submission.attachment_missing`; that half lands with ADR-0031's
  build, closed by `a_connector_keys_workarea_read_lists_only_the_inline_form`. The `oml-memory`
  skill and `initialize`'s `instructions` (MCP 2025-06-18, lifecycle, fetched 2026-10-31; unserved,
  mcp.rs:251-257) prefer an upload for any file on disk where the harness reaches the CLI or the
  [HTTP API](../api/http-api.md#attachments) under a bearer key, as Claude Code with the `oml`
  plugin does, so its bytes never pass through the model; and inline text for a short text it
  writes anyway, and through a connection, where no binary goes in or out ([FR-42](../product/roadmap.md)).

- **d7 — Several deliverables coexist in one workarea, each its own claim, and every act names one
  version.**
    - **Naming.** All kinds share the workarea's claim keys (ADR-0025 d18): a new deliverable on a
      `claim_key` a held, promoted or demoted version holds answers `409
      submission.claim_key_conflict` ([FR-31](../product/roadmap.md)'s residual), so the agent
      names each apart, as `q4-cost-report` and `q4-cost-deck`.
    - **Listing, the tip of each.** `GET /v1/memories?kind=deliverable`, an existing filter, lists
      each deliverable's promoted served tip in scope. On MCP, `oml_my_work` gains `kind` and
      `oml_recall` `kinds`, both ring filters, so an agent lists its own and finds others' by a
      query. Missing: no MCP tool rides the scoped listing ([FR-41](../product/roadmap.md)).
    - **Independence.** One memory may feed several deliverables, and a deck may derive from its
      report: R2's closure ([ADR-0001](ADR-0001-the-constitution.md) d2) keeps a shared memory's
      author off everything built from it, and demoting or erasing it leaves every deliverable's
      bytes, keyed apart (d9). A serve binds one version's digests ([ADR-0012](ADR-0012-admission-and-review.md)
      d5), ratification compare-and-sets one tip (ADR-0025 d10), a fetch names one version (d3),
      and rival exclusion is per target: none touches another.
    - **In an in-progress workarea**, the commit releases every deliverable inside together
      (ADR-0030 d12), so deliverables that ship apart live in sibling in-progress workareas, and a
      deck in one cites a report in the other only after that sibling commits (ADR-0030 d2).

- **d8 — The agent declares the chain, the server checks each link and infers none, and the
  lineage read follows it back and, with `direction=descendants`, forward.** `oml_learn` and
  `derives_from` name each step's parents; the server checks each is readable (ADR-0025 d3) and
  adds no link from what recall served (ADR-0001 d6). The `oml-memory` skill has the agent keep
  each used hit's `artifact_id` and resolve the claim keys the session hook prints (ADR-0029 d6)
  through `oml_my_work`. Back: `GET /v2/memories/{artifact_id}/lineage` and `oml_graph {roots}`,
  unreadable nodes redacted (FR-31); forward, "what was built from X", the same read and
  `oml_graph` with `direction=descendants`, bounded by `depth`, stopping at a node the caller
  cannot read, which is absent rather than redacted, so no one learns that unreadable work exists.

- **d9 — File bytes live behind the object-store port, in d10's store, under one DEK per stored
  object, wrapped by a tenant KEK through the KMS port**, built in pure Rust as the crates on paper
  ([ADR-0019](ADR-0019-hexagonal-architecture-and-plugins.md) d2; [ADR-0006](ADR-0006-pure-rust.md)'s `oml-objectstore`, `oml-kms`).
    - **Keys and binding.** A session's DEK is minted when it opens and wrapped into a new
      `upload_sessions.wrapped_dek`. `artifact_keys` trades its primary key (`tenant_id`,
      `artifact_id`), one DEK per version, for (`tenant_id`, `object_id`), the version's
      `artifact_id` for its body and the session's `id` for a file, with (`tenant_id`,
      `artifact_id`) indexed. Inside [ADR-0024](ADR-0024-performance-foundation.md) d1's one
      data-modifying statement (amends it), binding compare-and-sets each named session from
      `complete` and unexpired to a new state `bound`, moves its wrapped DEK into `artifact_keys`,
      and inserts the journal row only if every session moved, else the transaction rolls back with
      `submission.attachment_missing`: an upload binds into one version only, races included.
      Erasure, once built ([FR-1](../product/roadmap.md)), destroys one version's DEKs in one
      commit (amends ADR-0005 d1's "per artifact" to per stored object).
    - **Objects.** Every object's name ends in its `artifact_keys.object_id`, the version's
      `artifact_id` for a body and the session's `id` for a file, as
      `oml-quarantine/<tenant>/<digest>/<object_id>` and its `oml-bodies` twin (amends ADR-0004
      d4), else two versions binding unchanged bytes would put two ciphertexts at one name. A
      file's object is a run of frames, one per part or inline file: a 4-byte big-endian
      ciphertext length, a random 12-byte nonce (a retried part re-encrypts, and a counter would
      repeat one), then AES-256-GCM ciphertext and tag under the session DEK, with the session's
      `id` and part number as associated data, so a moved frame fails to decrypt, and the
      plaintext digest, checked at completion and on every serve (ADR-0004 d4), catches a dropped
      one. Each encrypted part is its own object at
      `oml-quarantine/<tenant>/uploads/<session_id>/<part_number>`, written by plain put on every
      backend, so a retry re-puts it; completion streams the frames in part order into the
      session's object with one `put_multipart`, the bytes `submission.attach`'s ciphertext digest
      covers, then deletes the parts.
    - **Lifecycle.** A held, rejected or dead-lettered version's objects stay in quarantine, since
      each can return to review ([admission](../architecture/admission.md#lifecycle)), and
      `quarantine.refused_expiry` spares them. After the ratifying commit, outside the chain lock,
      the lead copies a promoted version's ciphertext to `oml-bodies` and deletes the quarantine
      object once the copy's ciphertext digest matches; a fetch serves whichever exists. A session
      expires `uploads.session_expiry` after it opens (S2 ships 24 hours, the `idempotency.retention`
      floor of [configuration](../operations/configuration.md); FR-15 still decides session limits):
      the lead's sweep compare-and-sets an `open` or `complete` session past it to `expired`,
      nulling its wrapped DEK, and deletes its parts and object once that commits, so a session is
      bound or expired, never both; it also retries an interrupted move. Each session row is written
      before its objects, so the keyless quarantine GC sweep deletes a part whose session is not
      `open` and a file object whose session is `expired`, never one whose row is to come; a body
      object keeps admission's rule, deleted when no journalled event references its digest.
    - **Copy classes** (amends ADR-0005 d4): `upload_sessions.wrapped_dek`, live until binding or
      expiry, then in base backups and WAL under the `artifact_keys.wrapped_dek` bound; and part
      objects, session-DEK ciphertext.
    - **The KEK, at its minimum.** One per tenant, its id in `artifact_keys.kek_id`, in the
      `kms-file` keyring beside the signing key (ADR-0023 d4), through ADR-0023 d8's external
      adapter once the kubernetes profile lands; custody stays ADR-0005's and [security](../architecture/security.md)'s.
      No rotation ships, so ADR-0005 d4's backup bound lacks its period ([FR-17](../product/roadmap.md)); the keyring is [RS-12](../product/roadmap.md).

- **d10 — One object-store port over the Rust `object_store` crate holds every stored object and
  lake table; one URL, `storage.url`, picks the backend, the local filesystem by default.** delta-rs
  is built on that crate and takes a caller's store (`DeltaTableBuilder::with_storage_backend`,
  deltalake 1.1.2; delta-rs `crates/core/Cargo.toml`), so each binary's composition builds one
  store with `object_store::parse_url_opts` from `storage.url` and `storage.options`, the crate's
  endpoint and credential keys, for `oml-objectstore` and, once the lake is built, `oml-lake`
  ([ADR-0003](ADR-0003-delta-lake-derived-lakehouse.md)). The workspace pins the `object_store`
  the pinned deltalake-core links (`^0.13.2` in 1.1.2 and 1.1.3; all fetched 2026-10-31), since
  another version's store is another type; S1's suite rechecks ADR-0023 F1, read at 0.14.2.
  `oml-lake` enables deltalake's `s3`, `azure` and `gcs` features and registers their handlers,
  since delta-rs picks its log store by the URL's scheme even when handed a store. One root holds
  the `oml-quarantine/`, `oml-bodies/` and `lake/<tenant>/` prefixes (amends ADR-0023 d3's
  directory names), each process holding the prefix credentials [deployment](../operations/deployment.md#workloads) grants it.
    - **The default.** Unset, as the standalone profile leaves it, `storage.url` is the `file://`
      URL of `oml-data` in the working directory, where the built `kms.keyring_path` default
      `oml-keyring` also lies (crates/bins/oml-server-lead/src/lib.rs:94-95), under ADR-0023 d3's
      NFS and SMB refusal, so a basic install runs no storage service; the docker and kubernetes
      `local` payloads set it to their MinIO (ADR-0023 d7), and `scaled` to S3. It replaces [configuration](../operations/configuration.md)'s
      required `object_store.uri` (amends ADR-0023 d8 and ADR-0004 d4's "S3-compatible").
    - **Scaling out is configuration, never code:** `s3://` (AWS S3, Cloudflare R2, MinIO, Garage),
      `az://` or `gs://`, which cluster-mode followers on other hosts need, since `file` serves one
      host. Startup refuses the crate's `memory` and `http` schemes, naming the setting: one forgets
      every object at restart, and the suite covers neither. A Delta commit needs put-if-absent
      ([storage](../architecture/storage.md#the-protocol)), so the lead and the lake-writer, which
      write, run ADR-0016 d5's scratch-key self-test on every scheme and refuse readiness when it
      fails. The crate documents R2 and MinIO as supporting it (`S3ConditionalPut::ETagMatch`) and
      deployment states AWS S3's as native; Garage, Azure and Google Cloud Storage are unverified,
      and the suite runs against no cloud service ([FR-46](../product/roadmap.md)).
    - **One conformance suite**, put, get, list, copy, conditional put, the part path (put a part,
      re-put it, assemble) and delete as expiry uses it, runs against the filesystem and MinIO
      (S1); MinIO runs only in the compose file, the kubernetes `local` profile on k3d or kind, and
      CI. `oml storage copy <from-url> <to-url>`, a
      keyless Rust subcommand, copies every object and table file unchanged with the writers stopped
      and checks each copy's SHA-256; names are relative to the root, so no row changes, and once
      the operator sets `storage.url`, ADR-0023 d10's full-genesis reconciliation checks the move.
      Designed, not built ([FR-43](../product/roadmap.md)).

- **d11 — Two vocabulary rows** (amends ADR-0018 d4):

    | Term | Means | Never |
    |---|---|---|
    | deliverable | a claim of kind `deliverable`: finished work, the last claim in its chain, whose words describe it and whose files its file manifest names | "artifact" or "document" as the kind's name (an `artifact_id` names a claim version; a claude.ai artifact is a hosted page) |
    | file manifest | a claim's first attachment when it binds files: the canonical JSON naming each file's path, digest and media type, and the entry path | "manifest" unqualified beside ADR-0020's import manifest |

- **d12 — The build follows iteration 3 in six steps, substrate first, each closed by its check.**
  Each changes its contract pages (OpenAPI, errors, HTTP API, `mcp-tools.json`, the agent plugin,
  configuration, the [Postgres schema](../data/postgres-schema.md)) with its code; migrations follow 0006.

    | Step | Lands | Check, tests named as sentences |
    |---|---|---|
    | S1 | d10's store: `storage.url`, `storage.options`, the default, the refused schemes, the self-test on every backend; the object-store and KMS ports; `oml-objectstore`; `oml-kms` (`kms-file`); `upload_sessions` and `artifact_keys` as d9 keys them; a `minio` service at ADR-0023 d7's pinned tag in `docker-compose.yml`, with `storage.url` set to it, and in the `test` job of `.github/workflows/ci.yml`, which runs the suite | `the_filesystem_and_minio_backends_pass_one_object_store_conformance_suite`, failing rather than skipping in CI when the MinIO endpoint is absent; `an_unset_storage_url_stores_under_the_data_directory`; `a_memory_or_http_storage_url_is_refused_at_startup`; `kms_file_wraps_and_unwraps_a_dek_and_never_stores_it_in_clear` |
    | S2 | upload sessions, the `bound` state, expiry and the lead's sweep, binding, `submission.attach`, the move at promotion, serves with every attachment, d3's fetch; d2's file manifest and d4's two forms on remember and supersede | `no_part_or_completed_upload_rests_as_plaintext_under_the_data_directory`, `a_frame_moved_to_another_position_fails_to_decrypt`, `an_unbound_session_past_its_expiry_has_no_wrapped_dek_and_no_object`, `a_session_the_sweep_expired_never_binds_and_a_bound_one_is_never_swept`, `the_keyless_gc_deletes_no_body_object_an_event_references_and_no_part_of_an_open_session`, `a_request_refused_before_begin_writes_no_object`, `a_promoted_versions_objects_move_from_quarantine_to_oml_bodies`, `one_key_uploading_the_same_bytes_twice_stores_two_objects_under_two_deks`, `a_completed_upload_binds_into_one_version_only`, `each_bound_file_has_its_own_wrapped_dek_row_under_its_version`, `a_verdict_counts_only_when_every_attachment_was_served_to_its_reviewer`, `a_fetch_serves_the_object_of_the_version_it_names`, `every_attachment_fetch_answers_as_a_download_with_scripts_disabled`, `a_file_manifest_that_does_not_name_exactly_the_other_bound_digests_is_refused`, `an_inline_text_and_an_uploaded_file_of_the_same_bytes_bind_the_same_digest_and_scan`, `inline_texts_past_the_body_bound_are_refused_as_payload_too_large`, `every_property_oml_remember_advertises_is_accepted_by_its_argument_struct` |
    | S3 | d5 and d6: descriptors, `oml-tc/2` with `memories.file_manifest_bytes` and retrieval's [token budgets](../architecture/retrieval.md#token-budgets), `include_text`, `attachment_forms` and `attachment_limits` | `recall_counts_a_hits_file_descriptors_under_oml_tc_2`, `a_listing_item_carries_no_file_descriptors`, `the_workarea_read_states_the_calling_keys_forms_and_limits` |
    | S4 | kind `deliverable`, its class, the `kind` and `kinds` filters | `a_new_deliverable_on_a_taken_claim_key_answers_claim_key_conflict`, `a_recall_without_kinds_serves_deliverable_tips`, `two_deliverables_in_one_workarea_keep_their_own_tips_when_one_is_superseded`, `the_scoped_listing_by_kind_lists_one_tip_per_deliverable` |
    | S5 | the lineage read both ways; `oml_graph` served | `the_descendants_read_lists_a_deck_built_from_a_report`, `the_descendants_read_stops_at_a_node_the_caller_cannot_read` |
    | S6 | `oml attach put` and `get`; the `oml-memory` skill and `instructions` naming d6's preferred forms; the session below | `attach_put_prints_the_session_id_and_never_the_bytes` and `attach_get_writes_a_file_only_after_its_digest_matches` in `cli_e2e.rs`; `scripts/check_plugin.py` fails when the skill names a tool or form the catalog lacks; `deliverables.rs` runs the session below in `cargo test --workspace` |

## Consequences

**One session, end to end**, in workarea `q4`: `agent-a` is a Claude Code key with writer and
reader, `agent-c` a bearer key with reader alone, `rev-1` and `rev-2` dreamer keys, and `M` a
promoted supplier quote another principal's key wrote; two deliverables share `M`, built at once.

| # | Key | Call | Answer |
|---|---|---|---|
| 1 | `agent-a` | `oml_recall {query: "supplier quote"}`; `oml_remember {claim_key: "q4-ask", text: "Asked for a Q4 cost report and a deck."}`; `oml_learn {claim_key: "q4-delta", text: …, derives_from: [M]}` | `M`, whose `artifact_id` it keeps; held `R`; held `F` |
| 2 | `agent-a` | the report, inline: `oml_remember {kind: "deliverable", claim_key: "q4-cost-report", text: "Q4 cost report\n\n…", entry: "report.md", attachments: [{path: "report.md", media_type: "text/markdown", text: …}, {path: "fig/cost.svg", media_type: "image/svg+xml", text: …}], derives_from: [R, F, M]}` | held `D1` |
| 3 | `agent-a` | the deck, at once: `oml attach put deck.pdf`, then `oml_remember {kind: "deliverable", claim_key: "q4-cost-deck", text: "Q4 board deck\n\n…", attachments: [{upload_session: U1, path: "deck.pdf"}], derives_from: [D1, M]}` | `U1`; held `D2` |
| 4 | `rev-1`, `rev-2` | `oml review next`: `R`, `F`, then `D1` with its file manifest and both files, digest-checked; `D2` | approving verdicts ratify all four; no key of `M`'s author's principal is served `F`, `D1` or `D2` |
| 5 | `agent-c` | `oml_recall {query: "Q4 cost", kinds: ["deliverable"]}`; `oml_get {artifact_id: D1, include_text: true}`; `oml attach get D2 <digest> --out deck.pdf` | `D1` and `D2` with descriptors; both report files' text; `deck.pdf`, written once its digest matches |
| 6 | `agent-c` | `oml_graph {roots: [D2]}`; `oml_graph {roots: [M], direction: "descendants"}` | `D2 → D1, M`, `D1 → R, F, M`, `F → M`; `F`, `D1`, `D2` |
| 7 | `agent-a` | report version 2: `oml_supersede {artifact_id: D1, …}` with both files re-sent | held; `D1` stays the tip until it is ratified, and `D2` is untouched; in an in-progress workarea the commit releases `D1` and `D2` together |

**Costs, stated.** Review scales with file count and size: every file of every version, file
manifest included, is served before a verdict counts (ADR-0004 d5), unchanged files are uploaded
and served again, and only comparing file manifests narrows the work ([FR-2](../product/roadmap.md)),
so an agent submits finished work, not drafts, as the `oml-memory` skill says. Inline text costs
the agent's output once per version, and whether claude.ai accepts a call near the 1 MiB cap is
unverified (FR-42). Wrapped DEKs multiply by file count, promotion copies every file once, each
served hit with files decrypts its file manifest, and a cloud backend adds a network round trip to
every object operation, all unmeasured against [FR-25](../product/roadmap.md); `oml-tc/2` changes
every count with files, the session hook's too.

**Opened.** [FR-40](../product/roadmap.md), deliverables, closes when S6's scenario passes in
continuous integration; FR-41 to FR-43 and FR-46 as above; FR-31 gains no residual, since the forward read
stops where the backward read redacts. New surface: `submission.file_manifest_invalid` (422), the
session state `bound`, `storage.url`, `storage.options` and the fields above; no contract page
changes now, so `info.version` stays `0.2.0-design`.

**Process obligations.** With the proposal: this file, a [register](index.md) row, a `mkdocs.yml`
nav entry, `llms.txt`, FR-40 to FR-43 and FR-46 on the [roadmap](../product/roadmap.md) marked proposed, one
[changelog](../changelog.md) entry. On acceptance, not before: this record and its register row
become `accepted design` with the date; the Status line of each record the Amends line names
(ADR-0022 and ADR-0024 among them) gains "· amended by ADR-0035"; ADR-0005 d4's table and the
[erasure inventory](../architecture/audit-and-lineage.md) gain d9's two copy classes;
[storage](../architecture/storage.md) states that the lake writes through the same port; storage,
admission and deployment name d10's one root and prefixes, and admission's [quarantine
placement](../architecture/admission.md#quarantine-placement), its keyless GC paragraph included,
d9's object names and both sweep rules; the budget-token row (ADR-0025 d6) and concepts' budget
sentence name `oml-tc/2`'s file manifest bytes; the D2–D5, D12, D13 and D15 cells name this
record; concepts gains a deliverables section and map row, and its and CONTRIBUTING's vocabulary
pointers name d11; the spec tag is cut.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **d1 — The file as the body, or recall indexing the files' text** | Words and bytes keep separate identities (ADR-0004): a re-issued file with the same words would claim a false supersession. Indexing would put up to 63 files of 64 MiB per version into the index, whose count would stop describing what recall serves. |
| **d2 — A file manifest for deliverables only** | The brief's wording, set aside by the maintainer on 2026-10-31: `path` would be required on deliverables and refused on every other kind, a conditionally required input (ADR-0025 d12). |
| **d2 — A client-built file manifest, or paths in the signed envelope** | An agent cannot compute digests reliably, and a second way to name files is a second write path; file names can be personal data, and the journal never erases (ADR-0005 d5). |
| **d3 — Inline rendering on the API origin** | Stored XSS against ADR-0031's authorization page: an HTML or SVG file rendered inline runs as the origin and could read or submit that page's form. A separate-origin preview reopens only under its own record. |
| **d4 — A separate `oml_deliver` tool** | More surface, and a second write path to keep equal to `oml_remember`; one extension serves every kind. |
| **d4 — A URL-fetch form, `{url}`** | The server fetching bytes brings SSRF and breaks ADR-0004 d3: the writer must have uploaded them. |
| **d4 — Inline base64 for small binaries** | A third larger than the bytes, through the model's output; a model holds no screenshot's bytes to emit; a scan sees an opaque binary as provenance and digest only (ADR-0004 d6); the harnesses that hold bytes upload over HTTP. Reopens when a harness without HTTP can pass bytes into a call the model does not generate. |
| **d5 — Descriptors left uncounted under `oml-tc/1`** | A hit with 63 files would serve far more than its count, misleading the agent that set the budget. |
| **d6 — A negotiation handshake**, such as a capability under `initialize`'s `experimental` | `/mcp` is stateless (ADR-0014 d2), with no session to hold a choice; `clientInfo` is the client's own statement and cannot decide what a server accepts; a oneOf schema already lets the client choose. |
| **d8 — Lineage inferred from recall** | Served is not used, and every inferred link would put a recalled memory's author into R2's closure under a provenance nobody declared. |
| **d9 — Re-encrypting each file under the version's DEK at bind** | A second pass over up to 64 MiB per file before `BEGIN`, on the write path ADR-0024 d1 keeps short. |
| **d10 — Hadoop and HDFS** | "HDFS is built using the Java language", a JVM against [ADR-0006](ADR-0006-pure-rust.md), with a NameNode and DataNodes to run even on one host; and it "is designed more for batch processing rather than interactive use", for files "gigabytes to terabytes in size" (HDFS Architecture, Hadoop 3.3.5, fetched 2026-10-31), where OML fetches files of at most 64 MiB one at a time. |
| **d10 — A second abstraction, such as OpenDAL, beside `object_store`** | delta-rs links `object_store` either way, so a second layer is a second set of options, credentials, retries and conditional-put semantics to configure and prove equal, and the suite would run twice. |
| **d10 — Files stored as Delta table columns** | Parquet stores rows of small fields, not one 64 MiB value per row; erasing one file rewrites every data file that holds it; and review needs a streamed fetch of a held file from quarantine, while the lake is derived, serves no read and never holds a held body (ADR-0003; [admission](../architecture/admission.md#quarantine-placement)). |
