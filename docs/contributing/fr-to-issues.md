<!--
id: contributing/fr-to-issues
tier: engineering
status: current
answers: which GitHub issue replaced each former roadmap row (FR-n, RS-n), so old references resolve
-->

# Former roadmap rows and the issues that replaced them

ADR-0066 retired the roadmap and its FR and RS numbers. Each open row became part of one of 62
issues (a 60th, on simulations and the reconciler, holds none); rows that described shipped behaviour closed without an issue. References in older
commits, pull requests and branches to `FR-n` or `RS-n` resolve here. A few numbers never had a
row (FR-37 to FR-43, FR-46 to FR-49, FR-95 to FR-99, RS-2) and map nowhere.
FR-170 to FR-176 are the rows ADR-0064 opened on its branch on 2026-11-05, the day before the
roadmap was retired, so they were never among those rows: the tracking issue of ADR-0064's build,
#344, holds them until each is an issue of its own.

| Former id | Issue | Title |
|---|---|---|
| FR-1 | [#146](https://github.com/e8kor/open-memory-lab/issues/146) | Erase or pseudonymise attribution metadata |
| FR-2 | [#98](https://github.com/e8kor/open-memory-lab/issues/98) | Measure review throughput and queue drain |
| FR-3 | [#101](https://github.com/e8kor/open-memory-lab/issues/101) | Prove exactly-once lake application across crashes, VACUUM and checkpoints |
| FR-4 | [#102](https://github.com/e8kor/open-memory-lab/issues/102) | Publish search quality and latency at stated corpus sizes |
| FR-5 | [#125](https://github.com/e8kor/open-memory-lab/issues/125) | Decrypted export scoped to a named audience |
| FR-6 | [#120](https://github.com/e8kor/open-memory-lab/issues/120) | Bulk key rotation for a crew |
| FR-7 | [#99](https://github.com/e8kor/open-memory-lab/issues/99) | Find the record store's scale ceiling and the partition plan |
| FR-8 | [#133](https://github.com/e8kor/open-memory-lab/issues/133) | Rate-limit precision across replicas |
| FR-9 | none | dropped: the market-claim recheck is not tracked publicly |
| FR-10 | none | shipped in v0.2.0 (own-held recall, MCP get and supersede, retire-proposal) |
| FR-11 | [#106](https://github.com/e8kor/open-memory-lab/issues/106) | Compare the recall budget rule with model tokenizers |
| FR-12 | [#122](https://github.com/e8kor/open-memory-lab/issues/122) | Batch write endpoint and upload wrapper |
| FR-13 | [#148](https://github.com/e8kor/open-memory-lab/issues/148) | Dataset onboarding: first slice, Merkle construction, sampling and connectors |
| FR-14 | [#148](https://github.com/e8kor/open-memory-lab/issues/148) | Dataset onboarding: first slice, Merkle construction, sampling and connectors |
| FR-15 | [#147](https://github.com/e8kor/open-memory-lab/issues/147) | Admission: scan latency and attachment open points |
| FR-16 | [#101](https://github.com/e8kor/open-memory-lab/issues/101) | Prove exactly-once lake application across crashes, VACUUM and checkpoints |
| FR-17 | [#108](https://github.com/e8kor/open-memory-lab/issues/108) | Time an end-to-end erasure against the erasure SLO |
| FR-18 | [#99](https://github.com/e8kor/open-memory-lab/issues/99) | Find the record store's scale ceiling and the partition plan |
| FR-19 | [#149](https://github.com/e8kor/open-memory-lab/issues/149) | Vocabularies as governed claims |
| FR-20 | [#149](https://github.com/e8kor/open-memory-lab/issues/149) | Vocabularies as governed claims |
| FR-21 | [#150](https://github.com/e8kor/open-memory-lab/issues/150) | Compatibility covenant of /v1 and measured latency targets |
| FR-22 | [#117](https://github.com/e8kor/open-memory-lab/issues/117) | Verify the three install profiles are one product |
| FR-23 | [#117](https://github.com/e8kor/open-memory-lab/issues/117) | Verify the three install profiles are one product |
| FR-24 | [#117](https://github.com/e8kor/open-memory-lab/issues/117) | Verify the three install profiles are one product |
| FR-25 | [#150](https://github.com/e8kor/open-memory-lab/issues/150) | Compatibility covenant of /v1 and measured latency targets |
| FR-26 | [#123](https://github.com/e8kor/open-memory-lab/issues/123) | Retention policy, legal hold and tenant offboarding verbs |
| FR-27 | [#123](https://github.com/e8kor/open-memory-lab/issues/123) | Retention policy, legal hold and tenant offboarding verbs |
| FR-28 | [#124](https://github.com/e8kor/open-memory-lab/issues/124) | CLI verbs for dead-letter re-enqueue, erase replay and standalone migrate |
| FR-29 | [#121](https://github.com/e8kor/open-memory-lab/issues/121) | Range requests on attachment and export download |
| FR-30 | [#106](https://github.com/e8kor/open-memory-lab/issues/106) | Compare the recall budget rule with model tokenizers |
| FR-31 | [#129](https://github.com/e8kor/open-memory-lab/issues/129) | A name-collision refusal leaks a name across areas |
| FR-32 | [#128](https://github.com/e8kor/open-memory-lab/issues/128) | Limit the repair service's reviewer-role key to its one act |
| FR-33 | [#114](https://github.com/e8kor/open-memory-lab/issues/114) | Verify Claude Code runs the plugin hooks |
| FR-34 | [#115](https://github.com/e8kor/open-memory-lab/issues/115) | Verify the claude.ai and mobile connect flow end to end |
| FR-35 | [#126](https://github.com/e8kor/open-memory-lab/issues/126) | Send WWW-Authenticate on every 401 |
| FR-36 | [#127](https://github.com/e8kor/open-memory-lab/issues/127) | A retried key creation must not store the key value |
| FR-44 | [#134](https://github.com/e8kor/open-memory-lab/issues/134) | The owner database account can read statistics over every tenant's text |
| FR-45 | [#134](https://github.com/e8kor/open-memory-lab/issues/134) | The owner database account can read statistics over every tenant's text |
| FR-50 | none | shipped in v0.5.1 (guardrails) |
| FR-51 | [#114](https://github.com/e8kor/open-memory-lab/issues/114) | Verify Claude Code runs the plugin hooks |
| FR-52 | [#112](https://github.com/e8kor/open-memory-lab/issues/112) | Do agents follow rules they only read? |
| FR-53 | [#116](https://github.com/e8kor/open-memory-lab/issues/116) | Verify the hooks of Codex, Gemini CLI, Cursor and Copilot |
| FR-54 | [#137](https://github.com/e8kor/open-memory-lab/issues/137) | Lazy-loaded rules and exported-rule copies that outlive withdrawal |
| FR-55 | [#136](https://github.com/e8kor/open-memory-lab/issues/136) | Session notes, definition-of-done checklists, reviewer rubrics and area summaries |
| FR-56 | [#136](https://github.com/e8kor/open-memory-lab/issues/136) | Session notes, definition-of-done checklists, reviewer rubrics and area summaries |
| FR-57 | [#135](https://github.com/e8kor/open-memory-lab/issues/135) | Start-of-session recall: area, sub-areas, any-word match and lessons |
| FR-58 | [#137](https://github.com/e8kor/open-memory-lab/issues/137) | Lazy-loaded rules and exported-rule copies that outlive withdrawal |
| FR-59 | [#136](https://github.com/e8kor/open-memory-lab/issues/136) | Session notes, definition-of-done checklists, reviewer rubrics and area summaries |
| FR-60 | [#136](https://github.com/e8kor/open-memory-lab/issues/136) | Session notes, definition-of-done checklists, reviewer rubrics and area summaries |
| FR-61 | [#136](https://github.com/e8kor/open-memory-lab/issues/136) | Session notes, definition-of-done checklists, reviewer rubrics and area summaries |
| FR-62 | [#135](https://github.com/e8kor/open-memory-lab/issues/135) | Start-of-session recall: area, sub-areas, any-word match and lessons |
| FR-63 | [#114](https://github.com/e8kor/open-memory-lab/issues/114) | Verify Claude Code runs the plugin hooks |
| FR-64 | [#112](https://github.com/e8kor/open-memory-lab/issues/112) | Do agents follow rules they only read? |
| FR-65 | none | shipped in v0.4.0 (the API rewrite) |
| FR-66 | [#130](https://github.com/e8kor/open-memory-lab/issues/130) | Bind a verdict to the exact content served, with an expiry |
| FR-67 | [#118](https://github.com/e8kor/open-memory-lab/issues/118) | Verify plugin settings through the Claude Code keychain |
| FR-68 | [#107](https://github.com/e8kor/open-memory-lab/issues/107) | Time and CI-verify the first-memory install paths |
| FR-69 | [#132](https://github.com/e8kor/open-memory-lab/issues/132) | Install: verify the release signature without installing cosign first |
| FR-70 | [#109](https://github.com/e8kor/open-memory-lab/issues/109) | Measure request-log, metrics and trace overhead |
| FR-71 | [#109](https://github.com/e8kor/open-memory-lab/issues/109) | Measure request-log, metrics and trace overhead |
| FR-72 | [#143](https://github.com/e8kor/open-memory-lab/issues/143) | Move existing shared workareas into a person's own workarea |
| FR-73 | [#144](https://github.com/e8kor/open-memory-lab/issues/144) | Flag a curated memory that still points at its source |
| FR-74 | [#145](https://github.com/e8kor/open-memory-lab/issues/145) | Measure shared-area search displacement and brief size |
| FR-75 | [#131](https://github.com/e8kor/open-memory-lab/issues/131) | Record when a key was last used |
| FR-76 | [#119](https://github.com/e8kor/open-memory-lab/issues/119) | Verify sign-in with Google and GitHub on a live server |
| FR-77 | [#145](https://github.com/e8kor/open-memory-lab/issues/145) | Measure shared-area search displacement and brief size |
| FR-78 | [#142](https://github.com/e8kor/open-memory-lab/issues/142) | Project knowledge and code skeleton: evaluation, caps and branch workareas |
| FR-79 | [#142](https://github.com/e8kor/open-memory-lab/issues/142) | Project knowledge and code skeleton: evaluation, caps and branch workareas |
| FR-80 | [#142](https://github.com/e8kor/open-memory-lab/issues/142) | Project knowledge and code skeleton: evaluation, caps and branch workareas |
| FR-81 | [#102](https://github.com/e8kor/open-memory-lab/issues/102) | Publish search quality and latency at stated corpus sizes |
| FR-82 | [#102](https://github.com/e8kor/open-memory-lab/issues/102) | Publish search quality and latency at stated corpus sizes |
| FR-83 | [#105](https://github.com/e8kor/open-memory-lab/issues/105) | Let recall answer "nothing relevant" |
| FR-84 | [#102](https://github.com/e8kor/open-memory-lab/issues/102) | Publish search quality and latency at stated corpus sizes |
| FR-85 | [#102](https://github.com/e8kor/open-memory-lab/issues/102) | Publish search quality and latency at stated corpus sizes |
| FR-86 | [#100](https://github.com/e8kor/open-memory-lab/issues/100) | Concurrent writes in one organisation meet 503 chain-head contention |
| FR-87 | [#98](https://github.com/e8kor/open-memory-lab/issues/98) | Measure review throughput and queue drain |
| FR-88 | [#102](https://github.com/e8kor/open-memory-lab/issues/102) | Publish search quality and latency at stated corpus sizes |
| FR-89 | [#105](https://github.com/e8kor/open-memory-lab/issues/105) | Let recall answer "nothing relevant" |
| FR-90 | [#141](https://github.com/e8kor/open-memory-lab/issues/141) | Remember: duplicate-check and advisory-warning thresholds |
| FR-91 | [#141](https://github.com/e8kor/open-memory-lab/issues/141) | Remember: duplicate-check and advisory-warning thresholds |
| FR-92 | [#141](https://github.com/e8kor/open-memory-lab/issues/141) | Remember: duplicate-check and advisory-warning thresholds |
| FR-93 | [#113](https://github.com/e8kor/open-memory-lab/issues/113) | Measure duplicate-write rate and stale-memory confirmation |
| FR-94 | [#141](https://github.com/e8kor/open-memory-lab/issues/141) | Remember: duplicate-check and advisory-warning thresholds |
| FR-100 | [#103](https://github.com/e8kor/open-memory-lab/issues/103) | Measure postings fill, statistics upkeep and migration locks at scale |
| FR-101 | [#104](https://github.com/e8kor/open-memory-lab/issues/104) | Measure the learned sparse encoder's cost and bound its recall leg |
| FR-102 | [#100](https://github.com/e8kor/open-memory-lab/issues/100) | Concurrent writes in one organisation meet 503 chain-head contention |
| FR-103 | [#103](https://github.com/e8kor/open-memory-lab/issues/103) | Measure postings fill, statistics upkeep and migration locks at scale |
| FR-104 | [#110](https://github.com/e8kor/open-memory-lab/issues/110) | Does recall by usage rank better? |
| FR-105 | [#110](https://github.com/e8kor/open-memory-lab/issues/110) | Does recall by usage rank better? |
| FR-106 | [#110](https://github.com/e8kor/open-memory-lab/issues/110) | Does recall by usage rank better? |
| FR-107 | [#110](https://github.com/e8kor/open-memory-lab/issues/110) | Does recall by usage rank better? |
| FR-108 | [#110](https://github.com/e8kor/open-memory-lab/issues/110) | Does recall by usage rank better? |
| FR-109 | [#159](https://github.com/e8kor/open-memory-lab/issues/159) | Claim formats: measure the subset parser, the bundle, write cost and quality thresholds |
| FR-110 | [#159](https://github.com/e8kor/open-memory-lab/issues/159) | Claim formats: measure the subset parser, the bundle, write cost and quality thresholds |
| FR-111 | [#159](https://github.com/e8kor/open-memory-lab/issues/159) | Claim formats: measure the subset parser, the bundle, write cost and quality thresholds |
| FR-112 | [#159](https://github.com/e8kor/open-memory-lab/issues/159) | Claim formats: measure the subset parser, the bundle, write cost and quality thresholds |
| FR-113 | [#159](https://github.com/e8kor/open-memory-lab/issues/159) | Claim formats: measure the subset parser, the bundle, write cost and quality thresholds |
| FR-114 | [#138](https://github.com/e8kor/open-memory-lab/issues/138) | Sync follow-ups: origin filters, bulk review size and withdrawn edges |
| FR-115 | [#138](https://github.com/e8kor/open-memory-lab/issues/138) | Sync follow-ups: origin filters, bulk review size and withdrawn edges |
| FR-116 | [#138](https://github.com/e8kor/open-memory-lab/issues/138) | Sync follow-ups: origin filters, bulk review size and withdrawn edges |
| FR-117 | [#138](https://github.com/e8kor/open-memory-lab/issues/138) | Sync follow-ups: origin filters, bulk review size and withdrawn edges |
| FR-118 | [#104](https://github.com/e8kor/open-memory-lab/issues/104) | Measure the learned sparse encoder's cost and bound its recall leg |
| FR-119 | [#103](https://github.com/e8kor/open-memory-lab/issues/103) | Measure postings fill, statistics upkeep and migration locks at scale |
| FR-130 | [#160](https://github.com/e8kor/open-memory-lab/issues/160) | Code sync: review cost, public-surface staleness and dependency edges |
| FR-131 | [#160](https://github.com/e8kor/open-memory-lab/issues/160) | Code sync: review cost, public-surface staleness and dependency edges |
| FR-132 | [#160](https://github.com/e8kor/open-memory-lab/issues/160) | Code sync: review cost, public-surface staleness and dependency edges |
| FR-133 | [#160](https://github.com/e8kor/open-memory-lab/issues/160) | Code sync: review cost, public-surface staleness and dependency edges |
| FR-140 | [#161](https://github.com/e8kor/open-memory-lab/issues/161) | Lake sync: measure build size, reader coverage, request cost and import throughput |
| FR-141 | [#161](https://github.com/e8kor/open-memory-lab/issues/161) | Lake sync: measure build size, reader coverage, request cost and import throughput |
| FR-142 | [#161](https://github.com/e8kor/open-memory-lab/issues/161) | Lake sync: measure build size, reader coverage, request cost and import throughput |
| FR-143 | [#161](https://github.com/e8kor/open-memory-lab/issues/161) | Lake sync: measure build size, reader coverage, request cost and import throughput |
| FR-144 | [#161](https://github.com/e8kor/open-memory-lab/issues/161) | Lake sync: measure build size, reader coverage, request cost and import throughput |
| FR-145 | [#161](https://github.com/e8kor/open-memory-lab/issues/161) | Lake sync: measure build size, reader coverage, request cost and import throughput |
| FR-146 | [#161](https://github.com/e8kor/open-memory-lab/issues/161) | Lake sync: measure build size, reader coverage, request cost and import throughput |
| FR-147 | [#161](https://github.com/e8kor/open-memory-lab/issues/161) | Lake sync: measure build size, reader coverage, request cost and import throughput |
| FR-160 | [#111](https://github.com/e8kor/open-memory-lab/issues/111) | Chat memory evals: agent extraction against turn storage |
| FR-161 | [#139](https://github.com/e8kor/open-memory-lab/issues/139) | Chat memory follow-ups: sessions ending between captures, two machines |
| FR-162 | [#111](https://github.com/e8kor/open-memory-lab/issues/111) | Chat memory evals: agent extraction against turn storage |
| FR-163 | [#139](https://github.com/e8kor/open-memory-lab/issues/139) | Chat memory follow-ups: sessions ending between captures, two machines |
| FR-164 | [#111](https://github.com/e8kor/open-memory-lab/issues/111) | Chat memory evals: agent extraction against turn storage |
| FR-170 | [#344](https://github.com/e8kor/open-memory-lab/issues/344) | Tracking: implicit capture of a repository's knowledge (ADR-0064), re-ported from #97 |
| FR-171 | [#344](https://github.com/e8kor/open-memory-lab/issues/344) | Tracking: implicit capture of a repository's knowledge (ADR-0064), re-ported from #97 |
| FR-172 | [#344](https://github.com/e8kor/open-memory-lab/issues/344) | Tracking: implicit capture of a repository's knowledge (ADR-0064), re-ported from #97 |
| FR-173 | [#344](https://github.com/e8kor/open-memory-lab/issues/344) | Tracking: implicit capture of a repository's knowledge (ADR-0064), re-ported from #97 |
| FR-174 | [#344](https://github.com/e8kor/open-memory-lab/issues/344) | Tracking: implicit capture of a repository's knowledge (ADR-0064), re-ported from #97 |
| FR-175 | [#344](https://github.com/e8kor/open-memory-lab/issues/344) | Tracking: implicit capture of a repository's knowledge (ADR-0064), re-ported from #97 |
| FR-176 | [#344](https://github.com/e8kor/open-memory-lab/issues/344) | Tracking: implicit capture of a repository's knowledge (ADR-0064), re-ported from #97 |
| FR-180 | [#140](https://github.com/e8kor/open-memory-lab/issues/140) | Scanner: personal-data rules, rescan window and veto evidence |
| FR-181 | [#140](https://github.com/e8kor/open-memory-lab/issues/140) | Scanner: personal-data rules, rescan window and veto evidence |
| RS-1 | [#153](https://github.com/e8kor/open-memory-lab/issues/153) | Choose the embedding model and where it runs |
| RS-3 | [#151](https://github.com/e8kor/open-memory-lab/issues/151) | Federation across tenants |
| RS-4 | [#148](https://github.com/e8kor/open-memory-lab/issues/148) | Dataset onboarding: first slice, Merkle construction, sampling and connectors |
| RS-5 | [#148](https://github.com/e8kor/open-memory-lab/issues/148) | Dataset onboarding: first slice, Merkle construction, sampling and connectors |
| RS-6 | [#148](https://github.com/e8kor/open-memory-lab/issues/148) | Dataset onboarding: first slice, Merkle construction, sampling and connectors |
| RS-7 | [#148](https://github.com/e8kor/open-memory-lab/issues/148) | Dataset onboarding: first slice, Merkle construction, sampling and connectors |
| RS-8 | [#148](https://github.com/e8kor/open-memory-lab/issues/148) | Dataset onboarding: first slice, Merkle construction, sampling and connectors |
| RS-9 | [#152](https://github.com/e8kor/open-memory-lab/issues/152) | Anchor chain heads outside the deployment |
| RS-10 | [#149](https://github.com/e8kor/open-memory-lab/issues/149) | Vocabularies as governed claims |
| RS-11 | [#148](https://github.com/e8kor/open-memory-lab/issues/148) | Dataset onboarding: first slice, Merkle construction, sampling and connectors |
| RS-12 | [#154](https://github.com/e8kor/open-memory-lab/issues/154) | Keyring mechanism, read-log split and DEK cache shape |
| RS-13 | [#154](https://github.com/e8kor/open-memory-lab/issues/154) | Keyring mechanism, read-log split and DEK cache shape |
| RS-14 | [#154](https://github.com/e8kor/open-memory-lab/issues/154) | Keyring mechanism, read-log split and DEK cache shape |
| RS-15 | [#155](https://github.com/e8kor/open-memory-lab/issues/155) | Per-tenant full-text-search configuration |
| RS-16 | [#140](https://github.com/e8kor/open-memory-lab/issues/140) | Scanner: personal-data rules, rescan window and veto evidence |
| RS-17 | [#156](https://github.com/e8kor/open-memory-lab/issues/156) | Let an author revise its own claim while it awaits review |
