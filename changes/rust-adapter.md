`oml sync --adapter rust` (ADR-0061, accepted for v0.6.1) reads a Cargo workspace at a commit as one
claim per crate, from its manifest and root doc, and one per module, from the `//!` doc at the head
of its file, with line-span anchors and the hashers `oml-crate/1` and `oml-rustdoc/1`, which the
server now admits. A code edit marks nothing stale and a doc edit marks exactly its claim; the
adapter retires only claims in the `rust/` namespace and never a claim whose file it could not
read. The adapter port gains a set extraction; the CLI gains no runtime dependency. Pages: the CLI
reference, ADR-0061, the HTTP API's `Anchor.hasher`, the SDKs' generated models.
