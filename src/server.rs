use std::{sync::Arc, time::Instant};

use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{
        router::tool::ToolRouter,
        wrapper::{Json, Parameters},
    },
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    query::{
        AmbiguousLinksResult, BacklinksResult, ContextResult, FrontmatterQueryOptions,
        FrontmatterQueryResult, GraphNeighborhoodOptions, ListNotesResult, NoteGraphResult,
        NoteOutlineResult, OutlinksResult, ReadNoteResult, ReadSectionResult, SearchRegexResult,
        SearchTextResult, SectionSelector, TagsResult, UnresolvedLinksResult, VaultFilesOptions,
        VaultFilesResult, VaultQueries,
    },
    resolver::ResolveResult,
    vault::Vault,
};

pub async fn run_mcp_server(vault: Vault) -> anyhow::Result<()> {
    let service = ObsidianVaultMcp::new(vault);
    let server = service
        .serve((tokio::io::stdin(), tokio::io::stdout()))
        .await?;
    server.waiting().await?;
    Ok(())
}

#[derive(Clone)]
pub struct ObsidianVaultMcp {
    state: Arc<AppState>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

#[derive(Clone)]
pub struct AppState {
    pub queries: VaultQueries,
}

impl ObsidianVaultMcp {
    pub fn new(vault: Vault) -> Self {
        Self {
            state: Arc::new(AppState {
                queries: VaultQueries::new(vault),
            }),
            tool_router: Self::tool_router(),
        }
    }

    fn queries(&self) -> VaultQueries {
        self.state.queries.clone()
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Empty input for tools that operate on the whole vault.
pub struct EmptyRequest {}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for reading one Markdown note.
pub struct ReadNoteRequest {
    /// Vault-relative path, note stem, or alias.
    pub note: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for parsing one Markdown note.
pub struct ParseNoteRequest {
    /// Vault-relative path, note stem, or alias.
    pub note: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for resolving an Obsidian reference.
pub struct ResolveRefRequest {
    /// Reference such as "Note", "[[Note]]", "[[Note#Heading]]", or "[[Note#^block]]".
    pub reference: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for backlink lookup.
pub struct BacklinksRequest {
    /// Note path, stem, alias, or Obsidian reference to find inbound links for.
    pub target: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for outgoing local link lookup.
pub struct OutlinksRequest {
    /// Vault-relative path, note stem, or alias.
    pub note: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for listing tags.
pub struct TagsRequest {
    /// Optional exact tag filter. Both "状态/身体" and "#状态/身体" are accepted.
    pub tag: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for literal text search.
pub struct SearchTextRequest {
    /// Literal text to search for.
    pub query: String,
    /// Whether matching should be case-sensitive.
    #[serde(default)]
    pub case_sensitive: bool,
    /// Number of surrounding lines to include in each snippet.
    #[serde(default = "default_context_lines")]
    pub context_lines: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for regular expression search.
pub struct SearchRegexRequest {
    /// Rust regex pattern matched line by line.
    pub pattern: String,
    /// Whether matching should be case-sensitive.
    #[serde(default)]
    pub case_sensitive: bool,
    /// Number of surrounding lines to include in each snippet.
    #[serde(default = "default_context_lines")]
    pub context_lines: usize,
    /// Optional glob limiting searched note paths, such as "正文/**/*.md".
    pub path_glob: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for collecting context around one note.
pub struct ContextNoteRequest {
    /// Vault-relative path, note stem, or alias.
    pub note: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for collecting context around a reference.
pub struct ContextReferenceRequest {
    /// Reference such as "Note", "[[Note]]", or "[[Note#Heading]]".
    pub reference: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for reading a section from one note.
pub struct ReadSectionRequest {
    /// Vault-relative path, note stem, or alias.
    pub note: String,
    /// Exactly one selector: heading, block id, or line range.
    #[serde(flatten)]
    pub selector: SectionSelector,
}

pub type VaultFilesRequest = VaultFilesOptions;
pub type GraphNeighborhoodRequest = GraphNeighborhoodOptions;
pub type FrontmatterQueryRequest = FrontmatterQueryOptions;

#[derive(Debug, serde::Serialize, JsonSchema)]
pub struct ResolveRefToolResult {
    pub result: ResolveResult,
}

fn default_context_lines() -> usize {
    0
}

#[tool_router]
impl ObsidianVaultMcp {
    #[tool(description = "List visible Markdown notes with path and first heading title")]
    fn list_notes(&self, _: Parameters<EmptyRequest>) -> Result<Json<ListNotesResult>, String> {
        run_tool("list_notes", || self.queries().list_notes())
    }

    #[tool(
        description = "Read one Markdown note body by path, stem, or alias; use when exact source text is needed"
    )]
    fn read_note(
        &self,
        Parameters(ReadNoteRequest { note }): Parameters<ReadNoteRequest>,
    ) -> Result<Json<ReadNoteResult>, String> {
        run_tool("read_note", || self.queries().read_note(&note))
    }

    #[tool(
        description = "Parse one Markdown note into headings, local links, embeds, tags, block ids, frontmatter, and source spans"
    )]
    fn parse_note(
        &self,
        Parameters(ParseNoteRequest { note }): Parameters<ParseNoteRequest>,
    ) -> Result<Json<crate::parser::ParsedNote>, String> {
        run_tool("parse_note", || self.queries().parse_note(&note))
    }

    #[tool(
        description = "Return one note's heading tree without body text; use before selecting a section"
    )]
    fn get_note_outline(
        &self,
        Parameters(ParseNoteRequest { note }): Parameters<ParseNoteRequest>,
    ) -> Result<Json<NoteOutlineResult>, String> {
        run_tool("get_note_outline", || {
            self.queries().get_note_outline(&note)
        })
    }

    #[tool(description = "List gitignore-aware visible vault files as flat entries with paths")]
    fn list_vault_files(
        &self,
        Parameters(request): Parameters<VaultFilesRequest>,
    ) -> Result<Json<VaultFilesResult>, String> {
        run_tool("list_vault_files", || {
            self.queries().list_vault_files(request)
        })
    }

    #[tool(
        description = "Search literal text across visible Markdown notes and return section-aware snippets"
    )]
    fn search_text(
        &self,
        Parameters(SearchTextRequest {
            query,
            case_sensitive,
            context_lines,
        }): Parameters<SearchTextRequest>,
    ) -> Result<Json<SearchTextResult>, String> {
        run_tool("search_text", || {
            self.queries()
                .search_text(&query, case_sensitive, context_lines)
        })
    }

    #[tool(
        description = "Search visible Markdown notes with a Rust regular expression and return section-aware snippets"
    )]
    fn search_regex(
        &self,
        Parameters(SearchRegexRequest {
            pattern,
            case_sensitive,
            context_lines,
            path_glob,
        }): Parameters<SearchRegexRequest>,
    ) -> Result<Json<SearchRegexResult>, String> {
        run_tool("search_regex", || {
            self.queries().search_regex(
                &pattern,
                case_sensitive,
                context_lines,
                path_glob.as_deref(),
            )
        })
    }

    #[tool(
        description = "Resolve an Obsidian reference such as [[Note#Heading]] to a note, heading, or block without guessing ambiguous targets"
    )]
    fn resolve_ref(
        &self,
        Parameters(ResolveRefRequest { reference }): Parameters<ResolveRefRequest>,
    ) -> Result<Json<ResolveRefToolResult>, String> {
        run_tool("resolve_ref", || {
            self.queries()
                .resolve_ref(&reference)
                .map(|result| ResolveRefToolResult { result })
        })
    }

    #[tool(
        description = "Get outgoing local links from one note with resolved target status and source snippets"
    )]
    fn get_outlinks(
        &self,
        Parameters(OutlinksRequest { note }): Parameters<OutlinksRequest>,
    ) -> Result<Json<OutlinksResult>, String> {
        run_tool("get_outlinks", || self.queries().get_outlinks(&note))
    }

    #[tool(
        description = "Get backlinks to a note or Obsidian reference with resolved target status and source snippets"
    )]
    fn get_backlinks(
        &self,
        Parameters(BacklinksRequest { target }): Parameters<BacklinksRequest>,
    ) -> Result<Json<BacklinksResult>, String> {
        run_tool("get_backlinks", || self.queries().get_backlinks(&target))
    }

    #[tool(
        description = "List tags across body tag nodes and frontmatter tags, or list notes under one exact tag"
    )]
    fn get_tags(
        &self,
        Parameters(TagsRequest { tag }): Parameters<TagsRequest>,
    ) -> Result<Json<TagsResult>, String> {
        run_tool("get_tags", || self.queries().get_tags(tag.as_deref()))
    }

    #[tool(
        description = "Query notes by a top-level frontmatter field using explicit exists, equals, or regex mode"
    )]
    fn query_frontmatter(
        &self,
        Parameters(request): Parameters<FrontmatterQueryRequest>,
    ) -> Result<Json<FrontmatterQueryResult>, String> {
        run_tool("query_frontmatter", || {
            self.queries().query_frontmatter(request)
        })
    }

    #[tool(
        description = "Collect bounded context grouped as current note, outlinks, and backlinks"
    )]
    fn collect_note_context(
        &self,
        Parameters(ContextNoteRequest { note }): Parameters<ContextNoteRequest>,
    ) -> Result<Json<ContextResult>, String> {
        run_tool("collect_note_context", || {
            self.queries().collect_note_context(&note)
        })
    }

    #[tool(
        description = "Resolve a reference, then collect bounded context grouped as current note, outlinks, and backlinks"
    )]
    fn collect_reference_context(
        &self,
        Parameters(ContextReferenceRequest { reference }): Parameters<ContextReferenceRequest>,
    ) -> Result<Json<ContextResult>, String> {
        run_tool("collect_reference_context", || {
            self.queries().collect_reference_context(&reference)
        })
    }

    #[tool(
        description = "Read exactly one heading section, block id, or line range from a note with source span"
    )]
    fn read_section(
        &self,
        Parameters(ReadSectionRequest { note, selector }): Parameters<ReadSectionRequest>,
    ) -> Result<Json<ReadSectionResult>, String> {
        run_tool("read_section", || {
            self.queries().read_section(&note, selector)
        })
    }

    #[tool(
        description = "Find local links that do not resolve to any visible note; use as a vault health check"
    )]
    fn find_unresolved_links(
        &self,
        _: Parameters<EmptyRequest>,
    ) -> Result<Json<UnresolvedLinksResult>, String> {
        run_tool("find_unresolved_links", || {
            self.queries().find_unresolved_links()
        })
    }

    #[tool(
        description = "Find local links that resolve to multiple visible notes; use before relying on link graph context"
    )]
    fn find_ambiguous_links(
        &self,
        _: Parameters<EmptyRequest>,
    ) -> Result<Json<AmbiguousLinksResult>, String> {
        run_tool("find_ambiguous_links", || {
            self.queries().find_ambiguous_links()
        })
    }

    #[tool(
        description = "Build the full visible-note local-link graph for audit, visualization, or debugging; prefer get_graph_neighborhood for normal agent context"
    )]
    fn get_note_graph(&self, _: Parameters<EmptyRequest>) -> Result<Json<NoteGraphResult>, String> {
        run_tool("get_note_graph", || self.queries().get_note_graph())
    }

    #[tool(
        description = "Return a bounded local-link graph neighborhood around one note or reference; preferred graph tool for normal agent context"
    )]
    fn get_graph_neighborhood(
        &self,
        Parameters(request): Parameters<GraphNeighborhoodRequest>,
    ) -> Result<Json<NoteGraphResult>, String> {
        run_tool("get_graph_neighborhood", || {
            self.queries().get_graph_neighborhood(request)
        })
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ObsidianVaultMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Read-only Obsidian vault structure tools for LLM agents.")
    }
}

fn run_tool<T>(
    tool_name: &'static str,
    run: impl FnOnce() -> anyhow::Result<T>,
) -> Result<Json<T>, String> {
    let span = tracing::info_span!("mcp.tool", tool.name = tool_name);
    let _enter = span.enter();
    let started = Instant::now();

    tracing::info!("tool.call.start");
    match run() {
        Ok(result) => {
            tracing::info!(
                duration_ms = started.elapsed().as_millis() as u64,
                "tool.call.ok"
            );
            Ok(Json(result))
        }
        Err(error) => {
            let message = error.to_string();
            tracing::error!(
                duration_ms = started.elapsed().as_millis() as u64,
                error = %message,
                "tool.call.error"
            );
            Err(message)
        }
    }
}
