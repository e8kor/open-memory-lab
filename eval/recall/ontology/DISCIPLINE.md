# How the vocabulary and the tags were written

Committed before `run.py` here existed and before `queries.json` was opened in this lane, so the
commit that adds this file is the proof of order: its parent has no ontology code and its tree
holds the vocabulary and the tags exactly as measured.

- **Who wrote them.** One model (the same family that wrote the corpus, the queries and the
  labels of `eval/recall/`), in one pass, playing a curator for the vocabulary and the writing
  agent for the tags.
- **What it read.** `corpus.json` in full (a curator knows the content its vocabulary
  describes, and a writing agent knows the memory it writes). It did not open `queries.json`.
  It had read `REPORT.md` and ADR-0048's context before, which quote a handful of query
  fragments (`autoscal`, `otel`, `2fa`, a deploy-window query, `fct_mrr_daily`); those words were
  therefore known, and `otel` and `2fa` are alternative labels below. That is a leak of five
  fragments out of 121 queries, stated rather than hidden.
- **The vocabulary.** 80 terms in four facets (component 11, technology 26, topic 36,
  process 7), each with one preferred label and alternative labels written as a curator would:
  abbreviations, product names, plural and colloquial forms. No label was tuned after a run.
- **The tags.** 0 to 3 terms per memory, chosen from the vocabulary for what the memory is
  about, not for every word it mentions. All 200 memories got at least one term; every term is
  used at least once.
