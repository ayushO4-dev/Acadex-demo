//! Tracing and OpenTelemetry setup shared by every Acadex binary.
//!
//! Spans are exported over OTLP/HTTP when `OTEL_EXPORTER_OTLP_ENDPOINT` is set:
//! Jaeger locally, Cloud Trace on Google Cloud. Without it, logs go to stdout only.

use anyhow::Result;
use opentelemetry::{global, trace::TracerProvider as _};
use opentelemetry_otlp::SpanExporter;
use opentelemetry_sdk::{Resource, propagation::TraceContextPropagator, trace::SdkTracerProvider};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

/// Flushes buffered spans when dropped; hold it for the whole of `main`.
#[must_use = "dropping the guard immediately stops span export"]
pub struct TelemetryGuard {
    provider: Option<SdkTracerProvider>,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        if let Some(provider) = self.provider.take()
            && let Err(err) = provider.shutdown()
        {
            eprintln!("failed to flush spans: {err}");
        }
    }
}

pub fn init(service_name: &'static str) -> Result<TelemetryGuard> {
    // W3C `traceparent`, so spans from Next.js and Rust join one trace.
    global::set_text_map_propagator(TraceContextPropagator::new());

    let provider = if std::env::var_os("OTEL_EXPORTER_OTLP_ENDPOINT").is_some() {
        let exporter = SpanExporter::builder().with_http().build()?;
        let provider = SdkTracerProvider::builder()
            .with_batch_exporter(exporter)
            .with_resource(Resource::builder().with_service_name(service_name).build())
            .build();
        global::set_tracer_provider(provider.clone());
        Some(provider)
    } else {
        None
    };

    let otel_layer = provider
        .as_ref()
        .map(|p| tracing_opentelemetry::layer().with_tracer(p.tracer(service_name)));

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(otel_layer)
        .with(tracing_subscriber::fmt::layer())
        .try_init()?;

    Ok(TelemetryGuard { provider })
}
