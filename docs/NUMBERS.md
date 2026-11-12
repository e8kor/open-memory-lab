<!--
id: engineering/numbers
tier: engineering
status: process register
answers: which ADR, FR and migration numbers are used or reserved, and by which branch
-->

# Number registry

Every ADR, FR and migration number is used on main or reserved by one branch. Reserve a
number before writing the file or row that uses it:

```sh
python scripts/reserve_number.py adr --branch <branch> --title "<the decision, one sentence>"
git add docs/NUMBERS.md && git commit -m "chore: reserve ADR-nnnn" && git push
```

`scripts/check_numbers.py` fails a pull request that introduces a number the registry assigns
to another branch, or does not list. A reservation pushed on any unmerged remote branch holds its
number, whether or not the branch has a pull request yet; delete a dead branch to release its
numbers. `python scripts/reserve_number.py --rebuild` rescans main and the unmerged remote
branches and keeps the reservations. A number listed for two branches is a collision: the
branch that reserved later renumbers.

| kind | number | branch | PR | title | state |
|---|---|---|---|---|---|
| adr | 0001 | `main` |  | the constitution | used |
| adr | 0002 | `main` |  | postgresql system of record | used |
| adr | 0003 | `main` |  | delta lake derived lakehouse | used |
| adr | 0004 | `main` |  | bodies and attachments | used |
| adr | 0005 | `main` |  | erasure by crypto shredding | used |
| adr | 0006 | `main` |  | pure rust | used |
| adr | 0007 | `main` |  | tenancy first class | used |
| adr | 0008 | `main` |  | workarea algebra | used |
| adr | 0009 | `main` |  | status ladder | used |
| adr | 0010 | `main` |  | roles and keys | used |
| adr | 0011 | `main` |  | simulations | used |
| adr | 0012 | `main` |  | admission and review | used |
| adr | 0013 | `main` |  | reads served from postgres | used |
| adr | 0014 | `main` |  | http api and mcp facade | used |
| adr | 0015 | `main` |  | cli administration | used |
| adr | 0016 | `main` |  | kubernetes workloads | used |
| adr | 0017 | `main` |  | reconciliation | used |
| adr | 0018 | `main` |  | the documentation set | used |
| adr | 0019 | `main` |  | hexagonal architecture and plugins | used |
| adr | 0020 | `main` |  | dataset onboarding | used |
| adr | 0021 | `main` |  | ai human ontologies | used |
| adr | 0022 | `main` |  | two ring api | used |
| adr | 0023 | `main` |  | install profiles and cluster mode | used |
| adr | 0024 | `main` |  | performance foundation | used |
| adr | 0025 | `main` |  | the agent only memory surface | used |
| adr | 0026 | `main` |  | names are keys | used |
| adr | 0028 | `main` |  | agent key issuance | used |
| adr | 0029 | `main` |  | session start hook | used |
| adr | 0030 | `main` |  | an in progress workarea serves unreviewed memories inside it and nothi | used |
| adr | 0031 | `main` |  | oauth connections by connect code | used |
| adr | 0033 | `main` |  | interim helm chart | used |
| adr | 0034 | `main` |  | the lead serves as a role that owns nothing | used |
| adr | 0037 | `main` |  | a guardrail is a reviewed rule loaded at session start and checked at  | used |
| adr | 0038 | `main` |  | the agent recalls for itself and a failed command recalls only when op | used |
| adr | 0039 | `main` |  | the api rewritten for v0 4 0 | used |
| adr | 0040 | `main` |  | plugin setup in claude code | used |
| adr | 0041 | `main` |  | a quick start in one command per role | used |
| adr | 0042 | `main` |  | every request logs one line under an id the caller may name and metric | used |
| adr | 0043 | `main` |  | what oml releases defaults to the maintainers public server | used |
| adr | 0044 | `main` |  | every user works under a root of their own and universe serves curated | used |
| adr | 0045 | `main` |  | people sign in to a user page and an admin page on the servers own ori | used |
| adr | 0046 | `main` |  | the lifecycle claim edges and the brief ship in v0 5 2 inside user roo | used |
| adr | 0047 | `main` |  | project knowledge and a code skeleton follow the development lifecycle | used |
| adr | 0048 | `main` |  | search improves in stages any word first meaning last each gated on th | used |
| adr | 0049 | `main` |  | search is a pipeline of retrievers behind a port chosen by name and ga | used |
| adr | 0050 | `main` |  | remembering checks what is already held replaces instead of repeating  | used |
| adr | 0051 | `design/ontology-metadata-r2` | #58 | a memory says what it is about in reviewed terms and recall finds it b | open |
| adr | 0052 | `main` |  | text search reads integer postings that row security lets an index ser | used |
| adr | 0053 | `main` |  | reranking is a local cross encoder stage after fusion off by default u | used |
| adr | 0054 | `main` |  | recall is measured by what readers do next and only other readers use  | used |
| adr | 0055 | `main` |  | a person reviews a listed set of held items in one act each item judge | used |
| adr | 0056 | `feat/claim-formats` | #90 | a claim version names its format the signature covers its source and a | open |
| adr | 0056 | `main` |  | a claim version names its format the signature covers its source and a | used |
| adr | 0057 | `main` |  | a memory carries the source anchor its author declares and oml sync ke | used |
| adr | 0058 | `main` |  | the public server runs to stated slos with an error budget and without | used |
| adr | 0059 | `main` |  | a memory states when it is true recall hides what has ended and search | used |
| adr | 0060 | `main` |  | the lake writer drains the outbox it has seals what it materializes an | used |
| adr | 0061 | `main` |  | oml sync reads a rust workspace as one claim per crate and one per mod | used |
| adr | 0062 | `main` |  | oml sync maps a delta lakes tables into anchored claims from the log a | used |
| adr | 0063 | `main` |  | a chat turn becomes memory only when the agent in it writes the fact a | used |
| adr | 0064 | `design/adr-0064-on-main` |  | a repositorys knowledge is captured implicitly by git hooks and agent  | open |
| adr | 0065 | `main` |  | the scan is a versioned compiled in rule set that vetoes leaked keys a | used |
| adr | 0066 | `docs/restructure` |  | open work is tracked as github issues and the roadmap FR numbers and design draft status lines are retired | reserved |
| adr | 0067 | `feat/connector-directory` |  | a directory reviewer reaches a demo workarea through a connect code that lives longer, and the tools and instructions describe instead of command | reserved |
| adr | 0067 | `main` |  | a directory reviewer reaches a demo workarea through a connect code th | used |
| adr | 0068 | `feat/open-signup` |  | anyone signs in and gets a private root under quotas when the operator opens sign-up | reserved |
| adr | 0069 | `feat/design-system` |  | OML's design system is the source of truth for every surface | reserved |
| adr | 0070 | `design/web-api-client` |  | the web console is a client of the public HTTP API in a crate that cannot reach the application core | reserved |
| adr | 0071 | `bulk/v0.7.3` |  | every account is metered by counter rows updated in the write's own transaction, and named quota profiles set its limits | reserved |
| adr | 0072 | `design/federated-recall` |  | recall searches trusted and advertised workareas the key can already read, chosen per query by their reviewed descriptors | reserved |
| adr | 0073 | `design/distributed-nodes` |  | a leader signs every write and nodes serve the workareas placed on them, each read routed to the closest less busy replica fresh enough for it | reserved |
| adr | 0074 | `design/scan-expansion` |  | the scan names a threat class for every finding, refuses only what can never be safe, holds or annotates the rest, and recall serves every hit as data with its findings | reserved |
| adr | 0075 | `design/hypothetical-questions` | #239 | each approved memory is indexed by the questions it answers, generated off the request path, scanned, stored with their generator, and matched only by an opt-in pipeline | reserved |
| adr | 0076 | `docs/adr-no-default-server` | #265 | releases ship no default server and setup asks for the server or reads it from the key or the environment | reserved |
| adr | 0077 | `recall72/recall-approver` |  | a served memory says when it was approved and by what kind of reviewer, and never names the reviewer | reserved |
| fr | 1 | `main` |  | Erasure destroys body and attachment bytes by destroying their keys; a | used |
| fr | 2 | `main` |  | Review throughput is unmeasured. The design's gate is only as useful a | used |
| fr | 3 | `main` |  | The lake mirror claims exactly-once application of every event across  | used |
| fr | 4 | `main` |  | Search quality over the serving store is unmeasured: recall, latency,  | used |
| fr | 5 | `main` |  | Lake tables hold encrypted bodies only. A decrypted export scoped to a | used |
| fr | 6 | `main` |  | Rotating a crew's keys is key-by-key. A crew of many agents whose keys | used |
| fr | 7 | `main` |  | The record store's scale ceiling is unmeasured — how many events, memo | used |
| fr | 8 | `main` |  | Rate limiting is an approximation: each server replica enforces a tena | used |
| fr | 9 | `main` |  | The scoped market claim on the positioning page — no provider ships st | used |
| fr | 10 | `main` |  | The agent surface fails the only-memory test: recall excludes the call | used |
| fr | 11 | `main` |  | Retrieval counts rows (`k`), not tokens; agents assemble prompts under | used |
| fr | 12 | `main` |  | There is no batch write path and no client SDK: one memory per POST, a | used |
| fr | 13 | `main` |  | Dataset onboarding (ADR-0020) is designed and explicitly post-v1: its  | used |
| fr | 14 | `main` |  | Import performance and its retrieval cost are unmeasured: capture-side | used |
| fr | 15 | `main` |  | Admission-side open points have no rows of their own: scan latency is  | used |
| fr | 16 | `main` |  | Lake-writer throughput and Delta commit latency are unmeasured, and th | used |
| fr | 17 | `main` |  | The erasure SLO's composing windows — vacuum cadence, backup retention | used |
| fr | 18 | `main` |  | Full-genesis reconciler runtime and quorum-synchronous commit latency  | used |
| fr | 19 | `main` |  | Vocabularies as governed claims (ADR-0021): the v1 slice — prefix regi | used |
| fr | 20 | `main` |  | The retrieval value and cost of vocabulary expansion are unmeasured: r | used |
| fr | 21 | `main` |  | The provider ring (`/v1`, ADR-0022) carries a compatibility covenant — | used |
| fr | 22 | `main` |  | Three install profiles exist on paper (ADR-0023); nothing proves they  | used |
| fr | 23 | `main` |  | The standalone → docker → kubernetes migration path is designed (ADR-0 | used |
| fr | 24 | `main` |  | Lead failover semantics (ADR-0023) ship as unmeasured profile defaults | used |
| fr | 25 | `main` |  | The provider ring claims a very performant foundation and owns no numb | used |
| fr | 26 | `main` |  | Memory content has no automatic expiry and no retention machinery: not | used |
| fr | 27 | `main` |  | Tenant offboarding is a composed ceremony with missing verbs (runbooks | used |
| fr | 28 | `main` |  | Three journalled operator acts have no CLI verb and are composed by ha | used |
| fr | 29 | `main` |  | Attachment fetch (`GET /v1/attachments/{digest}`) and the export bundl | used |
| fr | 30 | `main` |  | Recall budgets count text by a fixed rule (four bytes of text per budg | used |
| fr | 31 | `main` |  | When an agent starts a new claim under a name another claim in the sam | used |
| fr | 32 | `main` |  | The background repair service holds a key with the role reviewers hold | used |
| fr | 33 | `main` |  | When Claude Code starts, resumes, clears, compacts or forks a session, | used |
| fr | 34 | `main` |  | Connecting Claude's own apps — claude.ai, Claude Desktop and the Claud | used |
| fr | 35 | `main` |  | When OML turns a request away because it carries no valid key, the ans | used |
| fr | 36 | `main` |  | When a request that creates a key, or replaces one with its successor, | used |
| fr | 44 | `main` |  | The server reaches its database through the same account that created  | used |
| fr | 45 | `main` |  | Two parts of the database's own machinery still tell one tenant someth | used |
| fr | 50 | `main` |  | Teams cannot yet give their agents standing rules — "never push to the | used |
| fr | 51 | `main` |  | Whether Claude Code runs the agent plugin's rule hooks when a session  | used |
| fr | 52 | `main` |  | A rule an agent receives only as text is an instruction to an AI model | used |
| fr | 53 | `main` |  | Codex, Gemini CLI, Cursor and GitHub Copilot's command-line agent each | used |
| fr | 54 | `main` |  | A rule exported into a project's files, and the copy of the rules each | used |
| fr | 55 | `main` |  | An agent that ends its work cannot leave a note for its next session — | used |
| fr | 56 | `main` |  | An area cannot hold a reviewed checklist — a definition of done — that | used |
| fr | 57 | `main` |  | Recall at session start searches with the name of the working director | used |
| fr | 58 | `main` |  | Every standing instruction loads at every session start. Instructions  | used |
| fr | 59 | `main` |  | Reviewers have no reviewed guidance of their own — a rubric shown when | used |
| fr | 60 | `main` |  | An agent entering an area starts without the few reviewed facts it mos | used |
| fr | 61 | `main` |  | Nothing summarizes a whole area for an agent that enters it. A summary | used |
| fr | 62 | `main` |  | Recall at session start searches the agent's area and every area below | used |
| fr | 63 | `main` |  | When a command fails, the agent plugin can add the memories that match | used |
| fr | 64 | `main` |  | The agent is asked to search memory itself before non-trivial work, on | used |
| fr | 65 | `main` |  | The way agents and programs talk to OML is simplified in release 0.4.0 | used |
| fr | 66 | `main` |  | A reviewer's decision is meant to be bound to the exact content the se | used |
| fr | 67 | `main` |  | The agent plugin takes its server address and key through Claude Code' | used |
| fr | 68 | `main` |  | How long it takes a new person to save a first memory and read it back | used |
| fr | 69 | `main` |  | The one-line install checks what it downloads against a checksum list  | used |
| fr | 70 | `main` |  | Release 0.4.1 is planned to log, for each request, how long the server | used |
| fr | 71 | `main` |  | What the request log, the metrics and the optional trace export cost e | used |
| fr | 72 | `main` |  | Since release 0.5.0 each person gets a workarea of their own (ADR-0044 | used |
| fr | 73 | `main` |  | A memory a curator copies into the organisation's shared area is shown | used |
| fr | 74 | `main` |  | Planned for release 0.5.0, a search inside a person's own workarea als | used |
| fr | 75 | `main` |  | Nothing records when a key was last used, so the user page, since rele | used |
| fr | 76 | `main` |  | The sign-in pages of release 0.5.0 (ADR-0045) are built and tested aga | used |
| fr | 77 | `main` |  | A workarea's brief is cut to `brief.max_tokens`, 400 budget tokens unl | used |
| fr | 78 | `main` |  | Whether project knowledge and the code skeleton help an agent is unmea | used |
| fr | 79 | `main` |  | ADR-0047's capture cap of 5 entries per session and its batch cap of 2 | used |
| fr | 80 | `main` |  | ADR-0047 creates one workarea per branch and deletes none, so a reposi | used |
| fr | 81 | `main` |  | The proposed ADR-0048 orders its stages by version 1 of OML's recall e | used |
| fr | 82 | `main` |  | ADR-0048's stage S7: the indexer lag after a promotion, the latency a  | used |
| fr | 83 | `main` |  | ADR-0048's any-word recall (S1) always serves something when one word  | used |
| fr | 84 | `main` |  | ADR-0048's stage S2 computes rarity statistics per query over the read | used |
| fr | 85 | `main` |  | ADR-0048's stage S6 reranks the top 50 with a cross-encoder; measured  | used |
| fr | 86 | `main` |  | Concurrent writes and verdicts in one organisation meet `503 system.un | used |
| fr | 87 | `main` |  | `review next` serves a reviewer key its own claimed row again until it | used |
| fr | 88 | `main` |  | The reranking pipeline `bm25-rerank` was measured only on a host share | used |
| fr | 89 | `main` |  | A recall cannot yet answer "nothing relevant" well: the relevance floo | used |
| fr | 90 | `main` |  | The proposed ADR-0050 d1 names, at each write, up to three memories th | used |
| fr | 91 | `main` |  | ADR-0050 d1 runs one lookup on every write (d17); what it adds to a wr | used |
| fr | 92 | `main` |  | The advisory warnings of ADR-0050 d6 (too long, several claims, vague, | used |
| fr | 93 | `main` |  | How many writes repeat a memory already held, and how many stale memor | used |
| fr | 94 | `main` |  | The batch limit of 20 writes and the capture limit of 5 per session in | used |
| fr | 95 | `design/ontology-metadata-r2` | #58 | The proposed ADR-0051 finds memories by the reviewed terms a query nam | open |
| fr | 96 | `design/ontology-metadata-r2` | #58 | What a reviewed vocabulary costs a crew is unmeasured: the time to wri | open |
| fr | 97 | `design/ontology-metadata-r2` | #58 | How often writing agents choose wrong terms, how many memories get non | open |
| fr | 98 | `design/ontology-metadata-r2` | #58 | The term retriever's label match and edge lookup add to every recall i | open |
| fr | 99 | `design/ontology-metadata-r2` | #58 | A broad label such as "commit" or "planned" draws memories into a reca | open |
| fr | 100 | `main` |  | The integer postings that let a text search use an index under row sec | used |
| fr | 101 | `main` |  | The learned sparse encoder that would write a second kind of postings  | used |
| fr | 102 | `main` |  | The per-workarea word counts that keep search statistics to the rows a | used |
| fr | 103 | `main` |  | Migration 0021 fills the postings and counts of every existing version | used |
| fr | 104 | `main` |  | Recall telemetry (ADR-0054 d2) adds an event per recall, written off t | used |
| fr | 105 | `main` |  | The windows in which a get, a derivation or a second recall label a re | used |
| fr | 106 | `main` |  | Usage as a ranking signal is served only as the opt-in pipeline `bm25- | used |
| fr | 107 | `main` |  | Usage counts one credit per principal per version per day, so one pers | used |
| fr | 108 | `main` |  | The real-usage eval set is rolling: its query text lives 30 days, so n | used |
| fr | 109 | `feat/claim-formats` | #90 | A Mermaid claim is validated by OML's own parser of a subset (ADR-0056 | open |
| fr | 109 | `main` |  | A Mermaid claim is validated by OML's own parser of a subset (ADR-0056 | used |
| fr | 110 | `feat/claim-formats` | #90 | The pinned Mermaid bundle adds to the server binary and to the first r | open |
| fr | 110 | `main` |  | The pinned Mermaid bundle adds to the server binary and to the first r | used |
| fr | 111 | `feat/claim-formats` | #90 | A write in a non-prose format parses its source and stores a search te | open |
| fr | 111 | `main` |  | A write in a non-prose format parses its source and stores a search te | used |
| fr | 112 | `feat/claim-formats` | #90 | Whether diagram and fragment claims help an agent is unmeasured: the r | open |
| fr | 112 | `main` |  | Whether diagram and fragment claims help an agent is unmeasured: the r | used |
| fr | 113 | `feat/claim-formats` | #90 | The per-format thresholds of `quality.many_claims` (12 relations in a | open |
| fr | 113 | `main` |  | The per-format thresholds of `quality.many_claims` (12 relations in a  | used |
| fr | 114 | `main` |  | The proposed ADR-0057 d5 marks a memory `stale` only once `oml sync` o | used |
| fr | 115 | `main` |  | The origin filters `anchor_repo` and `anchor_path_prefix` (ADR-0057 d4 | used |
| fr | 116 | `main` |  | A document sync writes up to 200 items a run for one person to review  | used |
| fr | 117 | `main` |  | A claim edge whose statement leaves the documents (an `Amends:` line e | used |
| fr | 118 | `main` |  | The learned sparse leg reads every posting of each query token among t | used |
| fr | 119 | `main` |  | Migration 0027 fills the near-copy check's keys of every existing vers | used |
| fr | 130 | `main` |  | The proposed ADR-0061 loads OML's crates as 92 claims (9 crates, 83 mo | used |
| fr | 131 | `main` |  | A module claim goes stale only when its doc changes (ADR-0061 d8), so  | used |
| fr | 132 | `main` |  | Crate dependencies are stated in each crate claim's text and not as `d | used |
| fr | 133 | `main` |  | Whether syn in the shipped `oml` binary would change its build time or | used |
| fr | 140 | `main` |  | The proposed ADR-0062 d3 ships `--adapter delta` in a second `oml` art | used |
| fr | 141 | `main` |  | The minimal Delta log reader (ADR-0062 d3, d4) is checked against delt | used |
| fr | 142 | `main` |  | The review burden of a lake (ADR-0062 d18) depends on how many columns | used |
| fr | 143 | `main` |  | The object-storage requests, time and cost of syncing 2,000 tables (AD | used |
| fr | 144 | `main` |  | The lake eval (ADR-0062 d19) has no results: whether table, schema and | used |
| fr | 145 | `main` |  | A table copied to a new location reads `removed` there and `new` here  | used |
| fr | 146 | `main` |  | A lake sync records source checks one request each through ADR-0057's  | used |
| fr | 147 | `main` |  | ADR-0062 d1 moves ADR-0020 d7's row reading to the client, which strea | used |
| fr | 160 | `main` |  | Whether the agent's own extraction of chat facts answers more LongMemE | used |
| fr | 161 | `main` |  | The turns of a session that ends between two captures are not captured | used |
| fr | 162 | `main` |  | Per-turn recall's relevance is unlabelled: at least 30 labelled inject | used |
| fr | 163 | `main` |  | One person's chat on two machines produces two pseudonymous sessions t | used |
| fr | 164 | `main` |  | The chat benchmark's QA numbers wait for an API key in the environment | used |
| fr | 170 | `feat/implicit-capture` | #97 | The pre-tool hook chooses an anchor's file from the paths a fact names | open |
| fr | 171 | `feat/implicit-capture` | #97 | A fact's span is the indentation block of the one definition line its  | open |
| fr | 172 | `feat/implicit-capture` | #97 | Every latency budget of ADR-0064 d22 is chosen: 100 ms for a git hook  | open |
| fr | 173 | `feat/implicit-capture` | #97 | Session start recalls the repository's knowledge with `k` 6 under 600  | open |
| fr | 174 | `feat/implicit-capture` | #97 | A chat fact about code carries its turn in `origin.turn`, stored and s | open |
| fr | 175 | `feat/implicit-capture` | #97 | Each repository a person opts into adds one workarea under their code  | open |
| fr | 176 | `feat/implicit-capture` | #97 | A hook manager that rewrites `core.hooksPath` on install silently remo | open |
| fr | 180 | `main` |  | The personal-data rules (`pii.email`, `pii.phone`) are compiled in and | used |
| fr | 181 | `main` |  | `GET /v2/admin/rescan` reads one window of at most 1,000 journal event | used |
| migration | 0001 | `main` |  | init | used |
| migration | 0002 | `main` |  | poc adapter | used |
| migration | 0003 | `main` |  | lead startup | used |
| migration | 0004 | `main` |  | grants | used |
| migration | 0005 | `main` |  | claim versions | used |
| migration | 0006 | `main` |  | names | used |
| migration | 0007 | `main` |  | role split | used |
| migration | 0008 | `main` |  | tenant keys | used |
| migration | 0009 | `main` |  | memories partitions | used |
| migration | 0012 | `main` |  | connections | used |
| migration | 0013 | `main` |  | oauth clients | used |
| migration | 0014 | `main` |  | user roots | used |
| migration | 0015 | `main` |  | web | used |
| migration | 0016 | `main` |  | guardrails | used |
| migration | 0017 | `main` |  | workarea transitions | used |
| migration | 0018 | `main` |  | claim edges | used |
| migration | 0019 | `main` |  | brief edges | used |
| migration | 0020 | `main` |  | search | used |
| migration | 0021 | `main` |  | search postings | used |
| migration | 0022 | `main` |  | search sparse queue | used |
| migration | 0023 | `main` |  | recall telemetry | used |
| migration | 0024 | `main` |  | review queue order | used |
| migration | 0025 | `main` |  | source anchors | used |
| migration | 0026 | `main` |  | valid time | used |
| migration | 0027 | `main` |  | similar keys | used |
| migration | 0028 | `main` |  | in progress | used |
| migration | 0029 | `main` |  | origin kind | used |
| migration | 0030 | `main` |  | origin kind validate | used |
| migration | 0031 | `main` |  | lake writer | used |
| migration | 0032 | `feat/claim-formats` | #90 | claim formats | reserved |
| migration | 0032 | `main` |  | claim formats | used |
| migration | 0033 | `feat/claim-formats` | #90 | claim formats validate | reserved |
| migration | 0033 | `main` |  | claim formats validate | used |
| migration | 0034 | `feat/claim-formats` | #90 | similar keys search text | reserved |
| migration | 0034 | `main` |  | similar keys search text | used |
| migration | 0036 | `feat/implicit-capture` |  | code parent | reserved |
| migration | 0037 | `feat/open-signup` |  | open signup | reserved |
| migration | 0038 | `fix/scan-bypasses` |  | scanned under | reserved |
| migration | 0039 | `feat/web-api-phase-a` | #188 | web session credentials | reserved |
| migration | 0040 | `feat/federated-recall-p1` | #205 | federated recall | reserved |
| migration | 0041 | `feat/as-of-reads` |  | as of tips | reserved |
| migration | 0042 | `fix/idempotency-atomic` | #233 | idempotency claims | reserved |
| migration | 0043 | `feat/scan-phase1` |  | scan findings | reserved |
| migration | 0044 | `recall72/recall-approver` |  | memory approval | reserved |
| migration | 0045 | `bulk/v0.7.3` |  | usage metering | reserved |
