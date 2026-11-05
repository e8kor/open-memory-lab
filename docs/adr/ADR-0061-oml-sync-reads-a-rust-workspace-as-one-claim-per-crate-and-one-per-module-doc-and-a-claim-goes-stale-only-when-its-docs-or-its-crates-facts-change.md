<!--
id: adr/ADR-0061-oml-sync-reads-a-rust-workspace-as-one-claim-per-crate-and-one-per-module-doc-and-a-claim-goes-stale-only-when-its-docs-or-its-crates-facts-change
tier: engineering
status: proposed
answers: what the `rust` source adapter of `oml sync` extracts from a Cargo workspace and what it never reads; why manifests are parsed from committed bytes and not by `cargo metadata`; why a prefix lexer reads module docs and syn is a test oracle only; how the adapter port reads a set of files and stays pure; each claim's key, anchor, fragment, hasher and text; why a code edit, a new pub item or an external dependency leaves a claim fresh; why dependencies are claim text and not edges; how the adapter avoids retiring what it did not write; which workareas the operator's targets load into; who reviews crate and module claims; volume, cost and the dry run; which of the operator's targets ship in v0.5.8 and which in v0.5.9; the tests, doctor lines and rejected alternatives
-->

# ADR-0061 — `oml sync` reads a Rust workspace as one claim per crate and one per module doc, and a claim goes stale only when its docs or its crate's facts change

**Status:** proposed · 2026-11-05

**Release:** planned for v0.5.9, right after v0.5.8 ([releases](../product/releases.md#planned-releases))

**Amends:** ADR-0057 (d3: two hashers join the closed set; d7: the port reads a set of files, and
`rust` joins the adapters; d8: an adapter retires only claims in its own key namespace),
ADR-0047 (d1: the skeleton entry for a crate is realised as one claim; d4: a module's doc block
has a hasher of its own; d9: one crate claim holds the mechanical part and the purpose, so an
automated reviewer re-derives the whole claim or approves none of it); and, on its branch,
ADR-0056 (d11: a `.rs` file's module doc is `text/markdown`, never a code snippet)

**Depends on:** [ADR-0057](ADR-0057-a-memory-carries-the-source-anchor-its-author-declares-and-oml-sync-keeps-a-workarea-in-step-with-a-repositorys-documents.md)
(anchors, `oml sync`, its diff and batches, planned for v0.5.8);
[ADR-0055](ADR-0055-a-person-reviews-a-listed-set-of-held-items-in-one-act-each-item-judged-and-signed-as-one-verdict.md)
(bulk review).

**Numbering.** ADR-0058 to ADR-0060 are left to lanes in flight on 2026-11-05; this record takes
0061, and its roadmap rows take FR-130 to FR-133 for the same reason. Either is renumbered at
merge if the gap is not taken.

## Context

**The operator's decision of 2026-11-05.** `oml sync` loads OML's own source as memories, in four
scopes: one claim per crate (its purpose, dependencies and dependents); one claim per module (its
`//!` doc); the markdown documents under `docs/`, through the `headings` adapter; and the
decision records, through the `adr` adapter. Public item docs (`///`) come later. The Rust adapter
ships as v0.5.9, right after v0.5.8.

**What the tree holds,** measured at `origin/main` `ad2daa3` on 2026-11-05 by reading git's object
store with a script kept outside the tree:

- nine packages under `crates/`, plus `testing/oml-port-contracts`, a test-support package that
  only `oml-pg` and `oml-rerank` name as a dev-dependency;
- 161 `.rs` files, 145,928 lines: 94 under `crates/*/src` or `crates/*/*/src` (64,969 lines), 62
  under `tests/`, 4 under `examples/`, 1 under `testing/`;
- every one of the 94 source files opens with a `//!` doc; 11 are crate roots (`lib.rs` in all
  nine, `main.rs` in `oml-cli` and `oml-server-lead`), so 83 are modules. Every one of the 83 is
  declared by `mod <name>;` in its parent; 53 of those declarations are `pub`, 30 private;
- no source file uses `#![doc = …]`, `/*! … */`, `#[path = …]`, `include!` or an `@generated`
  marker, and no inline `mod x { //! … }` carries a doc;
- the module docs run to 418 characters at the median and 1,714 at most, 40,969 in all; the
  eleven root docs to 9,894 in all, 2,209 at most;
- each of the nine manifests has a `description`, 90 to 249 characters; there are 12
  workspace-internal normal dependency edges among the nine (13 with `oml-port-contracts`);
- 1,166 `pub` items at the start of a line, that is, at module level, across the 94 files
  (the operator's count of 1,449 includes items inside `impl` blocks).

**How often each fact changes,** from the 279 non-merge commits on `main` that touch `crates/`:
817 changes to an existing source file; 65 of them changed the file's leading `//!` doc, 181
changed the set of module-level `pub` item names, 143 changed the names and not the doc, and 609
changed neither. Of 29 changes to a crate's `Cargo.toml`, 6 changed its description, its
workspace-internal dependencies or its binaries; 17 did once external dependency names are
counted too.

**What it is held against.** ADR-0047 d1: a small, stable skeleton plus the knowledge code cannot
tell, and no per-file or per-symbol map of raw code. ADR-0047 d9: an automated reviewer approves
only skeleton facts it re-derives, never model-written prose. ADR-0057 d7: an adapter is a pure
function of what it reads. The constitution ([ADR-0001](ADR-0001-the-constitution.md)): R1, no
agent key promotes; R2, the reviewer is never the author; R4, nothing is deleted. The Occam rule
([ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md) d8): every new call traces to a documented
use case.

Decided, not built: this lane writes no code and ran no build.

## Decision

- **d1 — The `rust` adapter reads a Cargo workspace's manifests and its source files' leading
  `//!` docs at one commit, and nothing else.** `oml sync <source> --workarea <w> --adapter rust`
  takes a directory whose tree at `--rev` holds one or more `Cargo.toml` files with a `[package]`
  table; it refuses a source with none, naming the rule. For each package under the source it reads
  the manifest and, for each file under that package's `src/` that is a crate root or a declared
  module, the doc block at the head of the file. It never reads:
    - **code bodies:** the lexer stops at the first token that is not whitespace, a comment, an
      inner doc or an inner attribute (d3), so no line of code reaches it;
    - **item docs** (`///`, `/** */`), on public or private items: the operator's later scope;
    - **tests:** `tests/`, `benches/`, `examples/`, `build.rs`, and any module whose declaration
      carries `#[cfg(test)]`;
    - **generated files:** nothing under `target/` or another ignored path is in the commit, and a
      file whose first line carries `@generated` is skipped and reported;
    - **undeclared files:** a `.rs` file under `src/` that no parent declares with `mod <name>;` is
      not compiled, and is reported `undeclared`, not extracted;
    - **packages outside the source:** `oml sync crates` reads the nine under `crates/` and not
      `testing/oml-port-contracts`, so a dependent outside the source is not listed (d6).

    Private modules are read like public ones: a module's doc explains the crate's structure to a
    reader with no checkout whether or not the module is exported, and "private items" here means
    item-level docs, which nothing in this record reads. The source must be a git clone's
    directory, read through the object store, under every rule of ADR-0057 d6.

- **d2 — Manifests are parsed from the committed bytes with the CLI's `toml` parser; `cargo
  metadata` is not run.** `oml-cli` already depends on `toml` (0.8.23 in the lock). The adapter
  reads `[package] name` and `description`, `[lib] name` and `path`, `[[bin]] name` and `path`, and
  the keys of `[dependencies]` and `[target.*.dependencies]`, resolving a `package = "…"` rename to
  the package it names. A dependency is workspace-internal when it names another package under
  the source. Binaries are the `[[bin]]` tables, plus `src/main.rs` and `src/bin/*.rs` when
  `autobins` is not `false`, as Cargo discovers them. `cargo metadata` would read the working tree
  and not the commit (ADR-0057 d6), may resolve the registry over the network, needs `cargo` on
  the client, and runs nothing the adapter needs: no feature resolution, no versions. A
  `description.workspace = true` is read from the workspace manifest when it is under the source,
  and otherwise reported `unresolved` with the crate claim written without a description.

- **d3 — Module docs are read by a prefix lexer in the CLI, with no new dependency; syn is a test
  oracle only.** The lexer reads from the file's start: an optional byte-order mark and `#!`
  shebang line, then any run of whitespace, `//` line comments, nested `/* */` block comments and
  `#![…]` inner attributes (brackets matched, string literals skipped), collecting each `//!`
  line and `/*! */` block in order, and stops at the first other token. That is the whole of the
  doc's possible place: rustc refuses an inner doc comment after an item
  ([E0753](https://doc.rust-lang.org/error_codes/E0753.html)), so no doc is missed and no code is
  read. It fails closed on what it does not read: a `#![doc = …]` attribute in the prefix, or a
  `#[path = …]` attribute on a `mod` declaration in a crate, reports the file or crate
  `unsupported` with that reason, and its claims are neither written nor retired (d10).
    - **Build cost.** The lexer is a few hundred lines in `oml-cli` and adds no crate, so the
      Linux musl, macOS and Windows CLI targets of the v0.3.1 release split build as they do now.
      syn appears in the lock (2.0.119 and 3.0.9) only under proc-macro crates such as
      `clap_derive` and `serde_derive`, which Cargo builds for the host, so the shipped `oml`
      binary links no syn today. As a runtime dependency, syn with its `full` and `parsing`
      features (2.0.119's source is 50,120 lines in the local registry) and `proc-macro2` would
      also be compiled for each release target; the added build time and binary size were not
      measured, since this lane runs no build.
    - **Correctness.** syn parses the whole file and refuses one whose syntax it does not know,
      which would make a module unreadable for a change below its doc; the lexer reads only the
      prefix the grammar fixes. syn would give the module's `pub` items, which d8's optional
      hasher needs; that is the revisit trigger.
    - **The oracle.** syn joins `oml-cli`'s dev-dependencies, which never link into the shipped
      binary, and a test reads every source file of the repository with both and asserts the same
      docs (d14).

- **d4 — The adapter port gains a set extraction: an adapter may be a pure function of every
  `(path, bytes)` under the source.** *Amends ADR-0057 d7.* A crate claim names its dependents,
  which live in other manifests, and a module's path needs its crate's name and its parent's
  declaration. The port adds `extract_set(files: &[(path, bytes)]) -> Extraction`, whose default
  calls `extract` per file in path order, so `adr`, `headings` and `lines` are unchanged. `rust`
  implements `extract_set` alone. Purity holds over the set: the same files, in any order, yield
  byte-identical items in path then document order, and no adapter reads the clock, the network,
  the environment or a file outside the set. The run already reads each file under the source at
  `--rev` before extracting (ADR-0057 d6), so the cost is the set held in memory, 64,969 lines for
  OML's `crates/`.

- **d5 — A module claim: key `rust/mod/<module path>`, anchored to the doc block's line span,
  hashed on the doc text alone.**
    - **Module path.** The crate's library name (`[lib] name`, else the package name with `-` as
      `_`), then the path under `src/` without `.rs` and with a trailing `mod.rs` dropped, joined
      by `::`: `crates/oml-domain/src/guardrail/mod.rs` is `oml_domain::guardrail`. A module
      reached only from a binary's root takes the binary's name in place of the library's.
    - **Key.** `rust/mod/` and the module path, which ADR-0050 d2's grammar admits (`:` is in it),
      fitted to 128 characters as ADR-0057 d7 fits any key.
    - **Anchor.** `{repo, rev, path: <the .rs file>, fragment: L<n>-L<m>, hash, hasher:
      oml-rustdoc/1}`; the fragment is the doc block's first and last line, a line span ADR-0057
      d21 already admits. A `mod=` fragment was weighed and not taken: it would be a fourth
      fragment form for the server's grammar to name, and the span says exactly which bytes the
      hash covers.
    - **Hash.** `oml-rustdoc/1` (d7) of the doc text, so an edit anywhere below the doc leaves the
      hash, and the claim, as they were.
    - **Text.** `Module <module path> of crate <package>.`, a blank line, then the doc as written,
      each line's `//!` and one following space removed. The module path is in the text because
      recall reads text, and "what does `oml_domain::access` do" should find it; no file path is
      in the text (ADR-0057 d7). Markdown is kept as written, intra-doc links included. When
      ADR-0056 is built the claim's format is `text/markdown`; until then claims carry no format
      and read as `text/plain`, as every claim does today. *Amends ADR-0056 d11 on its branch:* a
      `.rs` file's extension maps a whole-file snippet to `text/vnd.oml.code; lang=rust`, but a
      doc block extracted from it is prose.
    - **Root files.** A crate root's doc (`lib.rs`, or `main.rs` for a crate with no library)
      goes into the crate claim (d6), not a module claim of its own, so it is stated once.
    - **Renames cost two reviews.** A renamed or moved module changes its path, so its key and
      its text: the run reads `removed` plus `new`, never ADR-0057 d18's `moved`. A line added
      above the doc (a licence header) keeps the text and hash and moves the span: that is
      `moved`, recorded by a source check, with no review.

- **d6 — A crate claim: one claim, key `rust/crate/<package>`, anchored to its manifest, stating
  purpose, binaries, dependencies and dependents.** *Amends ADR-0047 d1 and d9.* Its text:

    ```text
    Crate <package>: <description>

    <the library root's //! doc, or the binary root's when there is no library>

    Binaries: <name> — <the first paragraph of that binary root's //! doc>; …
    Depends on (in this workspace): <package>, <package>.
    Depended on by (in this workspace): <package>, <package>.
    ```

    A line with nothing to list is left out; `Depends on` and `Depended on by` are sorted by name.
    Dependencies are the workspace-internal normal dependencies (d2); dev- and build-dependencies
    are tests and tooling and are left out, and so are external dependency names, which changed
    the claim in 17 of 29 manifest edits against 6 without them. Dependents are the packages under
    the source that name this one, so `oml sync crates` lists `oml-app`'s five dependents in
    `crates/` and not `oml-port-contracts`. The anchor is `{repo, rev, path: <its Cargo.toml>,
    hash, hasher: oml-crate/1}`, with no fragment: the claim is about the package, and the
    manifest is where the package is declared. ADR-0047 d9 writes a skeleton entry as two entries,
    the mechanical part and a purpose line a person or agent writes; here the purpose is extracted
    from the description and root doc already reviewed in their own pull requests, so the
    operator's one claim per crate holds the whole entry, and d12 says what that costs an
    automated reviewer.

- **d7 — Two hashers join the closed set: `oml-rustdoc/1` for a doc block and `oml-crate/1` for a
  crate's facts.** *Amends ADR-0057 d3.* `oml-rustdoc/1`: take the doc block's lines in order,
  remove each `//!` and one following space, or a `/*!` block's markers; then `oml-md/1` (NFC, line
  endings to LF, every run of whitespace to one space, trimmed, SHA-256). So a reflowed doc keeps
  its hash and a changed word does not. `oml-crate/1`: SHA-256 over the UTF-8 of one line per
  fact, in this order: `name`, `description` normalised as `oml-md/1` normalises, the root doc as
  `oml-rustdoc/1` normalises it, each binary as `bin <name> <its first paragraph, normalised>`,
  each internal dependency as `dep <name>`, each dependent as `rdep <name>`, each list sorted. A
  version bump, a feature change, an external dependency or a code edit changes no input, so it
  changes no hash. A crate claim's hash covers facts read from other manifests (its dependents):
  the hasher, not the anchored file, names what it covers. The server checks only that `hasher`
  is in the set and computes nothing from it (ADR-0057 d2), so the server's change is two enum
  values.

- **d8 — A new or removed `pub` item does not make a module claim stale; a hasher that would is
  designed, opt-in and not built in v0.5.9.** Over the 817 file changes, a doc-only hash asks for
  65 reviews; a hash that also covers module-level `pub` item names asks for 208, 3.2 times as
  many, and 143 of those would show the reviewer an unchanged doc. A module doc states purpose and
  structure, which a new function rarely contradicts, and the operator's scope is the doc. The
  cost is a doc that lists the module's entry points and goes out of date without a mark.
  **The opt-in, designed:** `oml sync … --adapter rust --hasher oml-rustdoc+pub/1`, whose hash
  covers the `oml-rustdoc/1` input, then a line `--`, then the sorted, distinct `<kind> <name>` of
  each module-level `pub` item (`fn`, `struct`, `enum`, `trait`, `type`, `const`, `static`, `mod`,
  `macro`). It is a hasher and not a second anchor field because the anchor's grammar is closed and
  holds one hash (ADR-0057 d3), and a second field would ask every surface to carry, show and
  compare it for one adapter. A workarea keeps one hasher: switching makes every claim `changed`,
  one review each, so it is chosen at the first load. It needs a lexer that walks item headers
  with brace matching, or syn (d3); both wait for the operator to ask, tracked as FR-131.

- **d9 — Dependencies are claim text in v0.5.9, not `depends_on` claim edges.** The text already
  answers "what does this crate depend on" for recall and for a reader with no checkout. An edge
  would add a proposal and a ratification per edge at the load (12 for `crates/`), and again for
  each edge whenever either endpoint is superseded (ADR-0057 d10), and crate claims are
  superseded whenever a root doc changes. No documented use case traverses a dependency graph
  through OML; when one does, the adapter's edge statements carry `depends_on` and d10's
  `--edges` proposes them unchanged, tracked as FR-132.

- **d10 — The `rust` adapter retires only claims in its own namespace, and never a claim whose
  file it could not read.** *Amends ADR-0057 d8, and closes the PR #68 review's H3 and M1 for this
  adapter.* An adapter declares its key namespace (`adr-`, `doc/`, `rust/`). In the diff, a listed
  claim whose key is outside the running adapter's namespace is never `removed`, whatever its
  anchor's path, so a `rust` run over `.` in a workarea that also holds `docs/adr` claims leaves
  them alone, and so does a run whose adapter changed. A claim whose anchored file, or whose
  crate's manifest, was reported `unreadable`, `unsupported` or `undeclared` at this `rev` is
  reported with that state and not retired. A deleted file or a deleted crate is `removed` as
  before.

- **d11 — The operator's four targets load into three workareas under one parent; the layout is
  the operator's decision.** Two layouts, each a child of the user root `e8kor` (ADR-0044):

    | Layout | Workareas | For | Against |
    |---|---|---|---|
    | **A. Three siblings** | `oml-design`, `oml-docs`, `oml-code` | ADR-0057 d14 already names `oml-design` there; no new name. | An agent that wants the project's decisions, docs and code knowledge reads three scopes, and ADR-0048 S4's multi-workarea scope, capped at four, is not built. |
    | **B. One parent, three children** (recommended) | `<parent>` with `oml-design`, `oml-docs`, `oml-code` beneath it | A read at the parent covers its subtree, so one scope reaches all three. Each sync still diffs its own workarea and not its children (ADR-0057 d8), so the three never see each other's claims, and ADR-0057 d14's `oml-design-pr-<n>` children stay under `oml-design`. | One more workarea name, kept for good (ADR-0026 d10). |

    **Recommended: B,** with the parent created before v0.5.8's first ADR load, since
    `POST /v2/workareas/{workarea}/reparent` is designed and not built
    ([HTTP API](../api/http-api.md)), so an `oml-design` created under `e8kor` cannot later be
    moved. The parent's name is the operator's: `oml` is tenant-unique and may already name the
    crew workarea `root.crew.oml` on the live installation, so `oml-lab` or `oml-project` would
    serve. Each target's sync key is a writer key on its own workarea, as ADR-0057 d14 issues
    `e8kor-design-sync` for `oml-design`: `e8kor-docs-sync` and `e8kor-code-sync`, since a writer
    supersedes only its own tips.

- **d12 — A person reviews every crate and module claim in v0.5.9; an automated reviewer, once
  built, may approve a crate claim only by re-deriving all of it, and never a module claim.**
  *Amends ADR-0047 d9.* ADR-0047 d8's CI key is not built, so in v0.5.9 the operator's reviewer
  key approves each run's batch through `oml review list` and `oml review approve --ids -`
  (ADR-0055), each verdict marked `same_user` (ADR-0055 d7), since the sync key and the reviewer
  key act for one person in a single-person tenant. That mark says what the approval is worth to
  an auditor and relaxes nothing (R2 still refuses a key reviewing its own write).
    - **Crate claims qualify for re-derivation.** Every byte of a crate claim is rendered from
      committed files at the anchored `rev`: the manifest's description, the root doc and the
      dependency lists. When ADR-0047 d8's key exists, it may approve a crate claim when its own
      run of the same adapter at that `rev` renders the same text and hash, and reject with
      `mismatch` otherwise. Re-derivation proves the claim is a faithful extraction, not that the
      description is true; the description and the root doc were reviewed in their own pull
      requests, which is the same standard ADR-0057 d12 applies to a document sync. Because one
      claim holds both parts (d6), the key approves all of it or none of it.
    - **Module claims never do.** A module doc is prose that a person or an agent wrote, and
      ADR-0047 d9 keeps model-written prose from an automated approval: a reviewer of the writer's
      model family shares its errors. A person reads them, as they read a document sync's
      sections.

- **d13 — Volume and cost: one run of 92 claims at the load, and about one review for every four
  commits that touch `crates/` afterwards.**
    - **The load.** `oml sync crates --workarea oml-code --adapter rust` at `ad2daa3` plans 92
      `new` (9 crates, 83 modules) and states no edges, one run under ADR-0057 d9's cap of 200 and
      one bulk verdict request. The text is about 51,000 characters: 40,969 of module docs, 9,894
      of root docs, and the nine descriptions. 92 `POST /v1/memories` requests, each honouring
      `Retry-After`.
    - **Afterwards.** Over the 279 commits measured, 65 doc changes and 6 crate-fact changes would
      each have been one supersession to review, about one for every four commits; a change to
      internal dependencies supersedes two crate claims, the dependent's and the dependency's.
      Every `rust` sync also records one source check per anchored tip (ADR-0057 d5), 92 a run.
    - **How long the review takes** is unmeasured; FR-116 records it for the ADR load, and FR-130
      adds the code load.

    The dry run at `ad2daa3`, as it would print (lines elided):

    ```text
    $ oml sync crates --workarea oml-code --adapter rust --dry-run
    source    github.com/e8kor/open-memory-lab @ ad2daa3e8dac  crates  adapter rust  hashers oml-crate/1, oml-rustdoc/1
    workarea  oml-code  key e8kor-code-sync
    new       rust/crate/oml-app                   crates/oml-app/Cargo.toml
    new       rust/mod/oml_app::ports              crates/oml-app/src/ports.rs#L1-L6
    …
    new       rust/mod/oml_domain::guardrail       crates/oml-domain/src/guardrail/mod.rs#L1-L5
    …
    plan      92 new (9 crates, 83 modules), 0 changed, 0 removed, 0 unchanged, 0 pending; this run writes 92
    skipped   0 unsupported, 0 undeclared, 0 generated
    ```

    The two spans shown are the doc blocks at `ad2daa3`, read by the same script.

- **d14 — The build, each step closed by named tests.**
    1. **Lexer:** `the_lexer_reads_the_same_inner_docs_as_syn_on_every_source_file` (syn as a
       dev-dependency, over the repository's own files),
       `the_lexer_stops_at_the_first_item_and_reads_no_code`,
       `nested_block_comments_and_inner_attributes_before_the_doc_are_skipped`,
       `a_doc_attribute_or_a_path_attribute_is_reported_unsupported`.
    2. **Extraction:** `extract_set_is_a_pure_function_of_the_file_set_in_any_order`,
       `the_rust_adapter_at_ad2daa3_yields_9_crates_and_83_modules` (golden, against a fixture of
       that commit's manifests and each source file's doc block and `mod` declarations),
       `test_modules_examples_benches_and_build_scripts_are_never_read`,
       `an_undeclared_file_is_reported_and_not_extracted`,
       `no_line_outside_a_doc_block_reaches_any_item_text`,
       `a_crate_claim_lists_internal_dependencies_and_dependents_and_no_external_names`,
       `a_module_path_follows_the_lib_name_and_drops_mod_rs`.
    3. **Staleness:** `a_code_edit_below_the_doc_leaves_the_module_claim_unchanged`,
       `adding_a_pub_fn_leaves_the_module_claim_unchanged`,
       `a_reflowed_module_doc_keeps_its_hash`, `a_changed_word_in_a_module_doc_is_changed`,
       `a_version_bump_or_an_external_dependency_leaves_the_crate_claim_unchanged`,
       `a_new_internal_dependency_changes_both_crate_claims`,
       `a_line_added_above_the_doc_is_moved_and_writes_nothing`,
       `a_renamed_module_is_removed_and_new`.
    4. **Retirement:** `the_rust_adapter_never_retires_a_claim_outside_its_namespace`,
       `an_unsupported_or_unreadable_file_retires_none_of_its_claims`.
    5. **Server:** `the_rust_hashers_are_accepted_and_an_unknown_hasher_is_request_validation`.

    And against the lead, in `cli_e2e`, `oml_sync_loads_a_crates_docs_and_a_code_edit_writes_nothing`.

- **d15 — Every new failure has a doctor line.** `oml doctor` names: a server that does not accept
  the hashers `oml-rustdoc/1` and `oml-crate/1` (before v0.5.9), which `oml sync --adapter rust`
  also refuses before reading anything; for the key's default workarea, the count of claims in
  the `rust/` namespace whose last run reported them `unsupported`, `unreadable` or `undeclared`;
  and a workarea that holds claims of more than one adapter's namespace, which d10 makes safe and
  which d11's layout avoids.

## The operator's sync targets by release

| Target | Adapter | Workarea (d11, B) | Ships in | Notes |
|---|---|---|---|---|
| Decision records, `docs/adr` | `adr` | `oml-design` | v0.5.8 | ADR-0057 d14's load. |
| Documents under `docs/` | `headings` | `oml-docs` | v0.5.8 | 121 files under `docs/` at `ad2daa3`, 49 of them in `docs/adr`. A `headings` run over `docs` would load the decision records a second time as sections, so the target is the 72 files outside `docs/adr`, about 1,005 sections with body text by a script's count, six runs of 200. v0.5.8 has no way to leave a subdirectory out of a source: the operator either runs one sync per subdirectory and per top-level file (17 sources), or v0.5.8 gains `--exclude <path>` (question 2). |
| Crate claims | `rust` | `oml-code` | v0.5.9 | This record, d6. |
| Module claims | `rust` | `oml-code` | v0.5.9 | This record, d5. |
| Public item docs (`///`) | `rust` | `oml-code` | later | The operator's later scope; at least 517 documented `pub` items by the operator's count. |

## Questions for the operator

1. **The workarea layout (d11).** B with a parent, or A with three siblings; and the parent's
   name. Needed before v0.5.8's first ADR load creates `oml-design`.
2. **The `docs` target's overlap with `docs/adr`.** `--exclude <path>` added to v0.5.8's
   `oml sync` (one repeatable flag, its use case UC-SM-04), or seventeen per-directory sources
   with no change to v0.5.8.
3. **One crate claim against ADR-0047 d9's two entries (d6, d12).** Recommended as decided: one
   claim, as the operator scoped it, at the price that an automated reviewer re-derives it whole.
   The alternative splits a mechanical claim (binaries and dependencies) from a purpose claim,
   letting a CI key approve the first alone, at nine more claims.

## Consequences

- **Costs.** Two hasher values on the server and nothing else server-side; a few hundred lines of
  lexer and adapter in `oml-cli`, no new runtime dependency, syn as a dev-dependency; the set of
  source files held in memory during a run. Review: 92 claims at the load, about one per four
  commits that touch `crates/` afterwards, two for an internal-dependency change and two for a
  renamed module. A doc that lists a module's entry points can go out of date without a mark
  (d8).
- **Roadmap rows.** FR-130 (the code load's review time and rejections), FR-131 (staleness by
  `pub` surface: the opt-in hasher and the lexer or syn it needs), FR-132 (dependency edges, on
  the first use case that traverses them), FR-133 (the syn and lexer build costs, measured on the
  three CLI targets if the opt-in hasher reopens the parser choice).
- **Use cases.** UC-SM-06, load a code base's crate and module docs and keep them in step, added
  to the [use cases](../product/use-cases.md#sources) and the
  [call map](../engineering/use-case-call-map.md#sources): its call is `oml sync --adapter rust`,
  over the same routes as UC-SM-04.
- **Pages to change at the build.** The CLI reference (`--adapter rust`, the new states
  `unsupported` and `undeclared`); ADR-0057's hasher set in openapi.yaml's `Anchor`; the plugin's
  skill, which can tell an agent that `rust/mod/<path>` keys hold module docs.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Raw code chunks through the `lines` adapter** | At most 40 lines a block, so at least 1,625 claims for the 64,969 lines of `crates/*/src`, each a review; every added line moves the spans below it; and it stores the per-file code map ADR-0047 d1 rules out, with claims that are code, not facts a reviewer can judge. |
| **Embedding the code** for recall | Stores derivable code and its vectors, which ADR-0047 d1 rules out; a vector is not a claim a person can review, so R2 has nothing to check; every edit re-embeds; and the server's scan would read every line of a private code base. |
| **LSP-based extraction** (rust-analyzer's `documentSymbol`, as ADR-0047 d4 allows for spans) | Needs rust-analyzer on the client; reads the working tree, not the commit; loads the workspace, which runs `cargo metadata` and build scripts, so a sync would run the repository's code; slow to start on 145,928 lines; and its answer for a module's doc is what the lexer reads. |
| **syn in the shipped binary** | Compiled for every release target, refuses a file with syntax it does not know although its doc is fine, and reads every line of code to find a prefix the grammar fixes (d3). Kept as the test oracle. |
| **`cargo metadata`** | Reads the working tree, may reach the network, needs `cargo` on the client (d2). |
| **A `mod=<path>` fragment** | A fourth fragment form for the server to name, and it does not say which bytes the hash covers; the line span does (d5). |
| **A second anchor field for the `pub` surface** | The anchor holds one hash by a closed grammar; a field every surface carries, for one adapter, against a hasher that a workarea opts into (d8). |
| **Stale on `pub` surface by default** | 208 reviews against 65 over the measured history, 143 of them showing an unchanged doc (d8). |
| **External dependency names in the crate claim** | 17 crate-claim changes in 29 manifest edits against 6; they are derivable from the manifest, and ADR-0047 d1's skeleton names dependencies between components. |
| **`depends_on` claim edges at the load** | A proposal and a ratification per edge, again on every supersession of either end, for a fact the text states and no use case traverses (d9). |
| **A module claim for each crate root** | States the root doc twice, once in the crate claim; a change would cost two reviews. |
| **Code into the same workarea as the documents** | Works only with d10's namespace guard, and every sync's listing then reads the others' claims; three workareas under one parent cost one name (d11). |
