//! OTLP trace export, off unless `server.otlp_endpoint` names a collector
//! (ADR-0042 d13). Spans go as OTLP/HTTP protobuf to
//! `<endpoint>/v1/traces` through the SDK's batch processor, whose queue is
//! bounded: a full queue drops spans, and a request never waits on export.
//! Only OML's setting configures it: the exporter here reads no `OTEL_*`
//! variable, the sampler, resource, limits and batch sizes are set
//! explicitly over the SDK's environment defaults, and no authentication
//! header is sent, since the supported shape is a collector beside the lead.

