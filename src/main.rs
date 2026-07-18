mod blueprint;
mod cli;
mod docs;
mod mutation;
mod parser;
mod query;
mod resolver;
mod server;
mod telemetry;
mod vault;

use clap::Parser;
use opentelemetry::trace::Status;
use std::time::Instant;
use tracing::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;

use crate::vault::{Vault, VaultConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();
    let vault_path = cli.vault.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "--vault is required for this command; set OBSIDIAN_VAULT_MCP_ROOT or pass --vault"
        )
    })?;
    let mut telemetry = telemetry::init_tracing(
        &cli.log_level,
        cli.otel_endpoint.as_deref(),
        &cli.otel_service_name,
    )?;
    tracing::debug!(
        otel.enabled = telemetry.is_enabled(),
        "telemetry.initialized"
    );
    let started = Instant::now();
    let (input_preview, input_truncated) = telemetry::telemetry_preview(&cli);
    let config = VaultConfig::build(&cli);
    let vault = Vault::open(vault_path, config)?;
    let command = cli.command();
    let command_name = command.name();
    let command_span = tracing::info_span!(
        "cli.command",
        command = command_name,
        input.preview = %input_preview,
        input.truncated = input_truncated,
    );
    tracing::info!(parent: &command_span, command = command_name, input.preview = %input_preview, input.truncated = input_truncated, "cli.command.start");
    let result = command.run(vault).instrument(command_span.clone()).await;
    let duration_ms = started.elapsed().as_millis() as u64;
    match &result {
        Ok(()) => {
            tracing::info!(parent: &command_span, command = command_name, duration_ms, "cli.command.ok")
        }
        Err(error) => {
            command_span.set_attribute("error.type", "command.error");
            command_span.set_status(Status::error("command failed"));
            let error = format_error_chain(error);
            tracing::error!(
                parent: &command_span,
                command = command_name,
                duration_ms,
                error = %error,
                "cli.command.error"
            )
        }
    }
    drop(command_span);
    telemetry.shutdown();
    result
}

pub(crate) fn format_error_chain(error: &anyhow::Error) -> String {
    format!("{error:#}")
}

#[cfg(test)]
mod tests {
    use super::format_error_chain;

    #[test]
    fn formats_complete_error_chain() {
        let error = anyhow::anyhow!("inner cause").context("outer context");
        assert_eq!(format_error_chain(&error), "outer context: inner cause");
    }
}
