// The wrapper's rules that need no server (docs/engineering/sdks.md).

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { inspect } from "node:util";
import { beforeEach, test } from "node:test";

import {
  ApiError,
  ConfigError,
  FAMILIES,
  NotBuiltError,
  OmlClient,
  SubmissionError,
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

function recorder() {
  const requests: Request[] = [];
  const fetcher = async (request: Request): Promise<Response> => {
    requests.push(request);
    return new Response(null, { status: 500 });
  };
  return { requests, fetcher };
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
  const client = new OmlClient({ baseUrl: "http://oml.test", apiKey: "oml_test_key", fetch: fetcher });
  const written = await client.remember({ claim_key: "retry", body: "Retried once." });
  assert.equal(sent.length, 2);
  assert.equal(sent[0], written.idempotencyKey);
  assert.equal(sent[1], written.idempotencyKey);
  assert.equal(written.value.artifact_id, "art_1");
});

test("a client without a key refuses before any request", () => {
  const { requests, fetcher } = recorder();
  assert.throws(() => new OmlClient({ baseUrl: "http://oml.test", fetch: fetcher }), ConfigError);
  assert.deepEqual(requests, []);
});

test("a client without a server refuses before any request", () => {
  const { requests, fetcher } = recorder();
  assert.throws(() => new OmlClient({ apiKey: "oml_test_key", fetch: fetcher }), ConfigError);
  assert.deepEqual(requests, []);
});

test("the key is in no inspection of the client", () => {
  const client = new OmlClient({ baseUrl: "http://oml.test", apiKey: "oml_secret_value" });
  assert.ok(!inspect(client, { depth: 10, showHidden: true }).includes("oml_secret_value"));
  assert.ok(!JSON.stringify(client).includes("oml_secret_value"));
  assert.ok(!String(client).includes("oml_secret_value"));
});

test("an upload raises NotBuiltError and sends no request", async () => {
  const { requests, fetcher } = recorder();
  const client = new OmlClient({ baseUrl: "http://oml.test", apiKey: "oml_test_key", fetch: fetcher });
  await assert.rejects(
    client.uploadAttachment(new TextEncoder().encode("%PDF-1.7"), { mediaType: "application/pdf" }),
    NotBuiltError,
  );
  assert.deepEqual(requests, []);
});
