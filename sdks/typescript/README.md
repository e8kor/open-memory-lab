# oml-sdk — the TypeScript client

A client for the Open Memory Lab HTTP API: types generated from
[`docs/api/openapi.yaml`](https://e8kor.github.io/open-memory-lab/api/openapi.yaml) by a pinned `openapi-typescript`,
`openapi-fetch` typed by them, and a thin hand-written wrapper. What the wrapper covers, the pins
and the regeneration rule are decided in [docs/engineering/sdks.md](https://e8kor.github.io/open-memory-lab/engineering/sdks/).

Install it from npm, Node 24 or later:

```sh
npm install oml-sdk
```

From a checkout of the repository, which is how to install it until the first release is on npm
and how to try a change that is not released, build it, then install it from that directory:

```sh
cd sdks/typescript && npm ci && npm run build
cd /your/project && npm install /path/to/open-memory-lab/sdks/typescript
```

```ts
import { OmlClient } from "oml-sdk";

// One pair: url and apiKey, else OML_URL and OML_API_KEY; one set alone is refused.
const oml = new OmlClient();
const written = await oml.write({ text: "The canary stage must drain before the next one starts." });
console.log(written.value.state); // held: invisible to other keys until a reviewer's verdict
console.log(written.value.claim_key); // derived by the server when none is sent
console.log(written.idempotencyKey); // the key every retry of this call sent

const { hits } = await oml.read({ query: "canary stage", include_unreviewed: true });
for (const hit of hits) console.log(hit.memory.state, hit.memory.text);

const memory = await oml.read({ artifact_id: written.value.artifact_id }); // own row, any state
console.log(memory.state, memory.text);

// replaces must name the served tip, so a reviewer's approve promotes the version first:
// superseding it while it is held rejects with SubmissionError (submission.tip_conflict).
await oml.write({ text: "Drain, then wait five minutes.", replaces: memory.artifact_id });

for await (const row of oml.memories({ limit: 50 })) {
  console.log(row.artifact_id, row.state); // the key's own rows; follows next_cursor to the end
}
```

Seven verbs: `write`, `read`, `workarea`, `events`, `reviewNext`, `approve` and `reject`.
`write`'s fields choose a submission or, with `replaces`, a supersession; `read`'s fields choose
a fetch (`artifact_id`), a recall (`query`) or the listing (neither), as `oml_write` and
`oml_read` do. Every refusal rejects with `ApiError` carrying the envelope of
[errors.md](https://e8kor.github.io/open-memory-lab/api/errors/) — `code`, `message`, `rule`, `retryable`, `details`,
`requestId` — and `status`, as the subclass of the code's family; a field outside its mode is
refused the same way before anything is sent.

`src/generated/schema.ts` is generated and never edited by hand; `python3 scripts/generate_sdks.py`
regenerates it. The tests run against a real `oml-server-lead` on a throwaway database
(`test/harness.ts` names the environment they read), on Node 24. The harness mints an owner
and a serving role for that database, so its admin URL names a superuser that can mint roles;
under a role that cannot, the server tests skip with `did not run: <role> cannot create roles`:

```sh
cd sdks/typescript && npm ci && npm run check && npm test
```
