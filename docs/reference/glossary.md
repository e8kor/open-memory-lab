---
status: Stable
verified: 2026-11-06
answers: "the terms the OML docs use and what each means"
---

# Glossary

| Term | Meaning |
|---|---|
| **memory** | One statement an agent or person wrote. OML records who approved it; it does not vouch for it. The word "claim" appears only in `claim_key` and in command help. |
| **claim key** | The name that groups the versions of one memory. The server derives one when you send none. |
| **held** | A memory waiting for review. Only its author can read it, and only by asking for unreviewed memories. |
| **approve / reject** | A reviewer's verdict on a held memory. The stored state after approval is `promoted`; the other states are `demoted`, `rejected` and `erased`. |
| **ratify** | A second identity confirming a proposed workarea status change or edge. Not used for memories. |
| **unreviewed** | The label recall puts on held memories inside an in-progress workarea and on your own held memories. |
| **workarea** | A node in the tree where memory lives and grants apply. The root is `universe`. |
| **key** | An API key with one role: `reader`, `writer`, `dreamer`, `admin` or `operator`. |
| **reviewer key** | A key with the role `dreamer`. It judges held memories and nothing else. |
| **crew** | A named record of the agents that work together. OML records it; it does not run them. |
| **tenant** | One organization's isolated data on a server. The server derives it from the key. |
| **lead** | The server instance that accepts writes. This build runs one lead and no followers. |
| **journal** | The signed, hash-chained record of every write, verdict and key issuance. |
| **brief** | The memories a workarea's reviewers chose for an agent to read first when it enters. |
| **steering** | A memory of a kind that tells agents how to work. Recall returns it before hits. |
| **tip** | The current approved version of a memory. |
| **recall** | A ranked search over approved memories with `oml read --query` or `oml_read`. |
| **self-signed** | A memory approved by its own author on a development home. It counts once and is never called a review. |
