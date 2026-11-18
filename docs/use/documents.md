---
status: Stable
verified: 2026-11-18
answers: "what a document is in OML, how one is written and approved, how recall finds it by passage, how an MCP client reads it as a resource, and what oml sync's document adapter loads"
---

# Keep documents in OML

A document is a reviewed markdown or plain-text body read as a whole, such as a runbook, a guide or
a policy. It is a memory of kind `document`: a title and a body of at most 1 MiB. Recall finds it
one passage at a time, and an MCP client reads it, or one passage of it, as a resource, without a
copy on disk ([ADR-0081](../adr/ADR-0081-a-document-is-a-reviewed-markdown-or-text-claim-that-recall-searches-by-passage-and-the-mcp-server-serves-as-a-resource.md)).
It is data, as a memory is: the workspace's review policy applies, and its text is never an
instruction to follow.

- **Writing one.** `oml_write` with `kind: "document"`, the text, `format` `text/markdown` (the
  default for the kind) or `text/plain`, and an optional `document: {title}`; or
  `POST /v1/memories` with the same fields. Without a title, a markdown body's first level-1
  heading is the title, else the claim key's last segment. The claim key is `doc/` and a path of
  segments of `a-z 0-9 . _ -`, at most 64 characters each and 128 in all; without one, the server
  derives `doc/<slug of the title>`.
- **Passages.** The server cuts the text when it is written: each heading of level 1 to 4 with
  its text, plus any text before the first heading, and any passage longer than 40 lines in
  blocks of at most 40. A heading with no text of its own joins the headings of the passages
  below it. A text that cuts into more than 4,096 passages is refused, `request.validation` with
  `details.reason: too_many_passages`; nothing is written.
- **Changing one.** `oml_write` with `replaces` and the whole new text. The approved version stays
  in use until the new one is approved, so a one-word change asks for a review of the whole
  document.

## Recall by passage

A recall ranks each passage as a unit of its own, by the document's title, the headings above
the passage and its lines, and serves the document once, as its best passage. The hit's `text` is
that passage's lines and its tokens are the passage's, so `k` and `max_tokens` count it as one
short hit. The hit also carries:

- `passage`: its `fragment` (the heading's slug, or `L<n>-L<m>` for a block), `ordinal`,
  `start_line`, `end_line` and `heading_path`;
- `more_passages`: how many of the document's other passages were among the candidates, a lower
  bound;
- `uri`: `oml://workspace/<workspace>/doc/<path>?passage=<fragment>`. The fragment is the
  heading's slug when that is plain `a-z`, `0-9`, `-` and `_`; a heading in another script, an
  emoji or punctuation alone is named by its lines, `L<n>-L<m>`, so every URI can be read back.

`k` counts documents, not passages: a document whose 30 passages match takes one of the `k`
places. A document adds at most one to a word's document frequency, so a long document that
repeats a word does not make that word worthless for the memories beside it.

A recall without `kinds` includes documents; `kinds: ["memory", "derived_fact"]` leaves them out,
and `kinds: ["document"]` asks for documents alone.

## Resources

The MCP server lists the documents at the key's default workspace in `resources/list`, newest
first, 50 a page, before the offered skills. Each entry names the document's URI, title, format and
size, and says where it lives and who approved it. `resources/read` on a document's URI answers its
text byte for byte; with `?passage=<fragment>` or `?passage=L<n>-L<m>` it answers that passage's
lines. A retired document is neither listed nor read. Provenance travels
in `_meta.oml`, never inside the text. A URI the key cannot read answers `-32002` "Resource not
found", as a URI that names nothing does.

## Held documents

A document waits for review like any other claim. While it is held, the keys allowed to see a held row (its author's, and
the keys an in-progress workspace admits) find it in recall and in `resources/list`, marked
"Unreviewed", at a URI that names its version, `?version=<n>`. Once a document is approved, an edit
of it is held beside the approved version: `resources/list` keeps one entry, the approved version,
and names the held edit and its `?version=<n>` URI in the description. The bare URI reads the
approved version; `?version=<n>` reads the edit for a key allowed to see it, and answers
`-32002` for any other.

## Loading files with oml sync

`oml sync --adapter document` loads each `.md` or `.txt` file as one document, keyed `doc/` and
the path without its extension, and anchored to the file. A path is loaded one way: a run refuses
a path the workspace already holds through another adapter, naming both, until
`--change-adapter` retires the other claims. [Sync](sync.md) covers the rest of the command.

## oml doctor

`oml doctor` warns when the server predates documents, when a path of the clone is loaded both as
a document and by another adapter, when a document version has no passages yet, and when a document's file now cuts into more than 4,096
passages, so its next version would be refused.
