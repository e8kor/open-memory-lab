// The hand-written wrapper over the generated types and openapi-fetch
// (docs/engineering/sdks.md). It adds what a generator cannot know: where the
// key comes from, one Idempotency-Key per write that every retry of the call
// reuses, cursor iteration, typed exceptions per errors.md family, and the
// upload stub. Every path, parameter and body is typed by the generated schema.

import createClient, { type Client } from "openapi-fetch";

import { ConfigError, NotBuiltError, TransportError, errorFromResponse } from "./errors.ts";
import type { components, operations, paths } from "./generated/schema.ts";

type Schemas = components["schemas"];
export type SubmitRequest = Schemas["SubmitRequest"];
export type SubmitAccepted = Schemas["SubmitAccepted"];
export type RecallRequest = Schemas["RecallRequest"];
export type RecallResponse = Schemas["RecallResponse"];
export type Memory = Schemas["Memory"];
export type MemorySummary = Schemas["MemorySummary"];
export type MemoryPage = Schemas["MemoryPage"];
export type SupersedeRequest = Schemas["SupersedeRequest"];
export type Serve = Schemas["Serve"];
export type VerdictResult = Schemas["VerdictResult"];
export type ListQuery = NonNullable<operations["searchMemories"]["parameters"]["query"]>;

/** A submission without its verb; `kind` defaults to the verb's usual kind. */
export type RememberInput = Omit<SubmitRequest, "verb" | "kind"> & { kind?: SubmitRequest["kind"] };
export type LearnInput = RememberInput & { derives_from: string[] };

export interface OmlClientOptions {
  /** The server; else OML_URL. */
  baseUrl?: string;
  /** The key; else OML_API_KEY. */
  apiKey?: string;
  /** Retries after a transport failure or a retryable refusal (default 2). */
  retries?: number;
  /** Per-request timeout in milliseconds (default 10000). */
  timeoutMs?: number;
  /** A fetch to send requests with (default globalThis.fetch). */
  fetch?: (request: Request) => Promise<Response>;
}

export interface WriteOptions {
  /** The Idempotency-Key for this call; a fresh UUIDv4 when omitted. */
  idempotencyKey?: string;
  /** The X-OML-Workarea override: a node id within the key's grants. */
  workarea?: string;
}

/** A write's result, the Idempotency-Key every attempt of it sent, and whether the server replayed it. */
export interface Written<T> {
  value: T;
  idempotencyKey: string;
  replayed: boolean;
}

const RETRY_AFTER_CAP_S = 30; // the most a Retry-After is waited for
const FIRST_BACKOFF_MS = 200; // doubling per attempt, when no Retry-After is sent

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

function fromEnv(name: string): string | undefined {
  const env = (globalThis as { process?: { env?: Record<string, string | undefined> } }).process
    ?.env;
  return env?.[name] || undefined;
}

/** fetch rejects with a TypeError on a network failure, and AbortSignal.timeout with a TimeoutError. */
function isTransportFailure(failure: unknown): boolean {
  return (
    failure instanceof TypeError ||
    (failure instanceof DOMException && (failure.name === "TimeoutError" || failure.name === "AbortError"))
  );
}

interface Answer<T> {
  data?: T;
  error?: unknown;
  response: Response;
}

/** A client for one OML server under one key. */
export class OmlClient {
  readonly #api: Client<paths>;
  readonly #baseUrl: string;
  readonly #retries: number;
  readonly #timeoutMs: number;

  constructor(options: OmlClientOptions = {}) {
    const baseUrl = options.baseUrl || fromEnv("OML_URL");
    const apiKey = options.apiKey || fromEnv("OML_API_KEY");
    if (!baseUrl) throw new ConfigError("no server: pass baseUrl or set OML_URL");
    if (!apiKey) throw new ConfigError("no key: pass apiKey or set OML_API_KEY");
    const retries = options.retries ?? 2;
    if (!Number.isInteger(retries) || retries < 0) throw new ConfigError("retries must be 0 or more");
    this.#baseUrl = baseUrl.replace(/\/+$/, "");
    this.#retries = retries;
    this.#timeoutMs = options.timeoutMs ?? 10_000;
    this.#api = createClient<paths>({
      baseUrl: this.#baseUrl,
      headers: { Authorization: `Bearer ${apiKey}` },
      ...(options.fetch ? { fetch: options.fetch } : {}),
    });
  }

  toString(): string {
    return `OmlClient(${this.#baseUrl})`;
  }

  toJSON(): { baseUrl: string } {
    return { baseUrl: this.#baseUrl };
  }

  /** Send one call, retrying transport failures and retryable refusals; `call` sends the same request each time. */
  async #send<T>(call: (signal: AbortSignal) => Promise<Answer<T>>): Promise<Answer<T>> {
    for (let attempt = 0; ; attempt += 1) {
      let answer: Answer<T>;
      try {
        answer = await call(AbortSignal.timeout(this.#timeoutMs));
      } catch (failure) {
        if (!isTransportFailure(failure)) throw failure;
        if (attempt >= this.#retries) {
          throw new TransportError(`the request failed in transit: ${String(failure)}`, {
            cause: failure,
          });
        }
        await sleep(FIRST_BACKOFF_MS * 2 ** attempt);
        continue;
      }
      if (answer.response.ok) return answer;
      const refusal = errorFromResponse(answer.response.status, answer.error, answer.response.headers);
      if (!refusal.retryable || attempt >= this.#retries) throw refusal;
      await sleep(
        refusal.retryAfter !== null
          ? Math.min(refusal.retryAfter, RETRY_AFTER_CAP_S) * 1000
          : FIRST_BACKOFF_MS * 2 ** attempt,
      );
    }
  }

  async #write<T>(
    idempotencyKey: string | undefined,
    call: (key: string, signal: AbortSignal) => Promise<Answer<T>>,
  ): Promise<Written<T>> {
    const key = idempotencyKey ?? crypto.randomUUID();
    const answer = await this.#send((signal) => call(key, signal));
    return {
      value: answer.data as T,
      idempotencyKey: key,
      replayed: answer.response.headers.get("idempotency-replay")?.toLowerCase() === "true",
    };
  }

  // -- the provider ring (/v1) ----------------------------------------------

  /** Submit a claim with verb `remember`; it is held on arrival. */
  remember(input: RememberInput, options: WriteOptions = {}): Promise<Written<SubmitAccepted>> {
    return this.#submit({ kind: "memory", ...input, verb: "remember" }, options);
  }

  /** Submit a derived claim with verb `learn`; `derives_from` is its lineage. */
  learn(input: LearnInput, options: WriteOptions = {}): Promise<Written<SubmitAccepted>> {
    return this.#submit({ kind: "derived_fact", ...input, verb: "learn" }, options);
  }

  #submit(body: SubmitRequest, options: WriteOptions): Promise<Written<SubmitAccepted>> {
    return this.#write(options.idempotencyKey, (key, signal) =>
      this.#api.POST("/v1/memories", {
        params: {
          header: {
            "Idempotency-Key": key,
            ...(options.workarea ? { "X-OML-Workarea": options.workarea } : {}),
          },
        },
        body,
        signal,
      }),
    );
  }

  /** Ranked recall over promoted rows, and the caller's own held rows with `include_own_held`; only the fields given are sent. */
  async recall(body: RecallRequest, options: { workarea?: string } = {}): Promise<RecallResponse> {
    const answer = await this.#send((signal) =>
      this.#api.POST("/v1/memories/recall", {
        params: { header: options.workarea ? { "X-OML-Workarea": options.workarea } : {} },
        body,
        signal,
      }),
    );
    return answer.data as RecallResponse;
  }

  /** One memory with its body. */
  async get(artifactId: string): Promise<Memory> {
    const answer = await this.#send((signal) =>
      this.#api.GET("/v1/memories/{artifact_id}", {
        params: { path: { artifact_id: artifactId } },
        signal,
      }),
    );
    return answer.data as Memory;
  }

  /** One page of `GET /v1/memories`, newest first, with its `next_cursor`. */
  async listMemories(query: ListQuery = {}, options: { workarea?: string } = {}): Promise<MemoryPage> {
    const answer = await this.#send((signal) =>
      this.#api.GET("/v1/memories", {
        params: {
          query,
          header: options.workarea ? { "X-OML-Workarea": options.workarea } : {},
        },
        signal,
      }),
    );
    return answer.data as MemoryPage;
  }

  /**
   * Every page of the listing, following `next_cursor` until it is null, with the same filters on
   * every page. Cursors are positions, not snapshots: a row that changes between pages can be
   * missed or repeated.
   */
  async *pages(
    query: Omit<ListQuery, "cursor"> = {},
    options: { workarea?: string } = {},
  ): AsyncGenerator<MemoryPage> {
    let cursor: string | undefined;
    for (;;) {
      const page = await this.listMemories({ ...query, ...(cursor ? { cursor } : {}) }, options);
      yield page;
      if (!page.next_cursor) return;
      cursor = page.next_cursor;
    }
  }

  /** Every row of the listing, across pages; see `pages`. */
  async *memories(
    query: Omit<ListQuery, "cursor"> = {},
    options: { workarea?: string } = {},
  ): AsyncGenerator<MemorySummary> {
    for await (const page of this.pages(query, options)) yield* page.items;
  }

  /** Propose a successor of the served tip `artifactId`, held like any write; `retirement: true` proposes retiring it. */
  supersede(
    artifactId: string,
    body: SupersedeRequest,
    options: WriteOptions = {},
  ): Promise<Written<SubmitAccepted>> {
    return this.#write(options.idempotencyKey, (key, signal) =>
      this.#api.POST("/v1/memories/{artifact_id}/supersede", {
        params: {
          path: { artifact_id: artifactId },
          header: {
            "Idempotency-Key": key,
            ...(options.workarea ? { "X-OML-Workarea": options.workarea } : {}),
          },
        },
        body,
        signal,
      }),
    );
  }

  /** Not built: the server serves no upload sessions yet, so this rejects before sending anything. */
  async uploadAttachment(_data: Uint8Array, _options: { mediaType: string }): Promise<never> {
    throw new NotBuiltError(
      "attachment uploads are not built: the server serves no upload sessions yet (FR-12)",
    );
  }

  // -- the review ceremony (/v2), for a dreamer key ---------------------------

  /** Serve exactly one held item to this reviewer, or null when nothing waits. */
  async reviewNext(): Promise<Serve | null> {
    const key = crypto.randomUUID();
    const answer = await this.#send((signal) =>
      this.#api.POST("/v2/review/next", {
        params: { header: { "Idempotency-Key": key } },
        signal,
      }),
    );
    return answer.response.status === 200 ? (answer.data as Serve) : null;
  }

  /** Record `approve` or `reject` (with a reason) against the `serveId` that `reviewNext` returned. */
  verdict(
    serveId: string,
    verdict: "approve" | "reject",
    options: { reason?: string; idempotencyKey?: string } = {},
  ): Promise<Written<VerdictResult>> {
    return this.#write(options.idempotencyKey, (key, signal) =>
      this.#api.POST("/v2/review/verdicts", {
        params: { header: { "Idempotency-Key": key } },
        body: {
          serve_id: serveId,
          verdict,
          ...(options.reason !== undefined ? { reason: options.reason } : {}),
        },
        signal,
      }),
    );
  }
}
