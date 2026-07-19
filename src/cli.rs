pub(crate) mod commands;

use crate::vault::DEFAULT_MAX_READ_NOTE_CHARS;
use camino::Utf8PathBuf;

#[derive(Debug, clap::Parser)]
#[command(version, about, long_about = None)]
pub(crate) struct Cli {
    /// Vault root directory
    #[arg(long, env = "OBSIDIAN_VAULT_MCP_ROOT")]
    pub(crate) vault: Option<Utf8PathBuf>,

    /// Optional config file
    #[arg(long, env = "OBSIDIAN_VAULT_MCP_CONFIG")]
    pub(crate) config: Option<Utf8PathBuf>,

    /// Include glob patterns
    #[arg(long)]
    pub(crate) include: Vec<String>,

    /// Exclude glob patterns
    #[arg(long)]
    pub(crate) exclude: Vec<String>,

    /// Follow symlinks inside vault
    #[arg(long, default_value_t = false)]
    pub(crate) follow_symlinks: bool,

    /// Max Unicode characters returned by read_note
    #[arg(long, default_value_t = DEFAULT_MAX_READ_NOTE_CHARS)]
    pub(crate) max_read_note_chars: usize,

    /// Max search results
    #[arg(long, default_value_t = 50)]
    pub(crate) max_results: usize,

    /// Seconds before an unused parsed Markdown cache entry expires
    #[arg(long, default_value_t = 600)]
    pub(crate) parse_cache_ttl_secs: u64,

    /// Max parsed Markdown cache entries kept in memory
    #[arg(long, default_value_t = 1024)]
    pub(crate) parse_cache_max_entries: usize,

    /// Log level for stderr, OpenTelemetry traces, and OpenTelemetry logs.
    #[arg(long, default_value = "debug")]
    pub(crate) log_level: String,

    /// Optional OTLP HTTP endpoint. When absent, telemetry stays on stderr only.
    #[arg(long, env = "OTEL_EXPORTER_OTLP_ENDPOINT")]
    pub(crate) otel_endpoint: Option<String>,

    /// OpenTelemetry service name
    #[arg(long, env = "OTEL_SERVICE_NAME", default_value = "obsidian-vault-mcp")]
    pub(crate) otel_service_name: String,

    #[command(subcommand)]
    command: Option<commands::Command>,
}

impl Cli {
    pub(crate) fn command_ref(&self) -> Option<&commands::Command> {
        self.command.as_ref()
    }

    pub(crate) fn command(self) -> commands::Command {
        self.command.unwrap_or(commands::Command::Serve)
    }
}
