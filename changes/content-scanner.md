### 2026-11-05 — the deterministic scan's rule set (ADR-0065, proposed)

- **[ADR-0065](adr/ADR-0065-the-scan-is-a-versioned-compiled-in-rule-set-that-vetoes-leaked-keys-and-holds-what-a-reviewer-should-judge.md)** replaces the marker scanner with `oml-scan/1`, a compiled-in,
  versioned rule set in a leaf crate: provider tokens, key-value secrets above an entropy floor,
  URL credentials and private keys veto; instructions aimed at the reader, bidirectional and
  invisible Unicode and long base64 runs hold; personal-data rules exist and stay off. One
  secret-shaped path list serves the anchor scan and `oml sync`. A finding names its class, rule,
  field and line, never what it matched. `scan.scanner_version` carries the rule set, and the
  findings metric counts by rule. `/readyz` and `oml doctor` name the rule set, its rule count
  and the personal-data setting; `GET /v2/admin/rescan` and `oml admin rescan` report what the
  rule set finds in versions already stored, by rule and place, changing nothing (d13). The
  per-tenant personal-data setting is FR-180, and the rescan's window is FR-181.
- After review: each rule runs as its own regex, with the base64 and AWS-pair rules counted by
  hand, so a mebibyte scans in about 13 ms rather than 5 s, and a field over 64 KiB is scanned off
  the async worker. Claim keys and verdict reasons are scanned (d7). Key assignments need an
  operator and a value that is not a written identifier. JSON `Authorization` headers,
  `curl -u`, legacy `sk-` keys and a bare AWS secret beside its key id are vetoed. Provider tokens
  glued to a word are found. The veto rules also read a normalised view (d14). The gitleaks
  notice ships as `LICENSES/gitleaks-MIT.txt`. The path list no longer refuses `id_generator.rs`,
  `.env.example` or this record, which was renamed so its file name holds no `secret`.
- Pages touched: [admission](architecture/admission.md) (the registry's new classes and the
  measured pass), [errors](api/errors.md) (`details.finding`'s shape),
  [workspace plan](engineering/workspace-plan.md) (the second leaf crate),
  [HTTP API](api/http-api.md) and [OpenAPI](api/openapi.yaml) (the rescan route and `/readyz`'s
  `scan`), and the [roadmap](product/roadmap.md) (FR-180, FR-181).
