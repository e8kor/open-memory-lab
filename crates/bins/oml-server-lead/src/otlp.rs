//! OTLP trace export, off unless `server.otlp_endpoint` names a collector
//! (ADR-0042 d13). Spans go as OTLP/HTTP protobuf to
//! `<endpoint>/v1/traces` through the SDK's batch processor, whose queue is
//! bounded: a full queue drops spans, and a request never waits on export.
//! Only OML's setting configures it: the exporter here reads no `OTEL_*`
//! variable, the sampler, resource, limits and batch sizes are set
//! explicitly over the SDK's environment defaults, and no authentication
//! header is sent, since the supported shape is a collector beside the lead.

use std::sync::Arc;
use std::time::Duration;

use opentelemetry::Context;
use opentelemetry::trace::{
    SpanContext, SpanId, TraceContextExt as _, TraceFlags, TraceId, TraceState, TracerProvider as _,
};
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::transform::common::tonic::ResourceAttributesWithSchema;
use opentelemetry_proto::transform::trace::tonic::group_spans_by_resource_and_scope;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::error::{OTelSdkError, OTelSdkResult};
use opentelemetry_sdk::trace::{
    BatchConfigBuilder, BatchSpanProcessor, Sampler, SdkTracerProvider, SpanData, SpanExporter,
    SpanLimits, Tracer,
};
use prost::Message as _;
use tracing_opentelemetry::OpenTelemetrySpanExt as _;

use oml_transport_http::traffic::RequestTracer;

/// The stable `event` of an export that failed (observability.md).
pub const EVENT_OTLP_EXPORT_FAILED: &str = "otlp_export_failed";

/// The batch processor's bounded queue, and its batch and delay.
const MAX_QUEUE: usize = 2048;
const MAX_BATCH: usize = 512;
const SCHEDULED_DELAY: Duration = Duration::from_secs(5);
const EXPORT_TIMEOUT: Duration = Duration::from_secs(10);

/// `server.otlp_endpoint`: an `http` or `https` base URL, with no query,
/// fragment or credentials (ADR-0042 d13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint(String);

impl Endpoint {
    /// The setting's value as an endpoint; any other value stops the start.
    pub fn parse(value: &str) -> Result<Self, String> {
        let refused = |why: &str| {
            format!(
                "server.otlp_endpoint {value:?} is not an http or https base URL: {why} (configuration.md; ADR-0042 d13)"
            )
        };
        let url = reqwest::Url::parse(value).map_err(|e| refused(&e.to_string()))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(refused("the scheme is neither http nor https"));
        }
        if url.host_str().is_none_or(str::is_empty) {
            return Err(refused("it names no host"));
        }
        if url.query().is_some() || url.fragment().is_some() {
            return Err(refused("it carries a query or a fragment"));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(refused("it carries credentials"));
        }
        Ok(Self(value.trim_end_matches('/').to_owned()))
    }

    /// Where the spans are posted.
    #[must_use]
    pub fn traces_url(&self) -> String {
        format!("{}/v1/traces", self.0)
    }
}

/// The exporter: one protobuf POST per batch, with no header but its
/// content type.
#[derive(Debug)]
struct Exporter {
    client: reqwest::blocking::Client,
    url: String,
    resource: ResourceAttributesWithSchema,
}

impl Exporter {
    fn send(&self, batch: Vec<SpanData>) -> OTelSdkResult {
        let body = ExportTraceServiceRequest {
            resource_spans: group_spans_by_resource_and_scope(batch, &self.resource),
        }
        .encode_to_vec();
        let failed = |reason: String| {
            tracing::error!(event = EVENT_OTLP_EXPORT_FAILED, %reason, "trace export failed; the batch is dropped");
            Err(OTelSdkError::InternalFailure(reason))
        };
        match self
            .client
            .post(&self.url)
            .header(reqwest::header::CONTENT_TYPE, "application/x-protobuf")
            .body(body)
            .send()
        {
            Ok(response) if response.status().is_success() => Ok(()),
            Ok(response) => failed(format!("the collector answered {}", response.status())),
            Err(e) => failed(e.to_string()),
        }
    }
}

impl SpanExporter for Exporter {
    // The batch processor calls this on its own thread and waits on the
    // future, so the blocking client's POST runs here, never on a request.
    fn export(&self, batch: Vec<SpanData>) -> impl Future<Output = OTelSdkResult> + Send {
        let result = self.send(batch);
        async move { result }
    }

    fn set_resource(&mut self, resource: &Resource) {
        self.resource = resource.into();
    }
}

/// Trace export, running: the provider to shut down and the tracer the
/// subscriber's OpenTelemetry layer exports through.
#[derive(Debug)]
pub struct Export {
    provider: SdkTracerProvider,
    tracer: Tracer,
}

impl Export {
    /// Starts export to `endpoint`. The HTTP client is built on a thread of
    /// its own, since a blocking client may not be built inside the async
    /// runtime, over rustls with ring.
    pub fn start(endpoint: &Endpoint) -> Result<Self, String> {
        let _already = rustls::crypto::ring::default_provider().install_default();
        let client = std::thread::spawn(|| {
            reqwest::blocking::Client::builder()
                .timeout(EXPORT_TIMEOUT)
                .build()
        })
        .join()
        .map_err(|_| "the trace export client could not be built".to_owned())?
        .map_err(|e| format!("the trace export client: {e}"))?;
        let exporter = Exporter {
            client,
            url: endpoint.traces_url(),
            resource: ResourceAttributesWithSchema::default(),
        };
        // Every value set here, so no OTEL_BSP_* variable moves it.
        let batch = BatchConfigBuilder::default()
            .with_max_queue_size(MAX_QUEUE)
            .with_max_export_batch_size(MAX_BATCH)
            .with_scheduled_delay(SCHEDULED_DELAY)
            .build();
        let provider = SdkTracerProvider::builder()
            .with_span_processor(
                BatchSpanProcessor::builder(exporter)
                    .with_batch_config(batch)
                    .build(),
            )
            // Parent-based: a root span is always sampled, a child follows
            // its parent's flag (ADR-0042 d13).
            .with_sampler(Sampler::ParentBased(Box::new(Sampler::AlwaysOn)))
            .with_span_limits(SpanLimits::default())
            .with_resource(
                Resource::builder_empty()
                    .with_service_name("oml-server-lead")
                    .build(),
            )
            .build();
        let tracer = provider.tracer("oml-server-lead");
        Ok(Self { provider, tracer })
    }

    /// The tracer the subscriber's OpenTelemetry layer exports through.
    #[must_use]
    pub fn tracer(&self) -> Tracer {
        self.tracer.clone()
    }

    /// Flushes what is queued and stops the processor's thread; call it off
    /// the async runtime, as it blocks.
    pub fn shutdown(&self) {
        let _ = self.provider.shutdown();
    }

    /// The layer's hook: `traceparent` adopted, the trace id named.
    #[must_use]
    pub fn request_tracer(&self) -> Arc<dyn RequestTracer> {
        Arc::new(LeadTracer)
    }
}

/// A W3C `traceparent` as d13 admits it: version `00`, a 32-hex trace id
/// and a 16-hex parent id that are not all zeros, two hex flag digits.
#[must_use]
pub fn traceparent(value: &[u8]) -> Option<SpanContext> {
    let value = std::str::from_utf8(value).ok()?;
    let mut parts = value.split('-');
    let (version, trace, parent, flags) =
        (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    let lower_hex = |s: &str, n: usize| {
        s.len() == n
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    if parts.next().is_some()
        || version != "00"
        || !lower_hex(trace, 32)
        || !lower_hex(parent, 16)
        || !lower_hex(flags, 2)
    {
        return None;
    }
    let trace_id = TraceId::from_hex(trace)
        .ok()
        .filter(|t| *t != TraceId::INVALID)?;
    let span_id = SpanId::from_hex(parent)
        .ok()
        .filter(|s| *s != SpanId::INVALID)?;
    let flags = u8::from_str_radix(flags, 16).ok()?;
    Some(SpanContext::new(
        trace_id,
        span_id,
        TraceFlags::new(flags) & TraceFlags::SAMPLED,
        true,
        TraceState::default(),
    ))
}

/// The lead's [`RequestTracer`]: a valid `traceparent` makes the request's
/// span its child and an invalid one starts a new trace; `tracestate` is
/// not read (ADR-0042 d13).
#[derive(Debug)]
struct LeadTracer;

impl RequestTracer for LeadTracer {
    fn adopt(
        &self,
        span: &tracing::Span,
        traceparent_value: Option<&axum::http::HeaderValue>,
    ) -> Option<String> {
        if let Some(parent) = traceparent_value.and_then(|v| traceparent(v.as_bytes())) {
            let _ = span.set_parent(Context::new().with_remote_span_context(parent));
        }
        let trace_id = span.context().span().span_context().trace_id();
        (trace_id != TraceId::INVALID).then(|| trace_id.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_traceparent_is_admitted_only_as_version_00_with_nonzero_ids_and_hex_flags() {
        let ok =
            traceparent(b"00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01").expect("valid");
        assert_eq!(
            ok.trace_id().to_string(),
            "4bf92f3577b34da6a3ce929d0e0e4736"
        );
        assert!(ok.is_sampled() && ok.is_remote());
        for bad in [
            "01-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            "00-00000000000000000000000000000000-00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01",
            "00-4BF92F3577B34DA6A3CE929D0E0E4736-00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-1",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-x",
        ] {
            assert!(traceparent(bad.as_bytes()).is_none(), "{bad}");
        }
    }

    #[test]
    fn the_endpoint_is_an_http_or_https_base_url_and_spans_go_to_v1_traces() {
        let endpoint = Endpoint::parse("http://127.0.0.1:4318/").expect("a base URL");
        assert_eq!(endpoint.traces_url(), "http://127.0.0.1:4318/v1/traces");
        for bad in [
            "grpc://collector:4317",
            "collector:4318",
            "http://collector:4318/?x=1",
            "https://user:pw@collector",
            "",
        ] {
            assert!(Endpoint::parse(bad).is_err(), "{bad}");
        }
    }
}
