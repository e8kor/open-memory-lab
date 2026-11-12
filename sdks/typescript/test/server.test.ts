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
import { marker, mintsRoles, type OmlServer, startServer } from "./harness.ts";

// Without a superuser to mint the role pair, every suite below skips and the
// harness has printed its did-not-run line (ADR-0034 d8).
const live = { skip: !mintsRoles() };

let server: OmlServer;
let agent: OmlClient;
let reviewer: OmlClient;
const tag = () => randomBytes(4).toString("hex");

before(async () => {
  if (live.skip) return;
  server = await startServer();
  agent = new OmlClient({ url: server.url, apiKey: server.agentKey });
  reviewer = new OmlClient({ url: server.url, apiKey: server.dreamerKey });
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

describe("the agent loop", live, () => {
  test("a written memory is held, recalled by its author alone, and promoted by a reviewer", async () => {
    const word = marker();
    const workarea = server.reviewWorkarea;
    const written = await agent.write({
      text: `The SDK agent loop wrote this memory, ${word}.`,
      claim_key: `loop-${word}`,
      workarea,
    });
    const held = written.value;
    assert.equal(held.state, "held");
    assert.equal(written.replayed, false);

    const own = await agent.read({ query: word, include_unreviewed: true, workarea });
    assert.deepEqual(
      own.hits.map((hit) => [hit.memory.artifact_id, hit.memory.state]),
      [[held.artifact_id, "held"]],
    );
    // Nobody has approved a held memory (ADR-0077 d5).
    assert.deepEqual([own.hits[0]?.memory.approver, own.hits[0]?.memory.approved_at], [null, null]);
    assert.deepEqual((await agent.read({ query: word, workarea })).hits, []);

    const serve = await reviewer.reviewNext();
    assert.ok(serve);
    assert.equal(serve.item.artifact_id, held.artifact_id);
    const verdict = await reviewer.approve(); // the one item this key claimed (ADR-0039 d10)
    assert.equal(verdict.value.state, "promoted");

    const recalled = await agent.read({ query: word, workarea });
    assert.deepEqual(
      recalled.hits.map((hit) => [hit.memory.artifact_id, hit.memory.state]),
      [[held.artifact_id, "promoted"]],
    );
    // The hit says a person approved it, and when; the read by id says the
    // same (ADR-0077 d1, d2). The fixture's reviewer key is held by a human
    // and acts for no user.
    const approved = recalled.hits[0]?.memory;
    assert.deepEqual(approved?.approver, { held_by: "human", same_user: false });
    assert.ok(approved?.approved_at && approved.promoted_at);
    assert.ok(approved.approved_at <= approved.promoted_at);
    const fetched = await agent.read({ artifact_id: held.artifact_id });
    assert.deepEqual([fetched.approver, fetched.approved_at], [approved.approver, approved.approved_at]);
    assert.equal(fetched.state, "promoted");
    assert.equal(await reviewer.reviewNext(), null);
  });

  test("a held version cannot be superseded and the promoted one can", async () => {
    const word = marker();
    const workarea = server.reviewWorkarea;
    const held = (
      await agent.write({ text: `The SDK README's canary rule, ${word}.`, claim_key: `tip-${word}`, workarea })
    ).value;
    const supersede = { text: "Drain, then wait five minutes.", replaces: held.artifact_id, expected_version: 1 };
    await assert.rejects(agent.write(supersede), (error: unknown) => {
      assert.ok(error instanceof SubmissionError);
      assert.equal(error.code, "submission.tip_conflict");
      return true;
    });

    assert.equal((await reviewer.reviewNext())?.item.artifact_id, held.artifact_id);
    assert.equal((await reviewer.approve()).value.state, "promoted");
    const successor = (await agent.write(supersede)).value;
    assert.deepEqual([successor.state, successor.replaces?.artifact_id], ["held", held.artifact_id]);

    assert.equal((await reviewer.reviewNext())?.item.artifact_id, successor.artifact_id);
    await reviewer.reject({ reason: "the test's successor; nothing more waits" });
    assert.equal(await reviewer.reviewNext(), null);
  });
});

describe("idempotency", live, () => {
  test("a write whose response was lost is retried under the same key and replayed", async () => {
    const claimKey = `lost-${tag()}`;
    const { state, fetcher } = loseFirstSubmitResponse();
    const client = new OmlClient({ url: server.url, apiKey: server.agentKey, fetch: fetcher });
    const written = await client.write({
      text: "A write whose first response never arrived.",
      claim_key: claimKey,
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
    const input = { text: "Written once, sent twice.", claim_key: claimKey };
    const first = await agent.write(input, { idempotencyKey });
    const second = await agent.write(input, { idempotencyKey });
    assert.deepEqual([first.replayed, second.replayed], [false, true]);
    assert.equal(second.idempotencyKey, idempotencyKey);
    assert.equal(second.value.artifact_id, first.value.artifact_id);
  });

  test("a key reused with a different body is refused and not retried", async () => {
    const claimKey = `mismatch-${tag()}`;
    const idempotencyKey = `sdk-test/${tag()}`;
    await agent.write({ text: "The first body.", claim_key: claimKey }, { idempotencyKey });
    const { state, fetcher } = countSubmits();
    const client = new OmlClient({ url: server.url, apiKey: server.agentKey, fetch: fetcher });
    await assert.rejects(
      client.write({ text: "A different body.", claim_key: claimKey }, { idempotencyKey }),
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
    const one = await agent.write({ text: "The first of two.", claim_key: `fresh-${t}-1` });
    const two = await agent.write({ text: "The second of two.", claim_key: `fresh-${t}-2` });
    assert.notEqual(one.idempotencyKey, two.idempotencyKey);
    assert.deepEqual([one.replayed, two.replayed], [false, false]);
  });
});

describe("pagination", live, () => {
  test("iteration follows next_cursor across pages until it is null", async () => {
    const t = tag();
    const written: string[] = [];
    for (let i = 0; i < 5; i++) {
      const w = await agent.write({ text: `Pagination row ${i}.`, claim_key: `page-${t}-${i}` });
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
      await agent.write({ text: `Cursor row ${i}.`, claim_key: `cursor-${t}-${i}` });
    }
    const first = await agent.read({ own: true, limit: 2 });
    assert.equal(first.items.length, 2);
    assert.ok(first.next_cursor);
    const second = await agent.read({ own: true, limit: 2, cursor: first.next_cursor });
    const firstIds = new Set(first.items.map((row) => row.artifact_id));
    assert.ok(second.items.every((row) => !firstIds.has(row.artifact_id)));
  });
});

describe("error mapping", live, () => {
  test("a bad key is an AuthError with its code, rule, message and retryable", async () => {
    const client = new OmlClient({ url: server.url, apiKey: "oml_not_a_key" });
    await assert.rejects(client.read({ query: "anything" }), (error: unknown) => {
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

  test("a second claim under a live claim_key is a SubmissionError", async () => {
    const claimKey = `taken-${tag()}`;
    await agent.write({ text: "The first claim under this key.", claim_key: claimKey });
    await assert.rejects(
      agent.write({ text: "A second claim under the same key.", claim_key: claimKey }),
      (error: unknown) =>
        error instanceof SubmissionError &&
        error.code === "submission.claim_key_conflict" &&
        error.status === 409 &&
        error.retryable === false,
    );
  });

  test("a memory the key cannot see is access.not_found", async () => {
    await assert.rejects(
      agent.read({ artifact_id: randomUUID() }),
      (error: unknown) =>
        error instanceof AccessError && error.code === "access.not_found" && error.status === 404,
    );
  });

  test("a budget without text is a RequestError", async () => {
    await assert.rejects(
      agent.read({ query: "anything", max_tokens: 100, include_text: false }),
      (error: unknown) => error instanceof RequestError && error.code === "request.validation",
    );
  });
});

describe("the URL and key", live, () => {
  test("the key and URL come from the environment when neither is passed", async () => {
    process.env.OML_API_KEY = server.agentKey;
    process.env.OML_URL = server.url;
    const page = await new OmlClient().read({ limit: 1 });
    assert.ok(Array.isArray(page.items));
  });

  test("the options win over the environment", async () => {
    process.env.OML_API_KEY = "oml_not_a_key";
    process.env.OML_URL = "http://127.0.0.1:9";
    await new OmlClient({ url: server.url, apiKey: server.agentKey }).read({ limit: 1 });
  });
});

describe("workareas and events", live, () => {
  test("workarea without a name reads the key's default workarea", async () => {
    const view = await agent.workarea();
    assert.equal(view.id, server.workarea);
    assert.ok(view.my_effective_verbs.includes("remember"));
    assert.equal((await agent.workarea(server.reviewWorkarea)).id, server.reviewWorkarea);
  });

  test("events tails the named workarea from after_seq", async () => {
    // The key's own workarea, never the review queue's, which the agent loop reads whole.
    const written = await agent.write({
      text: `An event to tail, ${marker()}.`,
      workarea: server.workarea,
    });
    const page = await agent.events({ workarea: server.workarea });
    assert.ok(page.head_seq >= (written.value.journal_seq ?? Number.POSITIVE_INFINITY));
    const later = await agent.events({ workarea: server.workarea, after_seq: page.head_seq });
    assert.deepEqual(later.items, []);
  });
});
