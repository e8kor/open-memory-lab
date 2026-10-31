# oml-sdk — the TypeScript client

A client for the Open Memory Lab HTTP API: types generated from
[`docs/api/openapi.yaml`](../../docs/api/openapi.yaml) by a pinned `openapi-typescript`,
`openapi-fetch` typed by them, and a thin hand-written wrapper. What the wrapper covers, the pins
and the regeneration rule are decided in [docs/engineering/sdks.md](../../docs/engineering/sdks.md).

It is not released to npm. Build it from a checkout, then install it from that directory:

```sh
cd sdks/typescript && npm ci && npm run build
cd /your/project && npm install /path/to/open-memory-lab/sdks/typescript
```

```ts
import { OmlClient } from "oml-sdk";

// The key comes from apiKey, else OML_API_KEY; the server from baseUrl, else OML_URL.
const oml = new OmlClient();
const written = await oml.remember({
  claim_key: "deploy-runbook",
  body: "The canary stage must drain before the next one starts.",
});
console.log(written.value.state); // held: invisible to other keys until a reviewer's verdict
console.log(written.idempotencyKey); // the key every retry of this call sent

const { hits } = await oml.recall({ query: "canary stage", include_own_held: true });
for (const hit of hits) console.log(hit.memory.state, hit.memory.body);

for await (const row of oml.memories({ own: true, limit: 50 })) {
  console.log(row.artifact_id, row.state); // follows next_cursor to the end
}
```

Refusals reject with one class per family of [errors.md](../../docs/api/errors.md), all
subclasses of `ApiError`, each carrying `code`, `rule`, `message`, `retryable`, `details`,
`requestId` and `status`. File uploads reject with `NotBuiltError`: the server serves no upload
sessions yet.

`src/generated/schema.ts` is generated and never edited by hand; `python3 scripts/generate_sdks.py`
regenerates it. The tests run against a real `oml-server-lead` on a throwaway database
(`test/harness.ts` names the environment they read), on Node 24:

```sh
cd sdks/typescript && npm ci && npm run check && npm test
```
