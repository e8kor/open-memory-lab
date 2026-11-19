// The hand-written wrapper over the generated types and openapi-fetch
// (docs/engineering/sdks.md). It adds what a generator cannot know: where the
// URL and key come from, one Idempotency-Key per write that every retry of the
// call reuses, cursor iteration, the one error type every refusal raises, and
// the seven verbs of ADR-0039 d10 — write, read, workspace, events, reviewNext,
// approve and reject — whose fields choose the route as oml_write's and
// oml_read's choose theirs. Every path, parameter and body is typed by the
// generated schema.

import createClient, { type Client } from "openapi-fetch";

import { ConfigError, TransportError, errorFromResponse, refusedLocally } from "./errors.ts";
import type { components, operations, paths } from "./generated/schema.ts";

/**
 * What every request sends as User-Agent, where the runtime lets a request set it (Node does;
 * a browser keeps its own): the server's request log names the client by it (ADR-0042 d9).
 * The version is package.json's, which a unit test holds it to.
 */
export const USER_AGENT = "oml-sdk-typescript/0.8.3";

type Schemas = components["schemas"];
export type SubmitRequest = Schemas["SubmitRequest"];
export type SubmitAccepted = Schemas["SubmitAccepted"];
export type RecallRequest = Schemas["RecallRequest"];
export type RecallResponse = Schemas["RecallResponse"];
export type Memory = Schemas["Memory"];
export type MemorySummary = Schemas["MemorySummary"];
export type MemoryPage = Schemas["MemoryPage"];
export type SupersedeRequest = Schemas["SupersedeRequest"];
export type WithdrawRequest = Schemas["WithdrawRequest"];
export type WithdrawResult = Schemas["WithdrawResult"];
export type Serve = Schemas["Serve"];
export type VerdictResult = Schemas["VerdictResult"];
export type WorkspaceView = Schemas["WorkspaceView"];
export type EventsPage = Schemas["EventsPage"];
export type SkillSet = Schemas["SkillSet"];
export type ServedSkill = Schemas["ServedSkill"];
export type ListQuery = NonNullable<operations["listMemories"]["parameters"]["query"]>;
export type EventsQuery = NonNullable<operations["tailEvents"]["parameters"]["query"]>;
/** `skills`' fields: the scope node, whether the key's own held skills are offered too, and a page's cursor. */
export type SkillsQuery = NonNullable<operations["getSkills"]["parameters"]["query"]>;

/**
 * `write`'s one flat field set (ADR-0039 d3): without `replaces` a submission, the server deriving
 * the verb from `derives_from` and, omitted, the kind and the claim key; with `replaces` a
 * successor of that served tip, or with `retirement: true` a proposal to retire it.
 */
export interface WriteInput {
  /** The claim's text; with `retirement: true`, why the claim is retired. */
  text: string;
  claim_key?: SubmitRequest["claim_key"];
  kind?: SubmitRequest["kind"];
  derives_from?: string[];
  workspace?: SubmitRequest["workspace"];
  /** The served tip this write supersedes. */
  replaces?: string;
  /** With `replaces`, required: the claim's latest version as last read (ADR-0057 d19). */
  expected_version?: SupersedeRequest["expected_version"];
  /** The origin the author declares (ADR-0057 d1). */
  anchor?: SubmitRequest["anchor"];
  /** The media type of `text`, `text/plain` when omitted (ADR-0056 d1). */
  format?: SubmitRequest["format"];
  /** One line stating the fact `text` shows; required with a format that is not prose (ADR-0056 d10). */
  title?: SubmitRequest["title"];
  /** What kind of source the text came from; omitted, recorded `manual` (ADR-0063 d1, d2). */
  origin?: SubmitRequest["origin"];
  retirement?: SupersedeRequest["retirement"];
  reason?: SupersedeRequest["reason"];
  /**
   * A skill's name and description: required on kind `skill`, refused on every other kind
   * (ADR-0080 d1). A supersession of a skill sends them again, as every version carries its own.
   */
  skill?: SubmitRequest["skill"];
  /** A document's title: optional on kind `document`, refused on every other kind (ADR-0081 d1). */
  document?: SubmitRequest["document"];
}

/**
 * The Markdown fence info string of a claim's media type, so a client that renders fenced blocks
 * can show a claim by its format: `mermaid`, `json`, `yaml`, the `lang` of a code snippet,
 * `markdown`, and the empty string for plain text and for a media type this SDK does not know
 * (ADR-0056 d9).
 */
export function fenceFor(format?: string | null): string {
  const [media = "", ...params] = (format ?? "text/plain").split(";");
  switch (media.trim()) {
    case "text/markdown":
      return "markdown";
    case "text/vnd.oml.mermaid":
      return "mermaid";
    case "application/json":
      return "json";
    case "application/yaml":
      return "yaml";
    case "text/vnd.oml.code": {
      for (const part of params) {
        const [name, value] = part.trim().split("=");
        if (name === "lang" && value) return value.trim();
      }
      return "";
    }
    default:
      return "";
  }
}

/** `read`'s fetch: one version with its text. */
export interface GetInput {
  artifact_id: string;
  /** The version as it was at this past instant: not found when recorded after it (ADR-0059 d19). */
  as_of?: string;
}
/** `read`'s recall: ranked hits over promoted rows, and the caller's held rows with `include_unreviewed`. */
export type RecallInput = RecallRequest;
/** `read`'s listing: the catalog of `workspace`'s subtree, or with `own: true` the key's own rows. */
export type ListingInput = ListQuery;
/** Every field of `read`'s three modes; the fields set choose the mode (ADR-0039 d4). */
export type ReadInput = Partial<GetInput & RecallInput & ListingInput>;

export interface OmlClientOptions {
  /** The server, without /mcp; else OML_URL, read only when apiKey is omitted too. */
  url?: string;
  /** The key; else OML_API_KEY, read only when url is omitted too. */
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
}

/** A write's result, the Idempotency-Key every attempt of it sent, and whether the server replayed it. */
export interface Written<T> {
  value: T;
  idempotencyKey: string;
  replayed: boolean;
}

const RETRY_AFTER_CAP_S = 30; // the most a Retry-After is waited for
const FIRST_BACKOFF_MS = 200; // doubling per attempt, when no Retry-After is sent

// The rules errors.md states for the two codes a refusal raised before sending
// carries, as the server's refusal of the same arguments would.
const VALIDATION_RULE =
  "The contract is openapi.yaml; what it does not admit is refused, not coerced.";
const UNRESOLVED_RULE =
  "The target workspace comes from the request's workspace or, omitted, from the key's " +
  "default_workspace. A key with neither is refused, never guessed for.";

// The fields of read's modes: oml_read's (ADR-0039 d4), and include_text,
// which the HTTP recall serves beside them.
const READ_GET: readonly string[] = ["artifact_id", "as_of"];
const READ_RECALL: readonly string[] = [
  "as_of",
  "query",
  "workspace",
  "k",
  "kinds",
  "include_text",
  "include_unreviewed",
  "max_tokens",
  "search",
  "match",
  "anchor_repo",
  "anchor_path_prefix",
  "origin_kinds",
  "sources",
];
const READ_LISTING: readonly string[] = [
  "as_of",
  "own",
  "workspace",
  "claim_key",
  "state",
  "kinds",
  "prefix",
  "include_text",
  "since",
  "until",
  "limit",
  "cursor",
  "anchor_repo",
  "anchor_path_prefix",
  "origin_kinds",
];

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

function fromEnv(name: string): string | undefined {
  const env = (globalThis as { process?: { env?: Record<string, string | undefined> } }).process
    ?.env;
  return env?.[name] || undefined;
}

/**
 * The URL-and-key pair from the first source that sets either value: the options, then OML_URL
 * and OML_API_KEY. A source setting one alone is refused naming the other, so a key is never sent
 * to a URL from another source (ADR-0039 d15, the CLI's rule).
 */
function startPair(url: string | undefined, apiKey: string | undefined): [string, string] {
  const sources: [string, string, string, string | undefined, string | undefined][] = [
    ["url", "apiKey", "passed", url || undefined, apiKey || undefined],
    ["OML_URL", "OML_API_KEY", "set", fromEnv("OML_URL"), fromEnv("OML_API_KEY")],
  ];
  for (const [urlName, keyName, verb, foundUrl, foundKey] of sources) {
    if (foundUrl && foundKey) return [foundUrl, foundKey];
    if (foundUrl) throw new ConfigError(`${urlName} is ${verb} without ${keyName}`);
    if (foundKey) throw new ConfigError(`${keyName} is ${verb} without ${urlName}`);
  }
  throw new ConfigError("no URL and API key: pass url and apiKey, or set OML_URL and OML_API_KEY");
}

/** `request.validation` naming `field`, raised before anything is sent. */
function invalid(field: string, message: string) {
  return refusedLocally(400, "request.validation", message, VALIDATION_RULE, { field: `/${field}` });
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
  readonly #url: string;
  readonly #retries: number;
  readonly #timeoutMs: number;

  /**
   * It starts from one pair: `url` and `apiKey`, else OML_URL and OML_API_KEY, taken from the
   * first source that sets either. A source setting one alone, or no source at all, throws
   * ConfigError before anything is sent (ADR-0039 d15).
   */
  constructor(options: OmlClientOptions = {}) {
    const [url, apiKey] = startPair(options.url, options.apiKey);
    const retries = options.retries ?? 2;
    if (!Number.isInteger(retries) || retries < 0) throw new ConfigError("retries must be 0 or more");
    this.#url = url.replace(/\/+$/, "");
    this.#retries = retries;
    this.#timeoutMs = options.timeoutMs ?? 10_000;
    this.#api = createClient<paths>({
      baseUrl: this.#url,
      headers: { Authorization: `Bearer ${apiKey}`, "User-Agent": USER_AGENT },
      ...(options.fetch ? { fetch: options.fetch } : {}),
    });
  }

  toString(): string {
    return `OmlClient(${this.#url})`;
  }

  toJSON(): { url: string } {
    return { url: this.#url };
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

  /**
   * Write a claim, held on arrival; the fields set choose the route (ADR-0039 d3). `retirement` or
   * `reason` without `replaces`, and `claim_key` with it, reject with `request.validation` naming
   * the field before anything is sent. Only the fields given are sent.
   */
  async write(input: WriteInput, options: WriteOptions = {}): Promise<Written<SubmitAccepted>> {
    const { text, replaces, claim_key, retirement, reason, expected_version, ...shared } = input;
    if (replaces === undefined) {
      for (const [field, value] of [
        ["retirement", retirement],
        ["reason", reason],
        ["expected_version", expected_version],
      ] as const) {
        if (value !== undefined) {
          throw invalid(
            field,
            `${field} is refused without replaces: it belongs to a supersession, so pass replaces naming the version it changes`,
          );
        }
      }
      const body: SubmitRequest = { body: text, ...shared, ...(claim_key !== undefined ? { claim_key } : {}) };
      return this.#write(options.idempotencyKey, (key, signal) =>
        this.#api.POST("/v1/memories", {
          params: { header: { "Idempotency-Key": key } },
          body,
          signal,
        }),
      );
    }
    if (claim_key !== undefined) {
      throw invalid("claim_key", "claim_key is refused with replaces: the new version takes its target's claim_key");
    }
    if (expected_version === undefined) {
      throw invalid(
        "expected_version",
        "expected_version is required with replaces: the claim's latest version as last read, so a supersession never replaces a version it did not see",
      );
    }
    const body: SupersedeRequest = {
      body: text,
      expected_version,
      ...shared,
      ...(retirement !== undefined ? { retirement } : {}),
      ...(reason !== undefined ? { reason } : {}),
    };
    return this.#write(options.idempotencyKey, (key, signal) =>
      this.#api.POST("/v1/memories/{artifact_id}/supersede", {
        params: { path: { artifact_id: replaces }, header: { "Idempotency-Key": key } },
        body,
        signal,
      }),
    );
  }

  /**
   * Withdraw a held version this key wrote, for a mistaken write: it is kept as withdrawn, leaves
   * every review queue and is never served; no reviewer is involved (ADR-0083 d13). Only a held
   * version is withdrawn (`memory.not_held`), and only by its author (`access.author_only`).
   */
  async withdraw(
    artifact_id: string,
    input: { reason?: string } = {},
    options: WriteOptions = {},
  ): Promise<Written<WithdrawResult>> {
    const body: WithdrawRequest = input.reason !== undefined ? { reason: input.reason } : {};
    return this.#write(options.idempotencyKey, (key, signal) =>
      this.#api.POST("/v1/memories/{artifact_id}/withdraw", {
        params: { path: { artifact_id }, header: { "Idempotency-Key": key } },
        body,
        signal,
      }),
    );
  }

  /**
   * Read in oml_read's three modes (ADR-0039 d4): `artifact_id` fetches one version with its text;
   * `query` recalls over promoted rows in `workspace`, else the key's default workspace, and the
   * caller's own held rows with `include_unreviewed`; neither lists one page of the catalog, newest
   * first: what the key reads in `workspace`'s subtree, narrowed by `kinds`, `prefix` and `state`,
   * each item without its text unless `include_text`, or with `own: true` the key's own rows with
   * their text. `as_of`, in every
   * mode, reads memory as it was at that past instant under today's access (ADR-0059 d14). A field outside
   * the mode, and `query` beside `artifact_id`, reject with `request.validation` naming it before
   * anything is sent; only the fields given are sent.
   */
  read(input: GetInput): Promise<Memory>;
  read(input: RecallInput): Promise<RecallResponse>;
  read(input?: ListingInput): Promise<MemoryPage>;
  read(input: ReadInput): Promise<Memory | RecallResponse | MemoryPage>;
  async read(input: ReadInput = {}): Promise<Memory | RecallResponse | MemoryPage> {
    const sent = Object.entries(input).filter(([, value]) => value !== undefined);
    const fields = new Set(sent.map(([field]) => field));
    if (fields.has("artifact_id") && fields.has("query")) {
      throw invalid(
        "query",
        "query is refused with artifact_id: artifact_id fetches one version and query recalls, so pass one",
      );
    }
    const [mode, allowed] = fields.has("artifact_id")
      ? (["get", READ_GET] as const)
      : fields.has("query")
        ? (["recall", READ_RECALL] as const)
        : (["listing", READ_LISTING] as const);
    const outside = sent.find(([field]) => !allowed.includes(field));
    if (outside) {
      throw invalid(
        outside[0],
        `${outside[0]} is not an argument of read's ${mode} mode: artifact_id fetches, query recalls, and neither lists`,
      );
    }
    const given = Object.fromEntries(sent);
    if (mode === "get") {
      const answer = await this.#send((signal) =>
        this.#api.GET("/v1/memories/{artifact_id}", {
          params: {
            path: { artifact_id: String(given.artifact_id) },
            ...(given.as_of !== undefined ? { query: { as_of: String(given.as_of) } } : {}),
          },
          signal,
        }),
      );
      return answer.data as Memory;
    }
    if (mode === "recall") {
      const answer = await this.#send((signal) =>
        this.#api.POST("/v1/memories/recall", { body: given as RecallRequest, signal }),
      );
      return answer.data as RecallResponse;
    }
    return this.#listing(given as ListingInput);
  }

  /**
   * One page of `GET /v1/memories`: the catalog unless `own` is true, as oml_read's listing mode
   * reads it (ADR-0092 d6). The key's own work, `own: true`, carries each item's text, as the
   * own-work check needs it (ADR-0092 d12 reading 3); the catalog carries it only with
   * `include_text`.
   */
  async #listing(query: ListingInput): Promise<MemoryPage> {
    const own = Boolean(query.own);
    const sent = { ...query, own, ...(own ? { include_text: true } : {}) };
    const answer = await this.#send((signal) =>
      this.#api.GET("/v1/memories", { params: { query: sent }, signal }),
    );
    return answer.data as MemoryPage;
  }

  /**
   * Every page of `read`'s listing, following `next_cursor` until it is null, with the same filters
   * on every page. Cursors are positions, not snapshots: a row that changes between pages can be
   * missed or repeated.
   */
  async *pages(query: Omit<ListingInput, "cursor"> = {}): AsyncGenerator<MemoryPage> {
    let cursor: string | undefined;
    for (;;) {
      const page = await this.#listing({ ...query, ...(cursor ? { cursor } : {}) });
      yield page;
      if (!page.next_cursor) return;
      cursor = page.next_cursor;
    }
  }

  /** Every row of `read`'s listing, across pages; see `pages`. */
  async *memories(query: Omit<ListingInput, "cursor"> = {}): AsyncGenerator<MemorySummary> {
    for await (const page of this.pages(query)) yield* page.items;
  }

  /**
   * One page of the skills offered at `workspace`, else the key's default workspace: names,
   * descriptions and resource URIs, 50 a page, without bodies (ADR-0080 d11). `include_own_held`
   * offers the key's own held skills too, each marked unreviewed. A body is `read` with the skill's
   * `artifact_id`. `set_digest` is the same on every page of one set, so a caller compares it to
   * tell the set changed. Only the fields given are sent.
   */
  async skills(query: SkillsQuery = {}): Promise<SkillSet> {
    const answer = await this.#send((signal) => this.#api.GET("/v1/skills", { params: { query }, signal }));
    return answer.data as SkillSet;
  }

  /** Every skill of the offered set, across pages, following `next_cursor` until it is null; see `skills`. */
  async *iterSkills(query: Omit<SkillsQuery, "cursor"> = {}): AsyncGenerator<ServedSkill> {
    let cursor: string | undefined;
    for (;;) {
      const page = await this.skills({ ...query, ...(cursor ? { cursor } : {}) });
      yield* page.skills;
      if (!page.next_cursor) return;
      cursor = page.next_cursor;
    }
  }

  // -- workspaces and the journal (/v2) ----------------------------------------

  /**
   * One workspace node, by id or name, with this key's view of it: its children, edges, effective
   * verbs and permission epoch. Omitted, the key's default workspace, which `GET /v2/whoami` names;
   * a key with none rejects with `request.workspace_unresolved`.
   */
  async workspace(workspace?: string): Promise<WorkspaceView> {
    let node = workspace;
    if (node === undefined) {
      const me = await this.#send((signal) => this.#api.GET("/v2/whoami", { signal }));
      const fallback = me.data?.default_workspace;
      if (!fallback) {
        throw refusedLocally(
          422,
          "request.workspace_unresolved",
          "no workspace was named and this key has no default_workspace",
          UNRESOLVED_RULE,
          {},
        );
      }
      node = fallback.id;
    }
    const named = node;
    const answer = await this.#send((signal) =>
      this.#api.GET("/v2/workspaces/{workspace}", { params: { path: { workspace: named } }, signal }),
    );
    return answer.data as WorkspaceView;
  }

  /**
   * One page of the journal tail of `workspace`'s subtree, else the key's default workspace's, in seq
   * order after `after_seq`; pass the page's `next_after_seq` back to resume. Digests only, never
   * claim text.
   */
  async events(query: EventsQuery = {}): Promise<EventsPage> {
    const answer = await this.#send((signal) =>
      this.#api.GET("/v2/events", { params: { query }, signal }),
    );
    return answer.data as EventsPage;
  }

  // -- the review ceremony (/v2), for a reviewer key ---------------------------

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

  /**
   * Approve the `serve_id` that `reviewNext` returned, or, omitted, the one item this key has
   * claimed (ADR-0039 d10); the id is opaque and never built here.
   */
  approve(
    input: { serve_id?: string; reason?: string } = {},
    options: WriteOptions = {},
  ): Promise<Written<VerdictResult>> {
    return this.#verdict("approve", input, options);
  }

  /** Reject the `serve_id` that `reviewNext` returned, or, omitted, the one item this key has claimed, with the reason a rejection requires. */
  reject(
    input: { serve_id?: string; reason: string },
    options: WriteOptions = {},
  ): Promise<Written<VerdictResult>> {
    return this.#verdict("reject", input, options);
  }

  #verdict(
    verdict: "approve" | "reject",
    input: { serve_id?: string; reason?: string },
    options: WriteOptions,
  ): Promise<Written<VerdictResult>> {
    return this.#write(options.idempotencyKey, (key, signal) =>
      this.#api.POST("/v2/review/verdicts", {
        params: { header: { "Idempotency-Key": key } },
        body: {
          ...(input.serve_id !== undefined ? { serve_id: input.serve_id } : {}),
          verdict,
          ...(input.reason !== undefined ? { reason: input.reason } : {}),
        },
        signal,
      }),
    );
  }
}
