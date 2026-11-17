**The workspace bundle format is built (#425, #426).** A new leaf crate, `oml-bundle`, builds
`oml-bundle/1` from [ADR-0084](adr/ADR-0084-a-workspace-moves-between-servers-as-a-signed-bundle-that-lands-held-or-in-progress-and-is-reviewed-again-unless-its-source-server-is-pinned.md).
It covers:

- the fixed tar layout and the manifest;
- the Ed25519 manifest signature under the `oml-bundle/1\n` prefix, with `--expect-key`;
- age encryption to named recipients, where no recipient is `bundle.unencrypted`;
- verification that fails closed with d7's codes;
- erasure markers that carry no digest;
- per-bundle pseudonyms;
- the d8 import plan as a pure function.

ADR-0084 records the operator's decision of 2026-11-17 that only the exporter's own versions carry
their signed source event, and nine readings the record left open. The readings cover the
container, order, manifest and line fields, `bundle.malformed`, `bundle.decrypt_failed`, markers,
pseudonyms, limits and plan rules. [Bundle format](data/bundle-format.md) describes the file. The
export and import routes, CLI verbs and console pages remain designed, not built. Adds the `age`,
`tar` and `hmac` crates.
