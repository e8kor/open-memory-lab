// One exception class per error family of docs/api/errors.md.
//
// docs/engineering/sdks.md, "What the wrapper covers": the class is chosen by
// the code's family prefix, so a code this client has never seen lands in its
// family's class and a family it has never seen lands in ApiError.

/** Every error this package raises. */
export class OmlError extends Error {
  override name = "OmlError";
}

/** The client has no key or no server URL; nothing was sent. */
export class ConfigError extends OmlError {
  override name = "ConfigError";
}

/** The server does not serve this capability yet; nothing was sent. */
export class NotBuiltError extends OmlError {
  override name = "NotBuiltError";
}

/** The request failed in transit after the last retry; the cause is attached. */
export class TransportError extends OmlError {
  override name = "TransportError";
}

export interface ApiErrorFields {
  status: number;
  code: string | null;
  message: string;
  rule: string | null;
  retryable: boolean;
  details: Record<string, unknown>;
  requestId: string | null;
  retryAfter: number | null;
}

/** A refusal in the errors.md envelope. `code` is null only when the response carried no envelope. */
export class ApiError extends OmlError {
  override name = "ApiError";
  readonly status: number;
  readonly code: string | null;
  readonly rule: string | null;
  readonly retryable: boolean;
  readonly details: Record<string, unknown>;
  readonly requestId: string | null;
  readonly retryAfter: number | null;

  constructor(fields: ApiErrorFields) {
    super(fields.message);
    this.status = fields.status;
    this.code = fields.code;
    this.rule = fields.rule;
    this.retryable = fields.retryable;
    this.details = fields.details;
    this.requestId = fields.requestId;
    this.retryAfter = fields.retryAfter;
  }

  override toString(): string {
    let text = `${this.code ?? `HTTP ${this.status}`}: ${this.message}`;
    if (this.rule) text += ` (rule: ${this.rule})`;
    if (this.requestId) text += ` [${this.requestId}]`;
    return text;
  }
}

/** `auth`: who is calling. */
export class AuthError extends ApiError {
  override name = "AuthError";
}
/** `access`: what this key may do. */
export class AccessError extends ApiError {
  override name = "AccessError";
}
/** `request`: the request itself. */
export class RequestError extends ApiError {
  override name = "RequestError";
}
/** `idempotency`: the replay contract. */
export class IdempotencyError extends ApiError {
  override name = "IdempotencyError";
}
/** `submission`: admission refusals. */
export class SubmissionError extends ApiError {
  override name = "SubmissionError";
}
/** `name`: workarea, crew, key and tenant names (named as the Python client names it). */
export class NamingError extends ApiError {
  override name = "NamingError";
}
/** `workarea`: structure and status refusals. */
export class WorkareaError extends ApiError {
  override name = "WorkareaError";
}
/** `review`: independence and the serve binding. */
export class ReviewError extends ApiError {
  override name = "ReviewError";
}
/** `simulation`: the fence. */
export class SimulationError extends ApiError {
  override name = "SimulationError";
}
/** `vocab`: vocabulary structure refusals. */
export class VocabError extends ApiError {
  override name = "VocabError";
}
/** `upload`: attachment sessions. */
export class UploadError extends ApiError {
  override name = "UploadError";
}
/** `lifecycle`: erased content. */
export class LifecycleError extends ApiError {
  override name = "LifecycleError";
}
/** `limit`: rate and quota. */
export class LimitError extends ApiError {
  override name = "LimitError";
}
/** `export`: evidence export jobs. */
export class ExportError extends ApiError {
  override name = "ExportError";
}
/** `settings`: the settings document. */
export class SettingsError extends ApiError {
  override name = "SettingsError";
}
/** `system`: the replica cannot serve (named as the Python client names it). */
export class ServerError extends ApiError {
  override name = "ServerError";
}

export const FAMILIES: Readonly<Record<string, typeof ApiError>> = Object.freeze({
  auth: AuthError,
  access: AccessError,
  request: RequestError,
  idempotency: IdempotencyError,
  submission: SubmissionError,
  name: NamingError,
  workarea: WorkareaError,
  review: ReviewError,
  simulation: SimulationError,
  vocab: VocabError,
  upload: UploadError,
  lifecycle: LifecycleError,
  limit: LimitError,
  export: ExportError,
  settings: SettingsError,
  system: ServerError,
});

function retryAfter(headers: Headers): number | null {
  const value = headers.get("retry-after");
  if (value === null) return null;
  const seconds = Number(value);
  return Number.isFinite(seconds) ? Math.max(0, seconds) : null;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** The typed exception for one non-2xx response; `body` is its parsed JSON, or its text. */
export function errorFromResponse(status: number, body: unknown, headers: Headers): ApiError {
  const error = isRecord(body) ? body.error : undefined;
  if (!isRecord(error) || typeof error.code !== "string") {
    return new ApiError({
      status,
      code: null,
      message: `HTTP ${status} without the error envelope`,
      rule: null,
      retryable: false,
      details: {},
      requestId: null,
      retryAfter: retryAfter(headers),
    });
  }
  const family = error.code.split(".", 1)[0] ?? "";
  const Family = Object.hasOwn(FAMILIES, family) ? FAMILIES[family]! : ApiError;
  const requestId = isRecord(body) ? body.request_id : undefined;
  return new Family({
    status,
    code: error.code,
    message: typeof error.message === "string" ? error.message : "",
    rule: typeof error.rule === "string" ? error.rule : null,
    retryable: error.retryable === true,
    details: isRecord(error.details) ? error.details : {},
    requestId: typeof requestId === "string" ? requestId : null,
    retryAfter: retryAfter(headers),
  });
}
