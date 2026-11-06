//! The listing cursors (http-api.md pagination): opaque keyset positions,
//! base64url (no padding) over JSON — the memory listing's
//! `{key_id, created_at, artifact_id, issued_at}` (ADR-0025 d13), the
//! crew listing's `{key_id, after_name, issued_at}` (ADR-0026 d8), and the
//! workarea tree's `{key_id, after_path, issued_at}` (ADR-0026 d15), and the
//! claim-edge read's `{key_id, created_at, edge_id, workarea_id,
//! artifact_id, issued_at}`. A cursor carries a position — every page
//! re-applies the listing's predicates — so a client that forges one for its
//! own key learns nothing a fresh first page would not. It answers only the
//! key it was issued to, only within `api.cursor_ttl` of its issue, and the
//! claim-edge cursor only the filter it was minted under.

