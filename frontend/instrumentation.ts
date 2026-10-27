import { registerOTel } from "@vercel/otel";

// Exports spans over OTLP when OTEL_EXPORTER_OTLP_ENDPOINT is set
// (Jaeger locally, Cloud Trace on Google Cloud).
export function register() {
  registerOTel({
    serviceName: "acadex-web",
    instrumentationConfig: {
      fetch: {
        // Forward `traceparent` to the Rust API so both services share one trace.
        propagateContextUrls: [process.env.ACADEX_API_URL ?? "http://localhost:8080"],
      },
    },
  });
}
