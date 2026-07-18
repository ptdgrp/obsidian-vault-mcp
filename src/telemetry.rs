use std::time::Duration;

use opentelemetry::{KeyValue, trace::TracerProvider};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{Resource, logs::SdkLoggerProvider, trace::SdkTracerProvider};
use tracing_subscriber::{
    EnvFilter, Layer, Registry, layer::SubscriberExt, util::SubscriberInitExt,
};

pub(crate) fn telemetry_preview(value: &impl std::fmt::Debug) -> (String, bool) {
    const MAX_TELEMETRY_INPUT_BYTES: usize = 1024;
    const TRUNCATION_MARKER: &str = "…";

    let mut preview = format!("{value:?}");
    if preview.contains("Blueprint { command: Some(") {
        return ("Blueprint CLI input omitted".to_string(), true);
    }
    if preview.len() <= MAX_TELEMETRY_INPUT_BYTES {
        return (preview, false);
    }

    let mut cutoff = MAX_TELEMETRY_INPUT_BYTES - TRUNCATION_MARKER.len();
    while !preview.is_char_boundary(cutoff) {
        cutoff -= 1;
    }
    preview.truncate(cutoff);
    preview.push_str(TRUNCATION_MARKER);
    (preview, true)
}

pub(crate) struct TelemetryGuard {
    tracer_provider: Option<SdkTracerProvider>,
    logger_provider: Option<SdkLoggerProvider>,
}

impl TelemetryGuard {
    pub(crate) fn is_enabled(&self) -> bool {
        self.tracer_provider.is_some() || self.logger_provider.is_some()
    }

    pub(crate) fn shutdown(&mut self) {
        if let Some(provider) = self.tracer_provider.take() {
            if let Err(error) = provider.force_flush() {
                eprintln!("failed to force flush OpenTelemetry tracer provider: {error}");
            }
            if let Err(error) = provider.shutdown_with_timeout(Duration::from_secs(5)) {
                eprintln!("failed to shutdown OpenTelemetry tracer provider: {error}");
            }
        }
        if let Some(provider) = self.logger_provider.take() {
            if let Err(error) = provider.force_flush() {
                eprintln!("failed to force flush OpenTelemetry logger provider: {error}");
            }
            if let Err(error) = provider.shutdown_with_timeout(Duration::from_secs(5)) {
                eprintln!("failed to shutdown OpenTelemetry logger provider: {error}");
            }
        }
    }
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub(crate) fn init_tracing(
    level: &str,
    otel_endpoint: Option<&str>,
    otel_service_name: &str,
) -> anyhow::Result<TelemetryGuard> {
    let filter = telemetry_filter(level)?;
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_target(true)
        .with_filter(filter.clone());

    let Some(endpoint) = otel_endpoint.filter(|value| !value.trim().is_empty()) else {
        Registry::default().with(fmt_layer).init();
        return Ok(TelemetryGuard {
            tracer_provider: None,
            logger_provider: None,
        });
    };

    let trace_endpoint = otlp_signal_endpoint(endpoint, "v1/traces")?;
    let log_endpoint = otlp_signal_endpoint(endpoint, "v1/logs")?;
    let resource = Resource::builder()
        .with_service_name(otel_service_name.to_string())
        .with_attributes([KeyValue::new("service.version", env!("CARGO_PKG_VERSION"))])
        .build();

    let span_exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_protocol(opentelemetry_otlp::Protocol::HttpBinary)
        .with_endpoint(trace_endpoint)
        .build()?;
    let tracer_provider = SdkTracerProvider::builder()
        .with_resource(resource.clone())
        .with_batch_exporter(span_exporter)
        .build();
    let tracer = tracer_provider.tracer("obsidian-vault-mcp");
    let otel_trace_layer = tracing_opentelemetry::layer()
        .with_tracer(tracer)
        .with_filter(filter.clone());

    let log_exporter = opentelemetry_otlp::LogExporter::builder()
        .with_http()
        .with_protocol(opentelemetry_otlp::Protocol::HttpBinary)
        .with_endpoint(log_endpoint)
        .build()?;
    let logger_provider = SdkLoggerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(log_exporter)
        .build();
    let otel_log_layer =
        opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(&logger_provider)
            .with_filter(filter.clone());

    Registry::default()
        .with(fmt_layer)
        .with(otel_trace_layer)
        .with(otel_log_layer)
        .init();
    Ok(TelemetryGuard {
        tracer_provider: Some(tracer_provider),
        logger_provider: Some(logger_provider),
    })
}

fn telemetry_filter(input: &str) -> anyhow::Result<EnvFilter> {
    if input.contains('=') || input.contains(',') {
        return Ok(EnvFilter::try_new(input)?);
    }
    Ok(EnvFilter::try_new(format!(
        "obsidian_vault_mcp={input},warn"
    ))?)
}

fn otlp_signal_endpoint(base_endpoint: &str, signal_path: &str) -> anyhow::Result<String> {
    let mut endpoint = url::Url::parse(base_endpoint.trim())?;
    let base_path = endpoint.path().trim_end_matches('/');
    endpoint.set_path(&format!("{base_path}/{signal_path}"));
    endpoint.set_query(None);
    endpoint.set_fragment(None);
    Ok(endpoint.into())
}

#[cfg(test)]
mod tests {
    use super::otlp_signal_endpoint;

    #[test]
    fn derives_otlp_signal_endpoints_from_base_url_paths() {
        let cases = [
            (
                "http://collector/prefix",
                "v1/traces",
                "http://collector/prefix/v1/traces",
            ),
            (
                "http://collector/prefix/",
                "v1/logs",
                "http://collector/prefix/v1/logs",
            ),
            (
                " http://collector/prefix?tenant=a#ignored ",
                "v1/traces",
                "http://collector/prefix/v1/traces",
            ),
            (
                "http://collector/?tenant=a",
                "v1/logs",
                "http://collector/v1/logs",
            ),
        ];

        for (base, signal_path, expected) in cases {
            assert_eq!(
                otlp_signal_endpoint(base, signal_path).expect("derive endpoint"),
                expected
            );
        }
    }
}
