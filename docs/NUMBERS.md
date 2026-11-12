<!--
id: engineering/numbers
tier: engineering
status: process register
answers: which ADR and migration numbers are used or reserved, and by which branch
-->

# Number registry

Every ADR and migration number is used on main or reserved by one branch. Reserve a
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
| adr | 0064 | `design/adr-0064-on-main` | #346 | a repositorys knowledge is captured implicitly by git hooks and agent  | open |
| adr | 0065 | `main` |  | the scan is a versioned compiled in rule set that vetoes leaked keys a | used |
| adr | 0066 | `main` |  | open work is tracked as github issues and the roadmap fr numbers and d | used |
| adr | 0067 | `main` |  | a directory reviewer reaches a demo workarea through a connect code th | used |
| adr | 0068 | `main` |  | anyone signs in and gets a private root under quotas when the operator | used |
| adr | 0069 | `main` |  | oml s design system is the source of truth for every surface | used |
| adr | 0070 | `main` |  | the web console is a client of the public http api in a crate that can | used |
| adr | 0071 | `main` |  | every account is metered by counters the write updates in its own tran | used |
| adr | 0072 | `main` |  | recall searches the trusted workareas a key already reads chosen per q | used |
| adr | 0073 | `main` |  | a leader signs every write and nodes serve the workareas placed on the | used |
| adr | 0074 | `main` |  | the scan names a threat class for every finding refuses only what can  | used |
| adr | 0075 | `main` |  | each approved memory is indexed by the questions it answers generated  | used |
| adr | 0076 | `main` |  | releases ship no default server and setup asks for the server or reads | used |
| adr | 0077 | `main` |  | a served memory says when it was approved and by what kind of reviewer | used |
| adr | 0078 | `design/recall-improvement` |  | better recall combines cross-encoder reranking with an optional learned sparse retrieval stage measured against OML's standing eval | reserved |
| adr | 0079 | `bulk/v0.7.3b` |  | a key ends on a date and is held to a request rate, both set at issuance and changed only by an admin key that is not the key itself | reserved |
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
| migration | 0032 | `main` |  | claim formats | used |
| migration | 0033 | `main` |  | claim formats validate | used |
| migration | 0034 | `main` |  | similar keys search text | used |
| migration | 0036 | `feat/implicit-capture` |  | code parent | reserved |
| migration | 0037 | `main` |  | open signup | used |
| migration | 0038 | `main` |  | scanned under | used |
| migration | 0039 | `main` |  | web session credentials | used |
| migration | 0040 | `main` |  | federated recall | used |
| migration | 0041 | `main` |  | as of tips | used |
| migration | 0042 | `main` |  | idempotency claims | used |
| migration | 0043 | `main` |  | scan findings | used |
| migration | 0044 | `main` |  | memory approval | used |
| migration | 0045 | `main` |  | usage metering | used |
| migration | 0046 | `bulk/v0.7.3b` |  | key limits | reserved |
