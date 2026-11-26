// The wrapper's rules that need no server (docs/engineering/sdks.md).

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { inspect } from "node:util";
import { beforeEach, test } from "node:test";

import {
  ApiError,
  ConfigError,
  FAMILIES,
  fenceFor,
  type MemoryPage,
  OmlClient,
  type ReadInput,
  RequestError,
  type SkillSet,
  SubmissionError,
  USER_AGENT,
  WorkspaceImportError,
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
  workspace_id: "wa_1",
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

test("an import refusal is a WorkspaceImportError, named as the Python SDK names it", () => {
  const error = errorFromResponse(409, JSON.parse(envelope("import.busy")), new Headers());
  assert.equal(error.constructor, WorkspaceImportError);
  assert.equal(error.name, "WorkspaceImportError");
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
      message: "scope is renamed workspace: send workspace",
      rule: "r",
      details: { field: "scope", renamed_to: "workspace" },
      retryable: false,
    },
    request_id: "req_1",
  };
  const error = errorFromResponse(400, body, new Headers());
  assert.equal(error.constructor, RequestError);
  assert.equal(error.code, "request.validation");
  assert.deepEqual(error.details, { field: "scope", renamed_to: "workspace" });
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
    [{ artifact_id: "art_1", workspace: "w" }, "/workspace"],
    [{ query: "q", own: true }, "/own"],
    [{ k: 3 }, "/k"],
  ];
  for (const [input, field] of cases) await refusedNaming(() => oml.read(input), field);
  assert.deepEqual(requests, []);
});

test("read with neither lists the catalog, own false", async () => {
  const { requests, fetcher } = recorder(200, { items: [], next_cursor: null });
  const page: MemoryPage = await client(fetcher).read({ limit: 5 });
  assert.deepEqual(page.items, []);
  const url = new URL(requests[0]?.url ?? "");
  assert.deepEqual(
    [requests[0]?.method, url.pathname, Object.fromEntries(url.searchParams)],
    ["GET", "/v1/memories", { own: "false", limit: "5" }],
  );
});

test("the catalog sends kinds, prefix and include_text, and refuses the single kind", async () => {
  const { requests, fetcher } = recorder(200, { items: [], next_cursor: null });
  const oml = client(fetcher);
  await oml.read({ kinds: ["skill", "guardrail"], prefix: "skill/", include_text: true });
  const url = new URL(requests[0]?.url ?? "");
  assert.deepEqual(url.searchParams.getAll("kinds").join(","), "skill,guardrail");
  assert.equal(url.searchParams.get("prefix"), "skill/");
  assert.equal(url.searchParams.get("include_text"), "true");
  assert.equal(url.searchParams.get("own"), "false");
  await refusedNaming(() => oml.read({ kind: "memory" } as unknown as ReadInput), "/kind");
  assert.equal(requests.length, 1);
});

test("the listing iterators list the catalog unless the caller passes own true, which carries the text", async () => {
  const { requests, fetcher } = recorder(200, { items: [], next_cursor: null });
  const oml = client(fetcher);
  for await (const _ of oml.memories({ limit: 5 })) assert.fail("no rows were answered");
  for await (const _ of oml.pages({ own: true, workspace: "w", limit: 5 })) continue;
  assert.deepEqual(
    requests.map((r) => Object.fromEntries(new URL(r.url).searchParams)),
    [
      { own: "false", limit: "5" },
      { own: "true", include_text: "true", workspace: "w", limit: "5" },
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

test("withdraw posts the reason under an idempotency key", async () => {
  const result = { artifact_id: "art_1", state: "withdrawn", at: "2026-11-17T18:00:00Z", journal_seq: 7 };
  const { requests, fetcher } = recorder(200, result);
  const written = await client(fetcher).withdraw("art_1", { reason: "written by mistake" });
  assert.deepEqual(
    requests.map((r) => [r.method, new URL(r.url).pathname]),
    [["POST", "/v1/memories/art_1/withdraw"]],
  );
  assert.deepEqual(await requests[0]?.json(), { reason: "written by mistake" });
  assert.equal(requests[0]?.headers.get("idempotency-key"), written.idempotencyKey);
  assert.equal(written.value.state, "withdrawn");
});

/** One offered skill as `GET /v1/skills` serves it (openapi.yaml `ServedSkill`). */
function servedSkill(name: string) {
  return {
    artifact_id: `art_${name}`,
    claim_key: `skill/${name}`,
    name,
    description: `The ${name} routine.`,
    version: 1,
    workspace_path: "universe.alice",
    depth: 0,
    overrides: [],
    approved_at: "2026-11-17T18:00:00Z",
    approver: { used_by: "person", same_user: false },
    uri: `oml://workspace/universe.alice/skill/${name}`,
    unreviewed: false,
  };
}

function skillPage(names: string[], next_cursor: string | null) {
  return {
    scope_node: "ws_1",
    scope_path: "universe.alice",
    guardrail_scope: "ancestors",
    set_digest: "sd_1",
    skills: names.map(servedSkill),
    next_cursor,
  };
}

test("skills reads one page of the offered set and sends only the fields given", async () => {
  const { requests, fetcher } = recorder(200, skillPage(["deploy"], "c2"));
  const oml = client(fetcher);
  const page: SkillSet = await oml.skills();
  await oml.skills({ workspace: "universe.alice", include_own_held: true, cursor: "c 1" });
  assert.deepEqual(
    requests.map((r) => [r.method, new URL(r.url).pathname, Object.fromEntries(new URL(r.url).searchParams)]),
    [
      ["GET", "/v1/skills", {}],
      ["GET", "/v1/skills", { workspace: "universe.alice", include_own_held: "true", cursor: "c 1" }],
    ],
  );
  assert.deepEqual(page.skills.map((s) => s.name), ["deploy"]);
  assert.equal(page.next_cursor, "c2");
});

test("skills list pages by cursor", async () => {
  const answers = [skillPage(["cite", "deploy"], "c2"), skillPage(["triage"], null)];
  const requests: Request[] = [];
  const fetcher = async (request: Request): Promise<Response> => {
    requests.push(request.clone());
    return Response.json(answers[requests.length - 1], { status: 200 });
  };
  const names: string[] = [];
  for await (const skill of client(fetcher).iterSkills({ workspace: "universe.alice", include_own_held: true })) {
    names.push(skill.name);
  }
  assert.deepEqual(names, ["cite", "deploy", "triage"]);
  assert.deepEqual(
    requests.map((r) => Object.fromEntries(new URL(r.url).searchParams)),
    [
      { workspace: "universe.alice", include_own_held: "true" },
      { workspace: "universe.alice", include_own_held: "true", cursor: "c2" },
    ],
  );
});

test("write sends a skill's name and description and a document's title", async () => {
  const { requests, fetcher } = recorder(202, ACCEPTED);
  const oml = client(fetcher);
  const skill = { name: "release", description: "Cut a release." };
  await oml.write({ text: "Run the tests, then tag.", kind: "skill", skill });
  await oml.write({ text: "# Runbook\n\nRestart the lead.", kind: "document", document: { title: "Runbook" } });
  await oml.write({ text: "Run the tests twice, then tag.", replaces: "art_1", expected_version: 1, skill });
  assert.deepEqual(
    requests.map((r) => [r.method, new URL(r.url).pathname]),
    [
      ["POST", "/v1/memories"],
      ["POST", "/v1/memories"],
      ["POST", "/v1/memories/art_1/supersede"],
    ],
  );
  assert.deepEqual(await Promise.all(requests.map((r) => r.json())), [
    { body: "Run the tests, then tag.", kind: "skill", skill },
    { body: "# Runbook\n\nRestart the lead.", kind: "document", document: { title: "Runbook" } },
    { body: "Run the tests twice, then tag.", expected_version: 1, skill },
  ]);
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
  assert.equal(declared.size, 115);
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

test("fenceFor names the Markdown fence of each media type", () => {
  assert.equal(fenceFor("text/vnd.oml.mermaid"), "mermaid");
  assert.equal(fenceFor("application/json"), "json");
  assert.equal(fenceFor("application/yaml"), "yaml");
  assert.equal(fenceFor("text/vnd.oml.code; lang=rust"), "rust");
  assert.equal(fenceFor("text/markdown"), "markdown");
  assert.equal(fenceFor("text/plain"), "");
  assert.equal(fenceFor(undefined), "");
  assert.equal(fenceFor("application/x-unknown"), "");
});

test("a kind and a state a newer server adds are read as their strings", async () => {
  // An SDK from v0.8.5 on validates no enum, so a value the spec it was
  // generated from does not list reaches the caller unchanged (ADR-0096).
  const row = { ...ACCEPTED, kind: "playbook", state: "quarantined" };
  const { fetcher } = recorder(200, { items: [row], next_cursor: null });
  const page: MemoryPage = await client(fetcher).read({ limit: 1 });
  assert.equal(page.items[0]?.kind, "playbook");
  assert.equal(page.items[0]?.state, "quarantined");
});
