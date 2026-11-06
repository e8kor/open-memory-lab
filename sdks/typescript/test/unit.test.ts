// The wrapper's rules that need no server (docs/engineering/sdks.md).

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { inspect } from "node:util";
import { beforeEach, test } from "node:test";

import {
  ApiError,
  ConfigError,
  FAMILIES,
  type MemoryPage,
  OmlClient,
  type ReadInput,
  RequestError,
  SubmissionError,
  USER_AGENT,
  type WriteInput,
  errorFromResponse,
} from "../src/index.ts";

const ERRORS_MD = new URL("../../../docs/api/errors.md", import.meta.url);

beforeEach(() => {
  delete process.env.OML_API_KEY;
  delete process.env.OML_URL;
});

/** Every family a code in an errors.md family table belongs to. */
function declaredFamilies(): Set<string> {
  const families = new Set<string>();
  let inFamilyTable = false;
  for (const line of readFileSync(ERRORS_MD, "utf8").split("\n")) {
    if (line.startsWith("| Code | HTTP |")) inFamilyTable = true;
    else if (!line.startsWith("|")) inFamilyTable = false;
    else if (inFamilyTable) {
      const match = /^\| `([a-z_]+)\.[a-z_]+` \|/.exec(line);
      if (match?.[1]) families.add(match[1]);
    }
  }
  return families;
}

function envelope(code: string): string {
  return JSON.stringify({
    error: { code, message: "m", rule: "r", retryable: false },
    request_id: "req_1",
  });
}

function recorder(status = 500, answer: unknown = null) {
  const requests: Request[] = [];
  const fetcher = async (request: Request): Promise<Response> => {
    requests.push(request.clone());
    return answer === null
      ? new Response(null, { status })
      : Response.json(answer, { status });
  };
  return { requests, fetcher };
}

const ACCEPTED = {
  artifact_id: "art_1",
  claim_key: "auto-0123456789abcdef",
  version: 1,
  state: "held",
  plaintext_digest: `sha256:${"0".repeat(64)}`,
  journal_seq: 1,
  workarea_id: "wa_1",
  retirement: false,
  replaces: null,
};

function client(fetcher: (request: Request) => Promise<Response>): OmlClient {
  return new OmlClient({ url: "http://oml.test", apiKey: "oml_test_key", fetch: fetcher });
}

/** Asserts `call` rejects with request.validation naming `field`. */
async function refusedNaming(call: () => Promise<unknown>, field: string): Promise<void> {
  await assert.rejects(call, (error: unknown) => {
    assert.ok(error instanceof RequestError);
    assert.equal(error.code, "request.validation");
    assert.deepEqual(error.details, { field });
    return true;
  });
}

test("every family errors.md declares has its own exception class", () => {
  const families = declaredFamilies();
  assert.ok(families.has("auth") && families.has("system"));
  assert.deepEqual(new Set(Object.keys(FAMILIES)), families);
  assert.equal(new Set(Object.values(FAMILIES)).size, families.size);
  for (const cls of Object.values(FAMILIES)) assert.ok(cls.prototype instanceof ApiError);
});

test("a code the client has never seen lands in its family's class", () => {
  const error = errorFromResponse(409, JSON.parse(envelope("submission.brand_new")), new Headers());
  assert.equal(error.constructor, SubmissionError);
  assert.equal(error.code, "submission.brand_new");
});

test("a family the client has never seen lands in ApiError", () => {
  const error = errorFromResponse(409, JSON.parse(envelope("brand.new")), new Headers());
  assert.equal(error.constructor, ApiError);
  assert.equal(error.code, "brand.new");
});

test("a renamed field refusal is a RequestError carrying details.renamed_to", () => {
  const body = {
    error: {
      code: "request.validation",
      message: "scope is renamed workarea: send workarea",
      rule: "r",
      details: { field: "scope", renamed_to: "workarea" },
      retryable: false,
    },
    request_id: "req_1",
  };
  const error = errorFromResponse(400, body, new Headers());
  assert.equal(error.constructor, RequestError);
  assert.equal(error.code, "request.validation");
  assert.deepEqual(error.details, { field: "scope", renamed_to: "workarea" });
});

test("a response without the envelope is an ApiError without a code", () => {
  const error = errorFromResponse(502, "<html>bad gateway</html>", new Headers());
  assert.equal(error.constructor, ApiError);
  assert.deepEqual([error.code, error.status, error.retryable], [null, 502, false]);
});

test("a retryable refusal is retried under the same key", async () => {
  const sent: (string | null)[] = [];
  const fetcher = async (request: Request): Promise<Response> => {
    sent.push(request.headers.get("idempotency-key"));
    if (sent.length === 1) {
      return new Response(
        JSON.stringify({
          error: {
            code: "system.unavailable",
            message: "the fork check has not verified",
            rule: "A replica that cannot prove its journal head accepts no writes (D1).",
            retryable: true,
          },
          request_id: "req_1",
        }),
        { status: 503, headers: { "content-type": "application/json", "retry-after": "0" } },
      );
    }
    return Response.json(
      {
        artifact_id: "art_1",
        claim_key: "retry",
        version: 1,
        state: "held",
        plaintext_digest: `sha256:${"0".repeat(64)}`,
        journal_seq: 1,
      },
      { status: 202 },
    );
  };
  const written = await client(fetcher).write({ text: "Retried once.", claim_key: "retry" });
  assert.equal(sent.length, 2);
  assert.equal(sent[0], written.idempotencyKey);
  assert.equal(sent[1], written.idempotencyKey);
  assert.equal(written.value.artifact_id, "art_1");
});

test("write without claim_key sends none", async () => {
  const { requests, fetcher } = recorder(202, ACCEPTED);
  const written = await client(fetcher).write({ text: "A claim with no key of its own." });
  assert.deepEqual(
    requests.map((r) => [r.method, new URL(r.url).pathname]),
    [["POST", "/v1/memories"]],
  );
  assert.deepEqual(await requests[0]?.json(), { body: "A claim with no key of its own." });
  assert.equal(requests[0]?.headers.get("idempotency-key"), written.idempotencyKey);
  assert.equal(written.value.claim_key, "auto-0123456789abcdef");
});

test("write with replaces supersedes its target and sends the reason", async () => {
  const { requests, fetcher } = recorder(202, { ...ACCEPTED, version: 2 });
  await client(fetcher).write({
    text: "Retired: the runbook moved.",
    replaces: "art_1",
    expected_version: 1,
    retirement: true,
    reason: "moved",
  });
  assert.deepEqual(
    requests.map((r) => [r.method, new URL(r.url).pathname]),
    [["POST", "/v1/memories/art_1/supersede"]],
  );
  assert.deepEqual(await requests[0]?.json(), {
    body: "Retired: the runbook moved.",
    expected_version: 1,
    retirement: true,
    reason: "moved",
  });
});

test("a write argument outside its mode raises before sending", async () => {
  const { requests, fetcher } = recorder();
  const oml = client(fetcher);
  const cases: [WriteInput, string][] = [
    [{ text: "t", retirement: true }, "/retirement"],
    [{ text: "t", reason: "why" }, "/reason"],
    [{ text: "t", replaces: "art_1", claim_key: "k" }, "/claim_key"],
    [{ text: "t", replaces: "art_1" }, "/expected_version"],
    [{ text: "t", expected_version: 1 }, "/expected_version"],
  ];
  for (const [input, field] of cases) await refusedNaming(() => oml.write(input), field);
  assert.deepEqual(requests, []);
});

test("read with artifact_id and query raises before sending", async () => {
  const { requests, fetcher } = recorder();
  const both: ReadInput = { artifact_id: "art_1", query: "canary stage" };
  await refusedNaming(() => client(fetcher).read(both), "/query");
  assert.deepEqual(requests, []);
});

test("a read argument outside its mode raises before sending", async () => {
  const { requests, fetcher } = recorder();
  const oml = client(fetcher);
  const cases: [ReadInput, string][] = [
    [{ artifact_id: "art_1", workarea: "w" }, "/workarea"],
    [{ query: "q", own: true }, "/own"],
    [{ k: 3 }, "/k"],
  ];
  for (const [input, field] of cases) await refusedNaming(() => oml.read(input), field);
  assert.deepEqual(requests, []);
});

test("read with neither lists the key's own rows", async () => {
  const { requests, fetcher } = recorder(200, { items: [], next_cursor: null });
  const page: MemoryPage = await client(fetcher).read({ limit: 5 });
  assert.deepEqual(page.items, []);
  const url = new URL(requests[0]?.url ?? "");
  assert.deepEqual(
    [requests[0]?.method, url.pathname, Object.fromEntries(url.searchParams)],
    ["GET", "/v1/memories", { own: "true", limit: "5" }],
  );
});

test("the listing iterators send own true unless the caller passes own false, workarea in the query", async () => {
  const { requests, fetcher } = recorder(200, { items: [], next_cursor: null });
  const oml = client(fetcher);
  for await (const _ of oml.memories({ limit: 5 })) assert.fail("no rows were answered");
  for await (const _ of oml.pages({ own: false, workarea: "w", limit: 5 })) continue;
  assert.deepEqual(
    requests.map((r) => Object.fromEntries(new URL(r.url).searchParams)),
    [
      { own: "true", limit: "5" },
      { own: "false", workarea: "w", limit: "5" },
    ],
  );
});

test("write with derives_from sends no kind unless the caller names one", async () => {
  const { requests, fetcher } = recorder(202, ACCEPTED);
  const oml = client(fetcher);
  await oml.write({ text: "Derived without a kind.", derives_from: ["art_0"] });
  await oml.write({ text: "Derived as learn stored it.", derives_from: ["art_0"], kind: "derived_fact" });
  assert.deepEqual(
    await Promise.all(requests.map((r) => r.json())),
    [
      { body: "Derived without a kind.", derives_from: ["art_0"] },
      { body: "Derived as learn stored it.", derives_from: ["art_0"], kind: "derived_fact" },
    ],
  );
});

test("approve without serve_id sends none", async () => {
  const result = { artifact_id: "art_1", version: 1, verdict: "approve", state: "promoted" };
  const { requests, fetcher } = recorder(200, result);
  const written = await client(fetcher).approve();
  assert.deepEqual(await requests[0]?.json(), { verdict: "approve" });
  assert.equal(written.value.state, "promoted");
});

test("reject sends its reason and serve_id", async () => {
  const result = { artifact_id: "art_1", version: 1, verdict: "reject", state: "rejected" };
  const { requests, fetcher } = recorder(200, result);
  await client(fetcher).reject({ serve_id: "srv_1", reason: "no source" });
  assert.deepEqual(await requests[0]?.json(), {
    serve_id: "srv_1",
    verdict: "reject",
    reason: "no source",
  });
});

test("a refusal raises the envelope's code and details", async () => {
  const envelope = {
    error: {
      code: "request.validation",
      message: "query is refused with artifact_id",
      rule: "The contract is openapi.yaml; what it does not admit is refused, not coerced.",
      details: { field: "/query" },
      retryable: false,
    },
    request_id: "req_7",
  };
  const { fetcher } = recorder(400, envelope);
  await assert.rejects(client(fetcher).read({ query: "canary stage" }), (error: unknown) => {
    assert.ok(error instanceof ApiError);
    assert.deepEqual(
      [error.status, error.code, error.details],
      [400, "request.validation", { field: "/query" }],
    );
    assert.deepEqual(
      [error.message, error.rule, error.requestId, error.retryable],
      [envelope.error.message, envelope.error.rule, "req_7", false],
    );
    return true;
  });
});

test("the client refuses an environment that sets only OML_URL", () => {
  process.env.OML_URL = "http://oml.test";
  const { requests, fetcher } = recorder();
  assert.throws(() => new OmlClient({ fetch: fetcher }), {
    name: "ConfigError",
    message: "OML_URL is set without OML_API_KEY",
  });
  assert.deepEqual(requests, []);
});

test("the client refuses an environment that sets only OML_API_KEY", () => {
  process.env.OML_API_KEY = "oml_test_key";
  assert.throws(() => new OmlClient(), {
    name: "ConfigError",
    message: "OML_API_KEY is set without OML_URL",
  });
});

test("one option alone is refused and never takes the other from the environment", () => {
  process.env.OML_URL = "http://oml.test";
  process.env.OML_API_KEY = "oml_test_key";
  const { requests, fetcher } = recorder();
  assert.throws(() => new OmlClient({ url: "http://elsewhere.test", fetch: fetcher }), {
    name: "ConfigError",
    message: "url is passed without apiKey",
  });
  assert.throws(() => new OmlClient({ apiKey: "oml_other_key", fetch: fetcher }), {
    name: "ConfigError",
    message: "apiKey is passed without url",
  });
  assert.deepEqual(requests, []);
});

test("a client with no source refuses before any request", () => {
  const { requests, fetcher } = recorder();
  assert.throws(() => new OmlClient({ fetch: fetcher }), ConfigError);
  assert.deepEqual(requests, []);
});

test("the key is in no inspection of the client", () => {
  const oml = new OmlClient({ url: "http://oml.test", apiKey: "oml_secret_value" });
  assert.ok(!inspect(oml, { depth: 10, showHidden: true }).includes("oml_secret_value"));
  assert.ok(!JSON.stringify(oml).includes("oml_secret_value"));
  assert.ok(!String(oml).includes("oml_secret_value"));
});

/** Every [METHOD, path] the document's paths declare, in its own layout. */
function declaredOperations(document: string): Set<string> {
  const operations = new Set<string>();
  let inPaths = false;
  let path: string | null = null;
  for (const line of document.split("\n")) {
    if (line !== "" && !line.startsWith(" ")) {
      inPaths = line === "paths:";
      path = null;
    } else if (inPaths && line.startsWith("  /")) {
      path = line.trim().replace(/:$/, "");
    } else if (inPaths && path !== null) {
      const method = /^ {4}(get|put|post|delete|patch):$/.exec(line)?.[1];
      if (method) operations.add(`${method.toUpperCase()} ${path}`);
    }
  }
  return operations;
}

/** Every [METHOD, path] the generated `paths` interface types as an operation. */
function generatedOperations(schema: string): Set<string> {
  const operations = new Set<string>();
  const start = schema.indexOf("export interface paths {");
  const body = schema.slice(start, schema.indexOf("\nexport ", start));
  let path: string | null = null;
  for (const line of body.split("\n")) {
    const item = /^ {4}"([^"]+)": \{$/.exec(line)?.[1];
    if (item) path = item;
    const method = /^ {8}(get|put|post|delete|patch): operations\[/.exec(line)?.[1];
    if (method && path !== null) operations.add(`${method.toUpperCase()} ${path}`);
  }
  return operations;
}

test("the generated SDK carries no operation the server does not route", () => {
  // openapi.yaml declares only what the server routes, which the server's
  // own test holds against its route table (ADR-0039 d9).
  const declared = declaredOperations(
    readFileSync(new URL("../../../docs/api/openapi.yaml", import.meta.url), "utf8"),
  );
  assert.ok(declared.has("GET /v2/whoami"));
  assert.equal(declared.size, 58);
  const generated = generatedOperations(
    readFileSync(new URL("../src/generated/schema.ts", import.meta.url), "utf8"),
  );
  assert.deepEqual([...generated].sort(), [...declared].sort());
});

test("every request names the SDK and package.json's version in User-Agent (ADR-0042 d9)", async () => {
  const { version } = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8"));
  assert.equal(USER_AGENT, `oml-sdk-typescript/${version}`);
  const { requests, fetcher } = recorder(200, { items: [], next_cursor: null });
  await client(fetcher).read({ limit: 1 });
  assert.equal(requests[0]?.headers.get("user-agent"), USER_AGENT);
});
