<!--
id: adr/ADR-0065-the-scan-is-a-versioned-compiled-in-rule-set-that-vetoes-leaked-keys-and-holds-what-a-reviewer-should-judge
tier: engineering
status: accepted design · 2026-11-06
answers: what replaces the marker scanner; how the rule set is named, versioned and journalled; which secret, path, injection, Unicode, encoding and personal-data rules it carries and what each does; where gitleaks' patterns come from and under which licence; which fields of a submission are scanned and which are not; what a finding says and what it never says; the time budget and the size bound; what happens to the PoC markers; the findings metric and the doctor line; how false positives are measured and on what; the rejected roads and the open questions
-->

# ADR-0065 — The scan is a versioned, compiled-in rule set that vetoes leaked keys and holds what a reviewer should judge

**Status:** accepted design · 2026-11-06 (was proposed) · introduced in v0.6.0 · first recorded 2026-11-05 · amended 2026-11-10 (d1, d5, d13, d14, d15, as ADR-0074 d28 shapes them: the rule set becomes `oml-scan/1.1`; every rule reads the normalised view; a tag sequence passes only whole and RGI; a version records the rule set it was scanned under, and inside an in-progress scope another author's held version is served only when the server's rule set scanned it and found nothing; `oml admin rescan --record-held` records held versions) · amended by [ADR-0074](ADR-0074-the-scan-names-a-threat-class-for-every-finding-refuses-only-what-can-never-be-safe-holds-or-annotates-the-rest-and-recall-serves-every-hit-as-data-with-its-findings.md) d6 on 2026-11-12 (d14: the letters-only view reads a phrase only at word edges; the rule set becomes `oml-scan/1.2`)

**Amends:** ADR-0019 (d4: the scanner port gains `version()`, the plugin's identity on the events
it informs, as d4 already asks); ADR-0057 (d2, d6, d18: the secret-shaped path list becomes one
list, kept by `oml-scan`, that the anchor scan and `oml sync` both apply); ADR-0006 (d6: a second
leaf crate, `oml-scan`, beside `oml-guardrail`, linked by `oml-domain` and `oml-cli`)

## Context

The server's scanner is still the proof-of-concept: `MarkerScanner` in the lead matches two
literal markers, `[oml:veto]` and `[oml:hold]`, and nothing else. The design has assumed a real
one for a while. [Admission](../architecture/admission.md) names the `secret.*` finding classes
and a versioned rule-set pin; ADR-0057 d2 scans every anchor field because a written path is
never erased; ADR-0063 (on `feat/chat-memory`, #78) lets a chat excerpt ride a body and leans on
the scanner to refuse a pasted secret; ADR-0056 (on `design/claim-formats`, #63) scans decoded
JSON and YAML values beside the source. From v0.6.0, chat capture has agents write facts straight
out of conversations, which is where a pasted token or an instruction aimed at the next reader
turns up. OML goes into production with that release, so the scan has to catch what it claims
to catch.

The constraints are fixed elsewhere and not restated: the scan is rules, never a model, and runs
in the same call before the commit transaction ([admission](../architecture/admission.md));
a veto refuses and a hold is held for review with a reason, and nothing is silently dropped
([ADR-0001](ADR-0001-the-constitution.md) R4); a scanner can veto or hold, never promote, and is
compiled in ([ADR-0019](ADR-0019-hexagonal-architecture-and-plugins.md) d4); everything is pure
Rust ([ADR-0006](ADR-0006-pure-rust.md)); the scanner reads content as data and never follows it
([ADR-0004](ADR-0004-bodies-and-attachments.md)).

## Decision

- **d1 — The rule set is `oml-scan/1`, compiled in, versioned, and journalled on every verdict.**
  The rules live in a leaf crate, `oml-scan`, that links no workspace crate. The lead composes it
  behind the scanner port as `RuleScanner`. The port gains `version()`, and its value is the
  `scan.scanner_version` of every submission event, which was `null` under the PoC; the test
  doubles still return `None`. Any change to a rule's pattern, id, class or action, or a new rule,
  is a new version (`oml-scan/2`). A verdict taken under `/1` stays valid against `/1`, and
  nothing is re-scanned retroactively ([admission](../architecture/admission.md), the rule-set
  registry). Same text, same findings: the set holds no state but its counters (d11).
  Amended 2026-11-10: the changes to d5 and d14 below are `oml-scan/1.1`, a fix's minor
  ([ADR-0074](ADR-0074-the-scan-names-a-threat-class-for-every-finding-refuses-only-what-can-never-be-safe-holds-or-annotates-the-rest-and-recall-serves-every-hit-as-data-with-its-findings.md)
  d21, d28), which `version()`, `/readyz` and every submission event from then on report. Each
  stored version records the rule set it was scanned under, `memories.scanned_under` (migration
  0038): admission's, or the recording rescan's (d13). The migration reads every row stored
  before it as `oml-scan/1`.

- **d2 — Each rule names a finding class, a rule id and one action.** Classes keep admission's
  `family.condition` vocabulary, so a review policy's `human_required_classes` can name them. The
  `oml-scan/1` registry:

    | Class | Rules | Action |
    |---|---|---|
    | `secret.private_key` | `private-key-block`, `putty-private-key` | veto |
    | `secret.api_token` | `aws-access-key-id`, `github-token`, `github-fine-grained-pat`, `gitlab-token`, `slack-token`, `slack-app-token`, `slack-webhook`, `stripe-key`, `google-api-key`, `openai-key`, `anthropic-key`, `npm-token`, `pypi-token`, `huggingface-token`, `jwt`, `oml-key`, `oml-connection-token`, `authorization-header` | veto |
    | `secret.credential_pair` | `url-credentials`, `curl-user` | veto |
    | `secret.assignment` | `aws-secret-access-key`, `aws-secret-near-id`, `keyword-assignment` | veto |
    | `anchor.secret_path` | the path rule (d4) | veto |
    | `format.scan_bound` | `scan-bound` (d9) | veto |
    | `test.marker` | `marker-veto` (veto), `marker-hold` (hold) (d10) | as named |
    | `injection.override` | `ignore-instructions`, `new-instructions` | hold |
    | `injection.role` | `chat-template`, `system-prompt-tag`, `persona-switch` | hold |
    | `injection.tool_call` | `reader-trigger`, `covert-action`, `tool-call-markup` | hold |
    | `unicode.bidi` | `bidi-control` | hold |
    | `unicode.invisible` | `invisible-format`, `tag-characters` | hold |
    | `encoding.base64_blob` | `base64-blob` | hold |
    | `pii.email`, `pii.phone` | `email-address`, `phone-number` | off; hold when on (d6) |

    `secret.assignment`, `injection.*`, `unicode.*`, `encoding.*`, `pii.*`, `format.scan_bound`
    and `test.marker` are new rows in admission's registry. `anchor.secret_path` was already the
    finding ADR-0057 d2 returned; it is registered here.

- **d3 — A secret is vetoed. Provider tokens match by prefix and shape; a key-value secret needs
  an entropy floor.** Provider tokens: AWS access key ids (`AKIA…`, `ASIA…`), GitHub (`ghp_`,
  `gho_`, `ghu_`, `ghs_`, `ghr_`, `github_pat_`), GitLab (`glpat-`, `glptt-`, `glrt-`, `gldt-`),
  Slack (`xox?-`, `xapp-`, webhook URLs), Stripe (`sk_live_`, `rk_test_`, …), Google API keys
  (`AIza…`), OpenAI (`sk-proj-`, `sk-svcacct-`, `sk-admin-`, the `T3BlbkFJ` shape and the
  legacy `sk-` plus 48 characters),
  Anthropic (`sk-ant-api03-`, `sk-ant-admin01-`, …), npm, PyPI and Hugging Face tokens, JWTs
  (three base64url parts, the first two opening with `eyJ`), private-key blocks (`-----BEGIN …
  PRIVATE KEY-----`, the header alone, and PuTTY key files), and OML's own credentials:
  `oml_` followed by 43 base62 characters, and `oml_at_` and `oml_rt_` likewise
  ([roles and keys](../architecture/roles-and-keys.md); ADR-0031 d10). The provider patterns are
  adapted from the rule regexes in gitleaks' default configuration (`config/gitleaks.toml`,
  github.com/gitleaks/gitleaks), MIT licence, Copyright (c) 2019 Zachary Rice. Only the regexes
  were adapted, rewritten for Rust's `regex`, which has no look-around. No gitleaks code is used,
  and the attribution is repeated in `crates/oml-scan/src/rules.rs`. The full MIT notice is
  `LICENSES/gitleaks-MIT.txt`, which every release archive and the image carry. A provider token
  needs no word boundary before it, so a token glued to a word (`KEY_ghp_…`, `xoml_…`, a claim
  key that runs into one) is still found; an AWS key id needs a non-alphanumeric character or
  the start of the text before it.

    Five rules need a second look before a match counts as a finding:
    - `keyword-assignment`: a key whose last segments name `password`, `passwd`, `pwd`,
      `secret`, `token`, `api_key`, `access_key`, `private_key`, `client_secret` or
      `credential`, where the keyword ends the key or is followed only by `_`, `.` or `-`
      segments (`DB_PASSWORD`, `client.secret`, `token_value`, never `tokenizer`,
      `tokens_per_second` or `secretKeyRef`), then `=`, `:`, `:=` or `=>`, then a value of 12 to
      256 characters. The word `is` is not an operator, so "the token is base64url-encoded"
      passes. The value counts only if it has a letter and a digit, is not an identifier a person
      wrote (two or more words of three or more letters in one case between `-`, `_`, `.` or `/`,
      as in `PROJ-12345-feature-branch` or `postgres-credentials-2024`), reaches a Shannon entropy of 3.0 bits per character (under 20
      characters) or 3.5 (from 20), and is not a stand-in. Stand-ins are `${…}`, `<…>`, `%…%`,
      `*…`, values containing `example`, `your`, `changeme`, `redacted`, `xxx` and the like, a
      digest that names its algorithm (`sha256:…`), a path (`/…`, `~/…`, `./…`, `C:\…`), a URL,
      and a file name (`….json`, `….pem`, …). A random 12-character alphanumeric value scores
      about 3.4; an English word or a `snake_case` name lacks the digit. `aws-secret-access-key`
      applies the same test to a 40-character value under an `aws_secret_access_key` key.
    - `url-credentials`: `scheme://user:password@host`, where the password has at least four
      characters, differs from the user, and is not a stand-in. The compose file's
      `postgres://oml:oml@…` passes; a real password does not.
    - `curl-user`: `-u user:password`, as curl takes it, with the password test above.
    - `authorization-header`: `Authorization: Bearer|Basic|Token …`, the header name quoted or
      not (`{"Authorization": "Bearer …"}`), with the entropy test above.
    - `aws-secret-near-id`: where an AWS key id fires, a 40-character base64 value within the
      next 200 bytes, with the entropy test: the secret half of a pair pasted with no key name.
      It is counted by hand after the id, not by a pattern.

- **d4 — Secret-shaped paths are one list, applied by the core before the scanner port.** A path
  component named `.env*`, ending in `.pem`, `.key`, `.p12`, `.pfx`, `.jks`, `.keystore`,
  `.ppk` or `.kdbx`, an SSH identity (`id_rsa`, `id_dsa`, `id_ecdsa`, `id_ed25519`, their `_sk`
  variants and `.pub` halves), containing `credentials` or `secret`, equal to `.npmrc`,
  `.pypirc`, `.netrc`, `_netrc`, `.pgpass`, `.htpasswd`, `.git-credentials`, `.dockercfg`,
  `.vault-token` or `kubeconfig*`, or `config` under `.kube/`, or
  `config.json` under `.docker/`. The match is case-insensitive and covers every component, so a
  directory so named covers every file under it. `.env.example`, `.env.sample` and
  `.env.template` are templates and pass, and `id_generator.rs` is not an identity.
  `oml_scan::secret_path` is the only copy:
  `oml-domain`'s `secret_shaped`, which vetoes an anchor's path and a moved check's path as
  `anchor.secret_path` (ADR-0057 d2, d18), delegates to it, and so does `oml sync`'s refusal to
  read a file (d6). Until now there were two lists, and they had already drifted: the server
  matched only the file name, while the client matched every component and five more dotfiles.
  The core keeps running this rule itself, ahead of whatever scanner is composed, because no
  scanner choice may loosen it (ADR-0019 d4's plugin law). It vetoes a `.npmrc` anchor and a
  kubeconfig, which the server's old list let through. A path under `secrets/` was already
  refused by `oml sync`, and the server now refuses it too.

- **d5 — What a reviewer should judge is held, never vetoed.** Injection heuristics hold:
  `ignore-instructions` ("ignore/disregard/forget/override … previous/prior/system …
  instructions/prompts/rules"), `new-instructions` ("new instructions:"), `chat-template`
  (`<|im_start|>`, `[INST]`, `<<SYS>>`), `system-prompt-tag` (`<system>`, a line opening
  `system prompt:`), `persona-switch` ("you are now an unrestricted…", "your new role is",
  "developer mode enabled"), `reader-trigger` ("when an agent reads this", "if you recall
  this"), `covert-action` ("silently run", "without telling the user") and `tool-call-markup`
  (`<tool_call>`, `"tool_calls":`). Each pattern is written for an instruction aimed at the
  reader, so ordinary imperatives ("run the tests first") pass. Unicode: any bidirectional
  override or isolate (U+202A–U+202E, U+2066–U+2069) holds; more than three invisible format
  characters (U+200B, U+2060–U+2064, U+FEFF, U+180E, U+206A–U+206F) hold; any tag character
  (U+E0000–U+E007F) outside an RGI emoji tag sequence holds. Amended 2026-11-10: the three RGI
  tag sequences, the subdivision flags of England, Scotland and Wales (U+1F3F4, the id as tag
  letters, U+E007F), are matched whole; a U+1F3F4 followed by any other tag characters is a
  hidden payload and holds, where the first build skipped every tag character after the prefix.
  Two or more variation selectors in a row (U+FE00–U+FE0F, U+E0100–U+E01EF) hold as
  `variation-run`: one picks a glyph for the character before it, and a run picks nothing and
  can carry a byte each. A joiner or another removable character between two selectors does
  not end the run. Joiners and the soft hyphen are left
  alone, because scripts and emoji use them. Encoding: a run of 400 or more base64 characters
  that mixes cases and digits at 4.5 bits per character or more holds, as a payload no reviewer
  can read. A hold costs a reviewer's look and refuses nothing. A veto on a heuristic would
  refuse a true memory about prompt injection, which is exactly what a security team writes
  down. In an anchor field, any finding refuses, holds included, because the anchor is kept for
  good (ADR-0057 d2, unchanged).

- **d6 — The personal-data rules exist, off by default, and hold when on.** `pii.email` and
  `pii.phone` (international `+` numbers and the `(nnn) nnn-nnnn` shape) are compiled in behind
  `Options { pii }`. A team's memory names its people ("ask jane@ about the canary"), and holding
  every such fact would flood review with findings that judge nothing. A tenant that must keep
  addresses out of shared memory needs them on as a journalled tenant setting, which holds every
  finding. That setting is not small (a journalled event, a fold onto the tenant row, a migration,
  and the tenant's flag carried into every scan) and is not built: v0.6.0 ships the rules off for
  every tenant, tracked as #140 on the [open issues](https://github.com/e8kor/open-memory-lab/issues?q=label%3Aroadmap). `/readyz` and doctor
  say they are off.

- **d7 — The scan reads every free-text field a submission keeps, and only those.**
    - **The body** of every `remember`, `learn` and supersession, after NFC normalization.
    - **The anchor**: `repo`, `path`, `uri`, `rev` and `fragment`, each value scanned
      (ADR-0057 d2), plus the path rule (d4) on `path`.
    - **A moved check's** path and fragment (ADR-0057 d18).
    - **A submission's `claim_key` and a verdict's `reason`**, single or bulk. Both reach the
      journal as plaintext, which is never edited, so a veto finding in either refuses the call
      and journals nothing. A hold finding passes: no review follows a claim key or a reason.
    - **A chat excerpt** (ADR-0063): it is part of the body, so the body scan covers it, with no
      call of its own.
    - **Decoded JSON and YAML values** (ADR-0056 d6, when that lane lands). `RuleSet::scan` takes
      a list of `(field, text)` pairs, so a format adapter's scan views go in as JSON pointers
      beside `/body`, and each finding names the pointer. The port's envelope carries one text
      today; the formats lane widens it.
    - **Not edge labels**: a claim edge's label is a CURIE from a closed vocabulary, validated
      against it ([ontologies](https://github.com/e8kor/open-memory-lab/blob/main/design/ontologies.md)), never free text.
    - **Not yet** a guardrail's `rule` JSON (its strings are globs and host patterns the
      `oml-gr/1` grammar validates) or an attachment's extracted text (no extractor is built).
      Both are Q2.

    The scanner only reads. It matches patterns over text and acts on nothing it reads, so an
    instruction inside a body can be found but cannot be followed (ADR-0004).

- **d8 — A finding names its class, rule and place, never what it matched.** The `Finding` type
  has a class, a rule id, an action, a field and a line, and no field that could hold matched
  text. The reason the port carries is `class: rule at field line n`, for example
  `secret.api_token: github-token at /body line 3`, up to five findings joined by `; ` and
  then `; and n more`. That string is what the client sees in `details.finding` of
  `submission.scan_veto`, what the journal keeps as a held version's `scan.finding`, and what a
  request log line can carry. The metric (d11) carries class, rule and action only. A test
  asserts that no eight-character window of any sample secret appears in a reason or in a
  verdict's debug form.

- **d9 — Each rule runs on its own, and one field is bounded at 1 MiB.** Every pattern is its own
  `regex` (linear time, no backtracking), with the literal prefilter its prefix gives it, and only
  a rule that matches runs again to place its first finding and apply its second look. One
  `RegexSet` over every pattern was tried first and dropped: its combined DFA thrashed its cache
  on dense input and fell back to the slower engine, 5 s on a mebibyte of base64. The base64 rule
  and the AWS pair rule are counted by hand, one pass each, and the personal-data patterns do not
  run while they are off. Word boundaries are ASCII (`(?-u:\b)`): a Unicode boundary makes the
  matcher leave its DFA on any non-ASCII text, and SciFact scanned about four times slower with
  one. Release build on the build host, measured: 3.9 µs per LoCoMo turn (mean 124 bytes),
  30.6 µs per NFCorpus document (1.6 KB), 26.3 µs per SciFact document (1.5 KB) and 18.0 µs
  per LongMemEval turn (988 bytes). A mebibyte, the largest field, takes 13 ms of random base64,
  9 ms of `password:` assignments, 10 ms of `password_aaaa` words and 12 ms of prose with
  emails. A test holds each under 50 ms in a release build, and another holds a typical claim
  under 1 ms. A field over 64 KiB is scanned off the async worker, with `block_in_place`, so a
  long scan never stalls the requests queued behind it. A field over 1 MiB is vetoed as
  `format.scan_bound` rather than scanned in part, because a partial scan would admit unscanned
  bytes. The bound applies after NFC, so a body that NFC grows past it is refused as
  `format.scan_bound`, not `payload_too_large`.

- **d10 — The PoC markers stay, as test markers.** `[oml:veto]` vetoes and `[oml:hold]` holds,
  under class `test.marker`, so suites and demos can force either outcome without writing a
  fake secret. They are documented as test markers. A deployment that wants them gone gets a new
  rule-set version, not a switch.

- **d11 — Findings are counted by rule; `/readyz` and doctor name the rule set.** The lead's rule
  set counts its findings by class, rule and action. The metrics listener serves those counts as
  `oml_scan_findings_total{class, rule, action}`, every label from the closed set d2 lists
  (ADR-0042 d12's rule for labels). `/readyz` carries `scan: {version, rules, pii}`: the rule
  set, how many rules it loaded and whether the personal-data rules are on, with no count, since
  the probe answers anyone. `oml doctor` prints `scan: oml-scan/1, 40 rules loaded, personal-data
  rules off`, warns `scan_ruleset` when the server names no rule set (one before v0.6.0) and
  `scan_rules` when it loaded none, and, where `OML_METRICS_URL` names the metrics listener,
  prints `findings since start: n refused, m held; most: <rule> ×k, …`. The counts are since the
  lead started; a trend over time is the metrics store's to keep.

- **d12 — False positives are measured on OML's own text and on public corpora, and every
  accepted one is listed.** A test scans every page under `docs/` and every recall-eval memory.
  The memories must pass. A documentation page may fire only a listed `(page, rule)` exception:
  the chain vectors' published test key (`private-key-block`), a worked OML key example
  (`oml-key`), a bearer header example (`authorization-header`), a spike's throwaway database
  URL (`url-credentials`), and this record, which names the markers and quotes the injection
  phrases. Measured with `cargo run --release -p oml-scan --example fp_rate`, documents with
  any finding:

    | Corpus | Documents | With a finding | Rate |
    |---|---|---|---|
    | SciFact (BEIR) | 5,183 | 0 | 0 % |
    | NFCorpus (BEIR) | 3,633 | 0 | 0 % |
    | LoCoMo turns | 5,882 | 0 | 0 % |
    | LongMemEval (`_s`, distinct turns) | 189,514 | 40 | 0.021 % |

    In LongMemEval, 10 turns veto and 30 hold. Nine of the vetoes are credential values users
    pasted into the conversation: a GitHub OAuth token quoted in three turns, a Docker access
    token, a JWT, a 149-character token and a hex API key quoted in three turns. The tenth is a
    prose mention of a PEM header with no key under it, which this rule set refuses on purpose.
    The holds are 17 turns that give the assistant overriding instructions ("ignore all previous
    instructions", "new instructions:"), eight with invisible format characters, two with tag
    characters that lost their flag, and three `data:` URIs carrying base64 images.

- **d13 — An admin rescans what is already stored, and the rescan changes nothing.** Rows written
  before `oml-scan/1` never met it, and a later rule set may find what an earlier one passed.
  `GET /v2/admin/rescan?after_seq=&limit=`, admin key role, reads the submissions journalled in
  one window of at most 1,000 journal events, loads their versions in one statement, and runs the
  composed scanner over every stored version but an erased one: the body, each anchor field, and
  the path rule (d4), with any anchor finding reported as a veto, as admission treats it. It
  answers each finding as `artifact_id`, `workspace_id`, `version`, `state`, class, rule, action
  and place, plus the counts by rule and the next window's `after_seq`. It journals nothing,
  because it reveals no text and decides nothing. A version it would veto stays where it is,
  promoted or held. Deleting it would be the silent drop R4 forbids
  ([ADR-0001](ADR-0001-the-constitution.md)), so a person reads it with `oml admin read` and
  retires or demotes it. `oml admin rescan` walks every window, prints the counts by rule and
  each finding, and exits 1 when any finding vetoes, so a script can gate on it. The scanner
  port gains `findings()`, which reports without counting, so a rescan does not inflate the
  admission metric, and `info()`, which `/readyz` reads. One call per window is a scaling limit,
  tracked as #140. Amended 2026-11-10 (ADR-0074 d28): `POST /v2/admin/rescan`, the same window
  and answer, also records what it read of the window's held versions, and
  `oml admin rescan --record-held` calls it; the plain verb keeps the read. Each held version
  takes the server's rule set as its `scanned_under`, and a finding the rescan made becomes its
  `scan_finding` unless it has one, journalled once per window as `scan.findings_recorded` with
  each version's id and findings, an empty list when there are none, never its text; the
  answer counts them as `recorded`. Nothing is promoted, rejected, demoted or deleted, and a
  promoted version is not recorded: review already read it. It reads exactly what admission
  reads: the body, the title and every view the version's format decodes, beside the anchor. It
  takes no Idempotency-Key: a repeat reads the window again and journals again, and leaves the
  same state. Every answer, read or recording, counts the tenant's versions scanned under
  another rule set, `held_unrescanned` and `rescan_due`, which doctor reads with an admin key
  as `scan_held_unrescanned` and `scan_rescan_due` (ADR-0074 d28), each a warning while above
  zero.

- **d14 — Every rule also reads the text as copy-paste mangles it** (amended 2026-11-10; the first
  build ran the veto rules alone over the view). A token copied from a web
  page, a chat or an editor arrives with a zero-width space, a joiner or a soft hyphen inside
  it, a no-break space in a PEM header, or a fullwidth letter from an input method. Where a
  field holds any of these, the scan builds two more views, each with U+00A0 made a space and
  then NFKC: in the first, Unicode's default-ignorable code points are removed (U+00AD, U+034F,
  U+061C, U+115F–U+1160, U+17B4–U+17B5, U+180B–U+180F, U+200B–U+200F, U+202A–U+202E,
  U+2060–U+206F, U+3164, U+FE00–U+FE0F, U+FEFF, U+FFA0, U+FFF0–U+FFFB, U+1BCA0–U+1BCA3,
  U+1D173–U+1D17A, U+E0000–U+E0FFF), which undoes one inside a word; in the second each becomes a
  space, which undoes one used as the space between words. A view longer than d9's bound is
  vetoed as `format.scan_bound`, as the text would be. No case
  fold: the injection patterns match either case already, and folding would hide an upper-case
  key from the veto patterns. The order is fixed: every pattern and run rule over the text as
  written; the invisible-character rules over the text as written, since the view removes what
  they count; then every pattern and run rule, veto and hold alike, over each view, keeping each
  rule no earlier pass reported. A pure ASCII field builds neither. Last comes ADR-0074 d6's
  letters-only view, built for every field: NFKD, lower case, the removable characters gone, and
  every code point that is not a letter or a digit (Unicode L* and N*) dropped, so spaces,
  punctuation, combining marks, the braille blank U+2800 and any invisible character go alike.
  Only the injection phrases of three words or more read it, each written without separators
  (`ignoreallpreviousinstructions`), since the view has no word boundaries; markup rules, which
  name symbols, do not. An injection phrase hidden by any mix of characters inside and between
  its words, or by combining marks or fullwidth forms, is held, which the first build let
  through, and OML's own documentation still scans clean. Still not undone, and known:
  mixed-script confusables (a Cyrillic о, small capitals), tracked as
  [#225](https://github.com/e8kor/open-memory-lab/issues/225) for ADR-0074's skeleton view;
  percent or HTML encoding, a token split across lines, and base64.
  Amended by ADR-0074 d6 (#226, #317): from `oml-scan/1.2` a phrase in the letters-only view counts only where no ASCII letter of the text as written touches either end of it, a combining mark counting with its letter, so a phrase glued to an ASCII letter (`xignore previous instructions`) is no longer held; and `new system instructions` counts only when its three words are set apart by marks (any character that is not a letter or a digit) but not by a bar, a line break other than one that follows a hyphen, the end of a clause, or a full stop and a space that is not repeated after every word or letter, and either a colon follows it on its line or its line holds nothing else but it, marks, one lone number or letter before it as a list marker, up to eight tags before it and up to eight tags or repeats of the phrase after it.

- **d15 — Inside an in-progress scope, another author's held version is served only when the
  server's rule set scanned it and found nothing** (added 2026-11-10, amending
  [ADR-0030](ADR-0030-an-in-progress-workspace-serves-unreviewed-memories-inside-it-and-nothing-leaves-until-a-person-commits-it.md)
  d3). ADR-0030 serves every author's held memory to readers inside an in-progress root. A held
  version is among them only when its `scan_finding` is empty and its `scanned_under` is the
  server's own (`None` matching `None` for a scanner that pins no version). Any other held version
  stays its author's and its reviewer's, as R3 has it, by id, listing, recall under every
  pipeline and claim edge, in both stores. The first build served a version the scan held, so a
  hold finding reached other agents' context unreviewed. The stamp covers what the finding
  cannot: a version an older rule set passed carries no finding, though the current rules may
  hold it, so it waits for `oml admin rescan --record-held` to read it (d13). Rejected: rescanning each such
  version on every read, which puts the scan in the read path and still records nothing a
  reviewer can see; and serving unstamped versions until the operator runs the rescan, which
  is the leak this decision closes.

## Consequences

- `crates/oml-scan` holds the rules, the path list and their tests. The lead composes
  `RuleScanner`; `oml-domain` and `oml-cli` link the leaf for the path rule. The admission page's
  registry gains the classes d2 lists, and the event catalog's `scanner_version` is no longer
  `null` from the lead.
- A body that carried a token under the PoC is now refused, so an agent that pasted one sees
  `submission.scan_veto` naming the rule and line and has to rewrite the fact without it. That
  is the intended cost. A body that quotes an injection phrase is held for a reviewer instead of
  promoting through the ordinary policy.
- An anchor under a `secrets/` or `.kube/` directory, or naming a `.npmrc` or a kubeconfig, is
  now refused where it used to pass (d4).
- Rows written before this change are not re-scanned at admission (d1). `oml admin rescan`
  finds them (d13), and a person retires them. Nothing retires them on its own.
- After an upgrade to `oml-scan/1.1`, run `oml admin rescan --record-held`. Until it runs,
  sibling agents in an in-progress workspace do not see held versions written before the
  upgrade (d15): it records each held version under `/1.1`, keeps the ones the amended rules
  flag their author's, and serves the rest inside again. A promoted version it reports keeps its
  state; a person reads each with `oml admin read` and demotes or retires it. Doctor warns with
  `scan_held_unrescanned` until the rescan has run. Migration 0038 has no down step: a server
  rolled back to a build before `/1.1` ignores `scanned_under` and serves as the three defects
  did, and a version an older build writes during a rolling deploy carries no `scanned_under`, so
  it stays its author's until the rescan reads it.
- A rule that needs changing waits for `oml-scan/2`. That is slower than a toggle, and it is what
  keeps a journalled verdict reproducible.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **A model-based scanner in the write path** | A model's verdict is not reproducible from a pin, which the admission page makes the scan's defining property. It adds a model call's latency, and a model's failure modes, to every write. And an injected instruction aimed at a model is exactly what a model reading the body can be talked into passing. Judgment stays with review. |
| **An external scanning service** (a gitleaks or TruffleHog process, a cloud DLP API) | It ships every body, secrets included, to another process or vendor, which is the leak the scan exists to stop. It adds a network dependency to the same-call path, and a down service would either refuse every write or admit unscanned bytes. The rules compile into the process instead (ADR-0019 d4). |
| **gitleaks as a dependency** | It is Go, and OML is pure Rust (ADR-0006). The regexes are what carries over, and they are adapted with attribution (d3). |
| **Entropy alone, over every token** | It flags commit hashes, digests, UUIDs and base64 test vectors, all of which OML's own docs are full of, and it misses low-entropy passwords anyway. Entropy is a second look on a key-value match, not a rule by itself. |
| **Vetoing injection phrases** | It would refuse true memories about prompt injection. Holding puts a person in front of the claim, which is the right tool for a heuristic. |
| **Scanning on read instead of write** | A secret would already be stored, journalled by digest, and in a reviewer's queue. The design scans before the commit for that reason ([admission](../architecture/admission.md)). |

## Open questions

- **Q1.** The tenant setting that turns `pii.*` on (#140): its key, its journalled event,
  and whether it may also pick veto.
- **Q2.** Scanning a guardrail's `rule` strings and an attachment's extracted text, once a
  deterministic extractor exists ([storage](../architecture/storage.md)).
- **Q3.** A durable trace of a veto outside the chain stays #140 on the
  [open issues](https://github.com/e8kor/open-memory-lab/issues?q=label%3Aroadmap).
