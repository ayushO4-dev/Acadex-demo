//! Request spans that join the caller's trace via the W3C `traceparent` header,
//! so a page render in Next.js and the API calls it makes show up as one trace.

use std::time::Duration;

use axum::{
    body::Body,
    extract::MatchedPath,
    http::{HeaderMap, Request, Response},
};
use opentelemetry::{global, propagation::Extractor, trace::TraceContextExt};
use tracing::{Span, field::Empty};
use tracing_opentelemetry::OpenTelemetrySpanExt;

pub fn request_span(request: &Request<Body>) -> Span {
    // Route template (`/students/{id}`), not the raw path, to keep span names low-cardinality.
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or(request.uri().path(), MatchedPath::as_str);

    let span = tracing::info_span!(
        "http.request",
        otel.name = %format!("{} {route}", request.method()),
        otel.kind = "server",
        http.request.method = %request.method(),
        http.route = route,
        http.response.status_code = Empty,
    );

    let parent = global::get_text_map_propagator(|propagator| {
        propagator.extract(&HeaderExtractor(request.headers()))
    });
    // Only fails when the OpenTelemetry layer isn't installed (export disabled).
    let _ = span.set_parent(parent);
    span
}

pub fn record_response<B>(response: &Response<B>, _latency: Duration, span: &Span) {
    span.record("http.response.status_code", response.status().as_u16());
}

/// The current trace id, for audit rows and error responses. `None` when span
/// export is off.
pub fn current_trace_id() -> Option<String> {
    let context = Span::current().context();
    let span_context = context.span().span_context().clone();
    span_context
        .is_valid()
        .then(|| span_context.trace_id().to_string())
}

struct HeaderExtractor<'a>(&'a HeaderMap);

impl Extractor for HeaderExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|value| value.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(|key| key.as_str()).collect()
    }
}
