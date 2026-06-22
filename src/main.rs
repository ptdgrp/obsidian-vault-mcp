mod byte_size;
mod docs;
mod mutation;
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

use crate::server::{run_mcp_server, section_parts};
use crate::vault::{DEFAULT_MAX_READ_NOTE_BYTES, Vault, VaultConfig};
use byte_size::parse_byte_size;

#[derive(clap::Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Vault root directory
    #[arg(long, env = "OBSIDIAN_VAULT_MCP_ROOT")]
    vault: Option<Utf8PathBuf>,

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

    /// Max bytes returned by read_note; accepts b, k, and m suffixes with decimals
    #[arg(
        long,
        default_value_t = DEFAULT_MAX_READ_NOTE_BYTES,
        value_parser = parse_byte_size
    )]
    max_read_note_bytes: usize,

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

    /// Generate docs from the MCP tool schemas
    GenerateDocs {
        /// Check whether docs/tools.md is up to date without writing it
        #[arg(long, default_value_t = false)]
        check: bool,

        /// Output markdown file
        #[arg(long, default_value = "docs/tools.md")]
        output: Utf8PathBuf,
    },

    /// Check vault config and list readable notes
    Doctor,

    /// List markdown notes in vault
    ListNotes,

    /// Read one Markdown note body
    ReadNote { note: String },

    /// Parse one note and print extracted Obsidian structures
    ParseNote { note: String },

    /// Print one note's heading tree
    GetNoteOutline { note: String },

    /// Return one note's word count, character count, and total backlink count
    GetNoteStats { note: String },

    /// Print a gitignore-aware flat list of visible vault files
    ListVaultFiles {
        #[arg(long, default_value_t = true)]
        include_files: bool,

        #[arg(long, default_value_t = false)]
        include_attachments: bool,

        #[arg(long, default_value_t = false)]
        include_readme_outline: bool,

        #[arg(long, default_value_t = 100)]
        max_files: usize,
    },

    /// Resolve an Obsidian reference, e.g. [[Note#Heading]]
    ResolveRef { reference: String },

    /// Get outgoing local links from one note
    GetOutlinks {
        note: String,

        #[arg(long, default_value_t = false)]
        verbose: bool,
    },

    /// Find backlinks to a note or reference
    GetBacklinks {
        target: String,

        #[arg(long, default_value_t = false)]
        verbose: bool,
    },

    /// List unique body and frontmatter tag names
    ListTags {
        #[arg(long, default_value = "note")]
        scope: String,
    },

    /// Locate selected body or frontmatter tags
    GetTags {
        tags: Vec<String>,

        #[arg(long, default_value = "note")]
        scope: String,

        #[arg(long, default_value_t = false)]
        verbose: bool,
    },

    /// List unique folder-derived category names
    ListCategories,

    /// Locate selected folder-derived categories
    GetCategories { categories: Vec<String> },

    /// Query notes by a top-level frontmatter field
    QueryFrontmatter {
        field: String,

        #[arg(long, default_value = "exists")]
        mode: String,

        #[arg(long)]
        value: Option<String>,
    },

    /// Literal search
    SearchText {
        query: String,

        #[arg(long)]
        case_sensitive: bool,

        #[arg(long, default_value_t = 0)]
        context_lines: usize,
    },

    /// Regex search
    SearchRegex {
        pattern: String,

        #[arg(long)]
        case_sensitive: bool,

        #[arg(long, default_value_t = 0)]
        context_lines: usize,

        #[arg(long)]
        path_glob: Option<String>,
    },

    /// Collect bounded context around one note
    CollectNoteContext { note: String },

    /// Resolve a reference, then collect bounded context
    CollectReferenceContext { reference: String },

    /// Read a heading, block id, or line reference from a note
    ReadSection {
        note: String,

        #[arg(long)]
        heading: Option<String>,

        #[arg(long)]
        block_id: Option<String>,

        #[arg(long)]
        line: Option<String>,
    },

    /// Find local links that do not resolve to any note
    FindUnresolvedLinks,

    /// Find local links that resolve to multiple candidate notes
    FindAmbiguousLinks,

    /// Build the full local-link vault graph for audit or visualization
    GetVaultGraph,

    /// Build a bounded local-link graph neighborhood for normal context use
    GetGraphNeighborhood {
        target: String,

        #[arg(long, default_value_t = 1)]
        depth: usize,

        #[arg(long, default_value = "both")]
        direction: String,

        #[arg(long, default_value_t = false)]
        include_unresolved: bool,
    },
    /// Append content at the end of exactly one heading, block, or line section. This uses structural selection, not text matching.
    AppendSection {
        /// Vault-relative path, note stem, or alias.
        note: String,

        /// Heading text, heading anchor, or slash-separated heading path.
        #[arg(long)]
        heading: Option<String>,

        /// Block id without the leading caret.
        #[arg(long)]
        block_id: Option<String>,

        /// Github-style line reference, e.g. #L1-L99.
        #[arg(long)]
        line: Option<String>,

        /// Replacement text for the relative line range.
        content: String,
    },
    /// Replace exactly one heading, block, or line section with new content. This uses structural selection, not text matching.
    ReplaceSection {
        /// Vault-relative path, note stem, or alias.
        note: String,

        /// Heading text, heading anchor, or slash-separated heading path.
        #[arg(long)]
        heading: Option<String>,

        /// Block id without the leading caret.
        #[arg(long)]
        block_id: Option<String>,

        /// Github-style line reference, e.g. #L1-L99.
        #[arg(long)]
        line: Option<String>,

        /// Replacement text for the relative line range.
        content: String,
    },
    /// Delete exactly one heading, block, or line section. This uses structural selection, not text matching.
    DeleteSection {
        /// Vault-relative path, note stem, or alias.
        note: String,

        /// Heading text, heading anchor, or slash-separated heading path.
        #[arg(long)]
        heading: Option<String>,

        /// Block id without the leading caret.
        #[arg(long)]
        block_id: Option<String>,

        /// Github-style line reference, e.g. #L1-L99.
        #[arg(long)]
        line: Option<String>,
    },
    /// Rename one heading and update uniquely resolved Obsidian wikilinks to it. Set dry_run to false to apply; preview is the default.
    RenameHeading {
        /// Vault-relative path, note stem, or alias.
        note: String,
        /// Current heading text or anchor.
        #[arg(long)]
        old_heading: String,
        /// Replacement heading text.
        #[arg(long)]
        new_heading: String,
        /// Preview changed notes and references without writing. Defaults to true.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        dry_run: bool,
    },
    /// Move a note to a new vault-relative path and update uniquely resolved wikilinks. Set dry_run to false to apply.
    RenameNote {
        /// Existing vault-relative path, note stem, or alias.
        note: String,
        /// New vault-relative Markdown path. Parent directories are created when applying.
        new_path: String,
        /// Preview changed notes and references without writing. Defaults to true.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        dry_run: bool,
    },
    /// Rename one block id and update uniquely resolved Obsidian wikilinks. Set dry_run to false to apply.
    RenameBlockId {
        /// Vault-relative path, note stem, or alias.
        note: String,
        /// Existing block id without the leading caret.
        old_block_id: String,
        /// Replacement block id without the leading caret.
        new_block_id: String,
        /// Preview changed notes and references without writing. Defaults to true.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        dry_run: bool,
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
    let config = vault_config(&cli);
    let command = cli.command.unwrap_or(Command::Serve);
    let result = async {
        if let Command::GenerateDocs { check, output } = command {
            let content =
                crate::docs::render_docs(&crate::server::ObsidianVaultMcp::tool_definitions())?;
            if check {
                crate::docs::check_tools_markdown(output, &content)?;
            } else {
                crate::docs::write_tools_markdown(output, &content)?;
            }
            return anyhow::Ok(());
        }

        let vault_path = cli.vault.clone().ok_or_else(|| {
            anyhow::anyhow!(
                "--vault is required for this command; set OBSIDIAN_VAULT_MCP_ROOT or pass --vault"
            )
        })?;
        let vault = Vault::open(vault_path, config)?;
        let queries = crate::query::VaultQueries::new(vault.clone());
        let mutations = crate::mutation::VaultMutations::new(queries.clone());

        match command {
            Command::Serve => run_mcp_server(vault).await?,
            Command::GenerateDocs { .. } => unreachable!("handled before opening vault"),
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
            Command::GetNoteStats { note } => {
                print_value(&queries.get_note_stats(&note)?)?;
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
            Command::ListTags { scope } => {
                print_value(&queries.list_tags(parse_tag_scope(&scope)?)?)?;
            }
            Command::GetTags {
                tags,
                scope,
                verbose,
            } => {
                if tags.is_empty() {
                    return Err(anyhow::anyhow!(
                        "provide at least one tag; use list_tags to discover tag names"
                    ));
                }
                print_value(&queries.get_tags(&tags, parse_tag_scope(&scope)?, verbose)?)?;
            }
            Command::ListCategories => {
                print_value(&queries.list_categories()?)?;
            }
            Command::GetCategories { categories } => {
                if categories.is_empty() {
                    return Err(anyhow::anyhow!(
                        "provide at least one category; use list_categories to discover category names"
                    ));
                }
                print_value(&queries.get_categories(&categories)?)?;
            }
            Command::QueryFrontmatter { field, mode, value } => {
                print_value(&queries.query_frontmatter(
                    crate::query::FrontmatterQueryOptions {
                        field,
                        mode: parse_frontmatter_match_mode(&mode)?,
                        value,
                    },
                )?)?;
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
                print_value(&queries.search_regex(
                    &pattern,
                    case_sensitive,
                    context_lines,
                    path_glob.as_deref(),
                )?)?;
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
                line,
            } => {
                let (note, selector) = section_parts(note, heading, block_id, line).map_err(|_|anyhow::anyhow!("provide exactly one selector: --heading, --block-id, or --line"))?;
                print_value(&queries.read_section(&note, selector)?)?;
            }
            Command::FindUnresolvedLinks => {
                print_value(&queries.find_unresolved_links()?)?;
            }
            Command::FindAmbiguousLinks => {
                print_value(&queries.find_ambiguous_links()?)?;
            }
            Command::GetVaultGraph => {
                print_value(&queries.get_vault_graph()?)?;
            }
            Command::GetGraphNeighborhood {
                target,
                depth,
                direction,
                include_unresolved,
            } => {
                print_value(&queries.get_graph_neighborhood(
                    crate::query::GraphNeighborhoodOptions {
                        target,
                        depth,
                        direction: parse_graph_neighborhood_direction(&direction)?,
                        include_unresolved,
                    },
                )?)?;
            }
            Command::AppendSection { note, heading, block_id, line, content } => {
                let (note, selector) = section_parts(note, heading, block_id, line).map_err(|_|anyhow::anyhow!("provide exactly one selector: --heading, --block-id, or --line"))?;
                print_value(&mutations.append_section(&note, selector, &content)?)?
            }
            Command::ReplaceSection { note, heading, block_id, line, content } => {
                let (note, selector) = section_parts(note, heading, block_id, line).map_err(|_|anyhow::anyhow!("provide exactly one selector: --heading, --block-id, or --line"))?;
                print_value(&mutations.replace_section(&note, selector, &content)?)?
            }
            Command::DeleteSection { note, heading, block_id, line } => {
                let (note, selector) = section_parts(note, heading, block_id, line).map_err(|_|anyhow::anyhow!("provide exactly one selector: --heading, --block-id, or --line"))?;
                print_value(&mutations.delete_section(&note, selector)?)?
            }
            Command::RenameHeading { note, old_heading, new_heading, dry_run } => {
                print_value(&mutations.rename_heading(&note, &old_heading, &new_heading, dry_run)?)?
            }
            Command::RenameNote { note, new_path, dry_run } => {
                print_value(&mutations.rename_note(&note, &new_path, dry_run)?)?
            }
            Command::RenameBlockId { note, old_block_id, new_block_id, dry_run } => {
                print_value(&mutations.rename_block_id(&note, &old_block_id, &new_block_id, dry_run)?)?
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

fn parse_tag_scope(input: &str) -> anyhow::Result<crate::query::TagScope> {
    match input {
        "note" => Ok(crate::query::TagScope::Note),
        "frontmatter" => Ok(crate::query::TagScope::Frontmatter),
        "body" => Ok(crate::query::TagScope::Body),
        "section" => Ok(crate::query::TagScope::Section),
        "line" => Ok(crate::query::TagScope::Line),
        other => Err(anyhow::anyhow!(
            "invalid tag scope '{other}'; expected note, frontmatter, body, section, or line"
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
    VaultConfig {
        include: cli.include.clone(),
        exclude: cli.exclude.clone(),
        follow_symlinks: cli.follow_symlinks,
        max_output_bytes: cli.max_output_bytes,
        max_read_note_bytes: cli.max_read_note_bytes,
        max_results: cli.max_results,
        parse_cache_ttl_secs: cli.parse_cache_ttl_secs,
        parse_cache_max_entries: cli.parse_cache_max_entries,
        ..Default::default()
    }
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
