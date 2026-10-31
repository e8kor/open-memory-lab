// The SDK against a real oml-server-lead (docs/engineering/sdks.md, "Tests").

import assert from "node:assert/strict";
import { randomBytes, randomUUID } from "node:crypto";
import { after, before, beforeEach, describe, test } from "node:test";

import {
  AccessError,
  AuthError,
  IdempotencyError,
  OmlClient,
  RequestError,
  SubmissionError,
} from "../src/index.ts";
import { marker, type OmlServer, startServer } from "./harness.ts";

let server: OmlServer;
let agent: OmlClient;
let reviewer: OmlClient;
const tag = () => randomBytes(4).toString("hex");

before(async () => {
  server = await startServer();
  agent = new OmlClient({ baseUrl: server.url, apiKey: server.agentKey });
  reviewer = new OmlClient({ baseUrl: server.url, apiKey: server.dreamerKey });
});

after(async () => {
  await server?.stop();
});

beforeEach(() => {
  // No test reads a key or a server from the environment it inherited.
  delete process.env.OML_API_KEY;
  delete process.env.OML_URL;
});

/** Forwards to the real server; loses the first submit's response after the server answered it. */
function loseFirstSubmitResponse() {
  const state = { lost: 0, keys: [] as (string | null)[] };
  const fetcher = async (request: Request): Promise<Response> => {
    const isSubmit = request.method === "POST" && new URL(request.url).pathname === "/v1/memories";
    if (isSubmit) state.keys.push(request.headers.get("idempotency-key"));
    const response = await fetch(request);
    if (isSubmit && state.lost === 0) {
      state.lost += 1;
      await response.arrayBuffer();
      throw new TypeError("fetch failed: simulated, the response was lost");
    }
    return response;
  };
  return { state, fetcher };
}

function countSubmits() {
  const state = { submits: 0 };
  const fetcher = async (request: Request): Promise<Response> => {
    if (request.method === "POST" && new URL(request.url).pathname === "/v1/memories") {
      state.submits += 1;
    }
    return await fetch(request);
  };
  return { state, fetcher };
}

describe("the agent loop", () => {
  test("a remembered memory is held, recalled by its author alone, and promoted by a reviewer", async () => {
    const word = marker();
    const workarea = server.reviewWorkarea;
    const written = await agent.remember(
      { claim_key: `loop-${word}`, body: `The SDK agent loop wrote this memory, ${word}.` },
      { workarea },
    );
    const held = written.value;
    assert.equal(held.state, "held");
    assert.equal(written.replayed, false);

    const own = await agent.recall({ query: word, include_own_held: true }, { workarea });
    assert.deepEqual(
      own.hits.map((hit) => [hit.memory.artifact_id, hit.memory.state]),
      [[held.artifact_id, "held"]],
    );
    assert.deepEqual((await agent.recall({ query: word }, { workarea })).hits, []);

    const serve = await reviewer.reviewNext();
    assert.ok(serve);
    assert.equal(serve.item.artifact_id, held.artifact_id);
    const verdict = await reviewer.verdict(serve.serve_id, "approve");
    assert.equal(verdict.value.state, "promoted");

    const recalled = await agent.recall({ query: word }, { workarea });
    assert.deepEqual(
      recalled.hits.map((hit) => [hit.memory.artifact_id, hit.memory.state]),
      [[held.artifact_id, "promoted"]],
    );
    assert.equal((await agent.get(held.artifact_id)).state, "promoted");
    assert.equal(await reviewer.reviewNext(), null);
  });
});

describe("idempotency", () => {
  test("a write whose response was lost is retried under the same key and replayed", async () => {
    const claimKey = `lost-${tag()}`;
    const { state, fetcher } = loseFirstSubmitResponse();
    const client = new OmlClient({ baseUrl: server.url, apiKey: server.agentKey, fetch: fetcher });
    const written = await client.remember({
      claim_key: claimKey,
      body: "A write whose first response never arrived.",
    });
    assert.equal(state.lost, 1);
    assert.deepEqual(state.keys, [written.idempotencyKey, written.idempotencyKey]);
    assert.equal(written.replayed, true);
    assert.equal(written.value.state, "held");
    const rows = [];
    for await (const row of agent.memories({ own: true, claim_key: claimKey })) rows.push(row);
    assert.deepEqual(
      rows.map((row) => row.artifact_id),
      [written.value.artifact_id],
    );
  });

  test("a caller-supplied key replays the first write", async () => {
    const claimKey = `replay-${tag()}`;
    const idempotencyKey = `sdk-test/${tag()}`;
    const body = { claim_key: claimKey, body: "Written once, sent twice." };
    const first = await agent.remember(body, { idempotencyKey });
    const second = await agent.remember(body, { idempotencyKey });
    assert.deepEqual([first.replayed, second.replayed], [false, true]);
    assert.equal(second.idempotencyKey, idempotencyKey);
    assert.equal(second.value.artifact_id, first.value.artifact_id);
  });

  test("a key reused with a different body is refused and not retried", async () => {
    const claimKey = `mismatch-${tag()}`;
    const idempotencyKey = `sdk-test/${tag()}`;
    await agent.remember({ claim_key: claimKey, body: "The first body." }, { idempotencyKey });
    const { state, fetcher } = countSubmits();
    const client = new OmlClient({ baseUrl: server.url, apiKey: server.agentKey, fetch: fetcher });
    await assert.rejects(
      client.remember({ claim_key: claimKey, body: "A different body." }, { idempotencyKey }),
      (error: unknown) =>
        error instanceof IdempotencyError &&
        error.code === "idempotency.digest_mismatch" &&
        error.status === 409 &&
        error.retryable === false,
    );
    assert.equal(state.submits, 1);
  });

  test("each call sends a fresh key", async () => {
    const t = tag();
    const one = await agent.remember({ claim_key: `fresh-${t}-1`, body: "The first of two." });
    const two = await agent.remember({ claim_key: `fresh-${t}-2`, body: "The second of two." });
    assert.notEqual(one.idempotencyKey, two.idempotencyKey);
    assert.deepEqual([one.replayed, two.replayed], [false, false]);
  });
});

describe("pagination", () => {
  test("iteration follows next_cursor across pages until it is null", async () => {
    const t = tag();
    const written: string[] = [];
    for (let i = 0; i < 5; i++) {
      const w = await agent.remember({ claim_key: `page-${t}-${i}`, body: `Pagination row ${i}.` });
      written.push(w.value.artifact_id);
    }
    const pages = [];
    for await (const page of agent.pages({ own: true, limit: 2 })) pages.push(page);
    assert.ok(pages.length >= 3);
    assert.ok(pages.slice(0, -1).every((page) => page.next_cursor));
    assert.equal(pages.at(-1)?.next_cursor, null);
    const ids = pages.flatMap((page) => page.items.map((row) => row.artifact_id));
    assert.equal(new Set(ids).size, ids.length);
    const positions = written.map((id) => ids.indexOf(id));
    assert.ok(positions.every((p) => p >= 0));
    assert.deepEqual(positions, [...positions].sort((a, b) => b - a)); // newest first

    const iterated = [];
    for await (const row of agent.memories({ own: true, limit: 2 })) iterated.push(row.artifact_id);
    assert.deepEqual(iterated, ids);
  });

  test("one page carries the cursor to the next", async () => {
    const t = tag();
    for (let i = 0; i < 3; i++) {
      await agent.remember({ claim_key: `cursor-${t}-${i}`, body: `Cursor row ${i}.` });
    }
    const first = await agent.listMemories({ own: true, limit: 2 });
    assert.equal(first.items.length, 2);
    assert.ok(first.next_cursor);
    const second = await agent.listMemories({ own: true, limit: 2, cursor: first.next_cursor });
    const firstIds = new Set(first.items.map((row) => row.artifact_id));
    assert.ok(second.items.every((row) => !firstIds.has(row.artifact_id)));
  });
});

describe("error mapping", () => {
  test("a bad key is an AuthError with its code, rule, message and retryable", async () => {
    const client = new OmlClient({ baseUrl: server.url, apiKey: "oml_not_a_key" });
    await assert.rejects(client.recall({ query: "anything" }), (error: unknown) => {
      assert.ok(error instanceof AuthError);
      assert.deepEqual([error.code, error.status, error.retryable], ["auth.invalid", 401, false]);
      assert.ok(error.rule && error.message);
      assert.ok(error.requestId?.startsWith("req_"));
      assert.ok(!String(error).includes("oml_not_a_key"));
      assert.ok(!JSON.stringify(error).includes("oml_not_a_key"));
      return true;
    });
  });

  test("a writer key asking for review is an AccessError", async () => {
    await assert.rejects(
      agent.reviewNext(),
      (error: unknown) =>
        error instanceof AccessError &&
        error.code === "access.verb_ceiling" &&
        error.status === 403 &&
        Boolean(error.rule),
    );
  });

  test("learn without lineage is a SubmissionError", async () => {
    await assert.rejects(
      agent.learn({ claim_key: "no-lineage", body: "A derived claim.", derives_from: [] }),
      (error: unknown) =>
        error instanceof SubmissionError &&
        error.code === "submission.lineage_required" &&
        error.status === 422 &&
        error.retryable === false,
    );
  });

  test("a memory the key cannot see is access.not_found", async () => {
    await assert.rejects(
      agent.get(randomUUID()),
      (error: unknown) =>
        error instanceof AccessError && error.code === "access.not_found" && error.status === 404,
    );
  });

  test("a budget without bodies is a RequestError", async () => {
    await assert.rejects(
      agent.recall({ query: "anything", max_tokens: 100, include_bodies: false }),
      (error: unknown) => error instanceof RequestError && error.code === "request.validation",
    );
  });
});

describe("the key", () => {
  test("the key and server come from the environment when none is passed", async () => {
    process.env.OML_API_KEY = server.agentKey;
    process.env.OML_URL = server.url;
    const page = await new OmlClient().listMemories({ own: true, limit: 1 });
    assert.ok(Array.isArray(page.items));
  });

  test("an explicit key wins over OML_API_KEY", async () => {
    process.env.OML_API_KEY = "oml_not_a_key";
    await new OmlClient({ baseUrl: server.url, apiKey: server.agentKey }).listMemories({
      own: true,
      limit: 1,
    });
    await assert.rejects(
      new OmlClient({ baseUrl: server.url }).listMemories({ own: true, limit: 1 }),
      AuthError,
    );
  });
});
