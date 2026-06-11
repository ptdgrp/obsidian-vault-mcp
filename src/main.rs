mod parser;
mod query;
mod resolver;
mod server;
mod vault;

use std::time::Duration;

use camino::Utf8PathBuf;
use clap::Parser;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{Resource, logs::SdkLoggerProvider, trace::SdkTracerProvider};
use tracing_subscriber::{
    EnvFilter, Layer as _, Registry, layer::SubscriberExt, util::SubscriberInitExt,
};

use crate::server::run_mcp_server;
use crate::vault::{Vault, VaultConfig};

#[derive(clap::Parser)]
struct Cli {
    /// Vault root directory
    #[arg(long, env = "OBSIDIAN_VAULT_MCP_ROOT")]
    vault: Utf8PathBuf,

    /// Optional config file
    #[arg(long, env = "OBSIDIAN_VAULT_MCP_CONFIG")]
    config: Option<Utf8PathBuf>,

    /// Include glob patterns
    #[arg(long)]
    include: Vec<String>,

    /// Exclude glob patterns
    #[arg(long)]
    exclude: Vec<String>,

    /// Follow symlinks inside vault
    #[arg(long, default_value_t = false)]
    follow_symlinks: bool,

    /// Max bytes returned by a single tool call
    #[arg(long, default_value_t = 262_144)]
    max_output_bytes: usize,

    /// Max search results
    #[arg(long, default_value_t = 50)]
    max_results: usize,

    /// Seconds before an unused parsed Markdown cache entry expires
    #[arg(long, default_value_t = 600)]
    parse_cache_ttl_secs: u64,

    /// Max parsed Markdown cache entries kept in memory
    #[arg(long, default_value_t = 1024)]
    parse_cache_max_entries: usize,

    /// Log level, written to stderr
    #[arg(long, default_value = "warn")]
    log_level: String,

    /// Optional OTLP HTTP endpoint. When absent, telemetry stays on stderr only.
    #[arg(long, env = "OTEL_EXPORTER_OTLP_ENDPOINT")]
    otel_endpoint: Option<String>,

    /// OpenTelemetry service name
    #[arg(long, env = "OTEL_SERVICE_NAME", default_value = "obsidian-vault-mcp")]
    otel_service_name: String,

    /// OTEL logs filter. Defaults to warnings/errors; traces still capture tool spans.
    #[arg(
        long,
        env = "OBSIDIAN_VAULT_MCP_OTEL_LOG_LEVEL",
        default_value = "warn"
    )]
    otel_log_level: String,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Run MCP server over stdio
    Serve,

    /// Check vault config and list readable notes
    Doctor,

    /// List markdown notes in vault
    #[command(name = "list_notes")]
    ListNotes,

    /// Read one Markdown note body
    #[command(name = "read_note")]
    ReadNote { note: String },

    /// Parse one note and print extracted Obsidian structures
    #[command(name = "parse_note")]
    ParseNote { note: String },

    /// Print one note's heading tree
    #[command(name = "get_note_outline")]
    GetNoteOutline { note: String },

    /// Print a gitignore-aware flat list of visible vault files
    #[command(name = "list_vault_files")]
    ListVaultFiles {
        #[arg(long = "include_files", default_value_t = true)]
        include_files: bool,

        #[arg(long = "include_attachments", default_value_t = false)]
        include_attachments: bool,

        #[arg(long = "include_readme_outline", default_value_t = false)]
        include_readme_outline: bool,

        #[arg(long = "max_files", default_value_t = 100)]
        max_files: usize,
    },

    /// Resolve an Obsidian reference, e.g. [[Note#Heading]]
    #[command(name = "resolve_ref")]
    ResolveRef { reference: String },

    /// Get outgoing local links from one note
    #[command(name = "get_outlinks")]
    GetOutlinks {
        note: String,

        #[arg(long, default_value_t = false)]
        verbose: bool,
    },

    /// Find backlinks to a note or reference
    #[command(name = "get_backlinks")]
    GetBacklinks {
        target: String,

        #[arg(long, default_value_t = false)]
        verbose: bool,
    },

    /// List body and frontmatter tags, optionally filtered by exact tag
    #[command(name = "get_tags")]
    GetTags {
        #[arg(long)]
        tag: Option<String>,

        #[arg(long, default_value_t = false)]
        verbose: bool,
    },

    /// Query notes by a top-level frontmatter field
    #[command(name = "query_frontmatter")]
    QueryFrontmatter {
        field: String,

        #[arg(long, default_value = "exists")]
        mode: String,

        #[arg(long)]
        value: Option<String>,
    },

    /// Literal search
    #[command(name = "search_text")]
    SearchText {
        query: String,

        #[arg(long = "case_sensitive")]
        case_sensitive: bool,

        #[arg(long = "context_lines", default_value_t = 0)]
        context_lines: usize,
    },

    /// Regex search
    #[command(name = "search_regex")]
    SearchRegex {
        pattern: String,

        #[arg(long = "case_sensitive")]
        case_sensitive: bool,

        #[arg(long = "context_lines", default_value_t = 0)]
        context_lines: usize,

        #[arg(long = "path_glob")]
        path_glob: Option<String>,
    },

    /// Collect bounded context around one note
    #[command(name = "collect_note_context")]
    CollectNoteContext { note: String },

    /// Resolve a reference, then collect bounded context
    #[command(name = "collect_reference_context")]
    CollectReferenceContext { reference: String },

    /// Read a heading, block id, or line range from a note
    #[command(name = "read_section")]
    ReadSection {
        note: String,

        #[arg(long)]
        heading: Option<String>,

        #[arg(long = "block_id")]
        block_id: Option<String>,

        #[arg(long = "line_start")]
        line_start: Option<u64>,

        #[arg(long = "line_end")]
        line_end: Option<u64>,
    },

    /// Find local links that do not resolve to any note
    #[command(name = "find_unresolved_links")]
    FindUnresolvedLinks,

    /// Find local links that resolve to multiple candidate notes
    #[command(name = "find_ambiguous_links")]
    FindAmbiguousLinks,

    /// Build the full local-link note graph for audit or visualization
    #[command(name = "get_note_graph")]
    GetNoteGraph,

    /// Build a bounded local-link graph neighborhood for normal context use
    #[command(name = "get_graph_neighborhood")]
    GetGraphNeighborhood {
        target: String,

        #[arg(long, default_value_t = 1)]
        depth: usize,

        #[arg(long, default_value = "both")]
        direction: String,

        #[arg(long = "include_unresolved", default_value_t = false)]
        include_unresolved: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let mut telemetry = init_tracing(
        &cli.log_level,
        cli.otel_endpoint.as_deref(),
        &cli.otel_service_name,
        &cli.otel_log_level,
    )?;
    let vault_path = cli.vault.clone();
    let config = vault_config(&cli);
    let command = cli.command.unwrap_or(Command::Serve);
    let result = async {
        let vault = Vault::open(vault_path, config)?;
        let queries = crate::query::VaultQueries::new(vault.clone());

        match command {
            Command::Serve => run_mcp_server(vault).await?,
        Command::Doctor | Command::ListNotes => {
            print_value(&queries.list_notes()?)?;
        }
        Command::ReadNote { note } => {
            print_value(&queries.read_note(&note)?)?;
        }
        Command::ParseNote { note } => {
            print_value(&queries.parse_note_result(&note)?)?;
        }
        Command::GetNoteOutline { note } => {
            print_value(&queries.get_note_outline(&note)?)?;
        }
        Command::ListVaultFiles {
            include_files,
            include_attachments,
            include_readme_outline,
            max_files,
        } => {
            let result = queries.list_vault_files(crate::query::VaultFilesOptions {
                include_files,
                include_attachments,
                include_readme_outline,
                max_files,
            })?;
            print_value(&result)?;
        }
        Command::ResolveRef { reference } => {
            print_value(&queries.resolve_ref(&reference)?)?;
        }
        Command::GetOutlinks { note, verbose } => {
            print_value(&queries.get_outlinks_output(&note, verbose)?)?;
        }
        Command::GetBacklinks { target, verbose } => {
            print_value(&queries.get_backlinks_output(&target, verbose)?)?;
        }
        Command::GetTags { tag, verbose } => {
            print_value(&queries.get_tags_output(tag.as_deref(), verbose)?)?;
        }
        Command::QueryFrontmatter {
            field,
            mode,
            value,
        } => {
            print_value(
                &queries.query_frontmatter(crate::query::FrontmatterQueryOptions {
                    field,
                    mode: parse_frontmatter_match_mode(&mode)?,
                    value,
                })?,
            )?;
        }
        Command::SearchText {
            query,
            case_sensitive,
            context_lines,
        } => {
            print_value(&queries.search_text(&query, case_sensitive, context_lines)?)?;
        }
        Command::SearchRegex {
            pattern,
            case_sensitive,
            context_lines,
            path_glob,
        } => {
            print_value(
                &queries.search_regex(
                    &pattern,
                    case_sensitive,
                    context_lines,
                    path_glob.as_deref(),
                )?,
            )?;
        }
        Command::CollectNoteContext { note } => {
            print_value(&queries.collect_note_context(&note)?)?;
        }
        Command::CollectReferenceContext { reference } => {
            print_value(&queries.collect_reference_context(&reference)?)?;
        }
        Command::ReadSection {
            note,
            heading,
            block_id,
            line_start,
            line_end,
        } => {
            let selector = match (heading, block_id, line_start, line_end) {
                (Some(heading), None, None, None) => {
                    crate::query::SectionSelector::Heading { heading }
                }
                (None, Some(block_id), None, None) => {
                    crate::query::SectionSelector::Block { block_id }
                }
                (None, None, Some(line_start), Some(line_end)) => {
                    crate::query::SectionSelector::Lines {
                        line_start,
                        line_end,
                    }
                }
                _ => {
                    return Err(anyhow::anyhow!(
                        "provide exactly one selector: --heading, --block-id, or --line-start with --line-end"
                    ));
                }
            };
            print_value(&queries.read_section(&note, selector)?)?;
        }
        Command::FindUnresolvedLinks => {
            print_value(&queries.find_unresolved_links()?)?;
        }
        Command::FindAmbiguousLinks => {
            print_value(&queries.find_ambiguous_links()?)?;
        }
        Command::GetNoteGraph => {
            print_value(&queries.get_note_graph()?)?;
        }
        Command::GetGraphNeighborhood {
            target,
            depth,
            direction,
            include_unresolved,
        } => {
            print_value(
                &queries.get_graph_neighborhood(crate::query::GraphNeighborhoodOptions {
                    target,
                    depth,
                    direction: parse_graph_neighborhood_direction(&direction)?,
                    include_unresolved,
                })?,
            )?;
        }
    }
    anyhow::Ok(())
    }
    .await;
    telemetry.shutdown();
    result
}

fn parse_graph_neighborhood_direction(
    input: &str,
) -> anyhow::Result<crate::query::GraphNeighborhoodDirection> {
    match input {
        "out" => Ok(crate::query::GraphNeighborhoodDirection::Out),
        "in" => Ok(crate::query::GraphNeighborhoodDirection::In),
        "both" => Ok(crate::query::GraphNeighborhoodDirection::Both),
        _ => Err(anyhow::anyhow!(
            "invalid graph neighborhood direction: {input}; expected one of: both, out, in"
        )),
    }
}

fn parse_frontmatter_match_mode(input: &str) -> anyhow::Result<crate::query::FrontmatterMatchMode> {
    match input {
        "exists" => Ok(crate::query::FrontmatterMatchMode::Exists),
        "equals" => Ok(crate::query::FrontmatterMatchMode::Equals),
        "regex" => Ok(crate::query::FrontmatterMatchMode::Regex),
        _ => Err(anyhow::anyhow!(
            "invalid frontmatter query mode: {input}; expected one of: exists, equals, regex"
        )),
    }
}

fn vault_config(cli: &Cli) -> VaultConfig {
    let mut config = VaultConfig::default();
    config.include = cli.include.clone();
    config.exclude = cli.exclude.clone();
    config.follow_symlinks = cli.follow_symlinks;
    config.max_output_bytes = cli.max_output_bytes;
    config.max_results = cli.max_results;
    config.parse_cache_ttl_secs = cli.parse_cache_ttl_secs;
    config.parse_cache_max_entries = cli.parse_cache_max_entries;
    config
}

fn print_value<T: serde::Serialize>(value: &T) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

struct TelemetryGuard {
    tracer_provider: Option<SdkTracerProvider>,
    logger_provider: Option<SdkLoggerProvider>,
}

impl TelemetryGuard {
    fn shutdown(&mut self) {
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

fn init_tracing(
    level: &str,
    otel_endpoint: Option<&str>,
    otel_service_name: &str,
    otel_log_level: &str,
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

    let resource = Resource::builder()
        .with_service_name(otel_service_name.to_string())
        .build();

    let span_exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_protocol(opentelemetry_otlp::Protocol::HttpBinary)
        .with_endpoint(endpoint)
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
        .with_endpoint(endpoint)
        .build()?;
    let logger_provider = SdkLoggerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(log_exporter)
        .build();
    let otel_log_filter = telemetry_filter(otel_log_level)?;
    let otel_log_layer =
        opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(&logger_provider)
            .with_filter(otel_log_filter);

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
