---
status: Beta
verified: 2026-11-14
answers: "how a workarea moves between servers or into a copy on one server: what a bundle carries and never carries, how trust is re-established on the target, how an import is rolled back, where export and import sit in the lifecycle, and what is built today"
---

# Import and export

A workarea leaves a server as a published release in a signed, encrypted bundle. It lands on the target held, in an in-progress development workarea, and is reviewed again there before any agent outside that workarea reads it. Nothing in this flow is built. It is decided in [ADR-0084](../adr/ADR-0084-a-workarea-moves-between-servers-as-a-signed-bundle-that-lands-held-or-in-progress-and-is-reviewed-again-unless-its-source-server-is-pinned.md), and builds after releases ([ADR-0085](../adr/ADR-0085-a-workarea-publishes-immutable-releases-that-one-person-cuts-and-another-ratifies-and-each-key-binds-to-the-tip-the-latest-release-or-a-pinned-one.md)).

<figure markdown="span">
  [![Export and import view: on the source server, payments.dev in progress, a published release payments.dev@1.4.0 and an export by a person's key; the oml-bundle/1 file carries claim versions of every kind, verdicts as provenance under pseudonyms, signed source events and a signed manifest, encrypted to named recipients, and never keys, grants, crews, bindings or settings. On the target, an import lands held in an in-progress workarea, a person reviews it again, two persons publish a release there, and an import rollback rejects or, as an admin fix, demotes. On one server, a copy streams from a release. All designed.](../architecture/img/exchange-light.svg#only-light)](../architecture/img/exchange.svg)
  [![Export and import view: on the source server, payments.dev in progress, a published release payments.dev@1.4.0 and an export by a person's key; the oml-bundle/1 file carries claim versions of every kind, verdicts as provenance under pseudonyms, signed source events and a signed manifest, encrypted to named recipients, and never keys, grants, crews, bindings or settings. On the target, an import lands held in an in-progress workarea, a person reviews it again, two persons publish a release there, and an import rollback rejects or, as an admin fix, demotes. On one server, a copy streams from a release. All designed.](../architecture/img/exchange-dark.svg#only-dark)](../architecture/img/exchange.svg)
  <figcaption>Every element is designed (ADR-0084, ADR-0085, ADR-0089). A target never takes the source's verdicts as its own: trusting a source server is ADR-0088, proposed and deferred.</figcaption>
</figure>

The reviewer pairs that count on the target are those of [producers and reviewers](../concepts/producers-and-reviewers.md): every imported version is a new write by the importing person.

## Place in the lifecycle

Export starts from a **published release**, the last step on the source ([workarea lifecycle](../architecture/lifecycle.md)). Import ends in a **development workarea in progress** on the target, the first step there. A move between servers therefore runs the whole review on the target again: import, review, cut, publish.

| Path | When | What it costs |
|---|---|---|
| One server, releases only | Development and production on the same server, separated by bindings | No copies and one review. Recommended by the lifecycle design as the default. |
| Same-server copy | A second development line seeded from a release | One streamed import; the source's authors cannot review the copy, because `derives_from` is kept. |
| Two servers, export and import | Staging and production on separate servers, keys and people | A second review per move, a second server to run, and drift between the two to watch. |

## What moves and what never moves

| Moves | Never moves |
|---|---|
| Claim versions of every kind: memories, steering, terms, guardrails, skills, documents, tests | Keys, connect codes, connections |
| Each version's format, anchor, valid time, kind fields, `derives_from` and `origin`, as signed | Grants, denies, crews, quotas, review policy |
| Verdicts, as provenance: the reviewer's kind, `same_user`, the approval time, and a pseudonym | Bindings, release gate records, workarea mode, federation settings |
| Each version's and verdict's source journal event, as signed | Other people's held, rejected or demoted versions |
| Edges whose two ends are both exported | The salt behind the pseudonyms |
| Attachments and bodies over 64 KiB, by digest | |

`release` mode, the default, carries one release's members. `history` mode carries every version ever promoted, and the exporter's own held, rejected and demoted versions. An erased version travels as a marker with no digest, so a bundle holder cannot confirm a guessed text (ADR-0084 d2, d3, d13).

**Signatures and keys.** The manifest is signed by the source's journal key; the signing key itself never leaves. The bundle is encrypted to at least one named age recipient, or the export is refused `bundle.unencrypted`. The server never holds a recipient's identity (ADR-0084 d5).

## How trust is re-established on the target

1. **Verify before writing.** The importer's CLI and the server check the layout, paths, sizes, digests, the manifest signature and each source event's signature. Each failure has its own code, such as `bundle.signature_invalid` or `bundle.path_unsafe`. An import that would not fit the target's quota is refused whole as `import.over_quota` before the first write (ADR-0084 d7).
2. **Name the expected key.** `--expect-key <fingerprint>`, obtained out of band, refuses a manifest signed by any other key. Without it the signature proves integrity only, and the plan says so.
3. **Scan again.** Each version passes the target's scan rule set, which may be newer than the source's. A veto skips the version and names it; a hold keeps it author-only (ADR-0084 d11).
4. **Land held.** The importing person's writer key becomes the author of every version on the target. The source's verdicts show on the review card as evidence and count for nothing (ADR-0084 d4, d6).
5. **Review again.** A person on the target judges each item, one act for a listed set allowed. Approving an imported version attests "did not author this on the source", journalled with the verdict, because a person with keys on both servers cannot be recognised across them (ADR-0084 d4).
6. **Publish on the target.** One person cuts a release and another publishes it. Keys bound to `latest` receive it then (ADR-0085 d4).

Until step 6, only `tip` keys inside the in-progress target read the imported content, marked unreviewed.

**Who acts.** A person's key exports, under the tenant's export policy: `off`, `own_root` (the default: a person's own root only) or `allowed`. An admin may allow one shared workarea's export, which reads no content. No agent, admin or service key exports, and only a person's writer key imports (ADR-0084 d10, d11). No MCP tool exists for either (R1).

**A repeated import** adds only what is new. Each claim key is planned as `oml sync` plans it: new, changed, removed, unchanged or in conflict. A conflict never writes over the target's tip; it lands as a held supersession marked `conflict`. An older bundle is refused `import.bundle_older` unless `--allow-older` is given, and `--dry-run` prints the plan and writes nothing (ADR-0084 d8).

## Rollback

- **Before review.** Reject the import's held versions. The bytes are kept.
- **After review, through the team.** Supersede or retire what was promoted, through review, and cut a new release.
- **At once, through an admin.** An admin rolls the import back as a fix: each promoted version it brought is demoted, restoring the tip it replaced, and each claim key it created is retired. Release-bound keys see the result as a marked overlay. A person confirms or reverts the fix afterwards ([ADR-0089](../adr/ADR-0089-an-admin-fixes-a-production-workarea-at-once-by-a-marked-break-glass-act-that-a-person-confirms-or-reverts-afterwards.md) d2, d5, d8).

Nothing is deleted on either server. A later bundle that carries an erasure marker raises an `import.erasure_notice` on the target, which the target's own erasure clears (ADR-0084 d13).

## Commands

Designed (ADR-0084 d15):

```sh
# on the source, as a person
oml workareas export payments.dev --release 1.4.0 --recipient age1… --out payments-1.4.0.oml
# on the target, as a person with writer there
oml workareas import payments-1.4.0.oml --into payments.dev --identity key.txt --expect-key <fingerprint> --dry-run
oml workareas import payments-1.4.0.oml --into payments.dev --identity key.txt --expect-key <fingerprint>
# a same-server copy
oml workareas import --from payments.dev@1.4.0 --into payments.copy
```

The console's personal side gains Export and Import pages. The HTTP routes are `POST /v2/workareas/{workarea}/exports`, `POST /v2/imports` and `GET /v2/imports/{id}`.

## Built today

| Need | Built path |
|---|---|
| A person's own data out of a server | The console's export, `GET /v2/web/export`: everything in the person's root as one JSON file. Nothing imports it. |
| Repository documents into a workarea | [`oml sync`](../use/sync.md), which plans, writes held and proposes changes, as an import will. |
| Guardrails for a harness without the plugin | [`oml guardrails export`](../use/guardrails.md): promoted text as a file, labelled "text, not enforcement". |
| A whole server | [Back up and restore](../operations/restore.md): the database and the keyring together. A bundle is never a backup and never a tenant move (ADR-0084 d16). |
