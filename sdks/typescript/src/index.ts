// oml-sdk: a TypeScript client for the Open Memory Lab HTTP API.
// src/generated/schema.ts is generated from docs/api/openapi.yaml and never
// edited; OmlClient is the hand-written wrapper (docs/engineering/sdks.md).

export {
  type ListQuery,
  type LearnInput,
  type Memory,
  type MemoryPage,
  type MemorySummary,
  OmlClient,
  type OmlClientOptions,
  type RecallRequest,
  type RecallResponse,
  type RememberInput,
  type Serve,
  type SubmitAccepted,
  type SubmitRequest,
  type SupersedeRequest,
  type VerdictResult,
  type WriteOptions,
  type Written,
} from "./client.ts";
export {
  AccessError,
  ApiError,
  type ApiErrorFields,
  AuthError,
  ConfigError,
  ExportError,
  FAMILIES,
  IdempotencyError,
  LifecycleError,
  LimitError,
  NotBuiltError,
  OmlError,
  RequestError,
  ReviewError,
  ServerError,
  SettingsError,
  SimulationError,
  SubmissionError,
  TransportError,
  UploadError,
  VocabError,
  WorkareaError,
  errorFromResponse,
} from "./errors.ts";
export type { components, operations, paths } from "./generated/schema.ts";
