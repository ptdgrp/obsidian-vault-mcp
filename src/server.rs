use std::{sync::Arc, time::Instant};

use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{
        router::tool::ToolRouter,
        wrapper::{Json, Parameters},
    },
    model::{ServerCapabilities, ServerInfo, Tool},
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    query::{
        AmbiguousLinksResult, BacklinksOutput, ContextResult, FrontmatterQueryOptions,
        FrontmatterQueryResult, GetTagsResult, GraphNeighborhoodOptions, ListNotesResult,
        ListTagsResult, NoteOutlineResult, OutlinksOutput, ParseNoteResult, ReadNoteResult,
        ReadSectionResult, SearchRegexResult, SearchTextResult, SectionSelector, TagScope,
        UnresolvedLinksResult, VaultFilesOptions, VaultFilesResult, VaultGraphResult, VaultQueries,
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

    pub fn tool_definitions() -> Vec<Tool> {
        Self::tool_router().list_all()
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
    /// Return detailed source spans and snippets when true. Defaults to compact output.
    #[serde(default)]
    pub verbose: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for outgoing local link lookup.
pub struct OutlinksRequest {
    /// Vault-relative path, note stem, or alias.
    pub note: String,
    /// Return detailed source spans and snippets when true. Defaults to compact output.
    #[serde(default)]
    pub verbose: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for listing tags.
pub struct ListTagsRequest {
    /// Scope to search: note, frontmatter, body, section, or line. Defaults to note.
    #[serde(default)]
    pub scope: TagScope,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for locating tags.
pub struct TagsRequest {
    /// Exact tags to locate. Both "状态/身体" and "#状态/身体" are accepted.
    pub tags: Vec<String>,
    /// Scope to search: note, frontmatter, body, section, or line. Defaults to note.
    #[serde(default)]
    pub scope: TagScope,
    /// Return detailed section metadata when true. Defaults to compact LLM-friendly output.
    #[serde(default)]
    pub verbose: bool,
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
    /// Heading text, heading anchor, or slash-separated heading path.
    pub heading: Option<String>,
    /// Block id without the leading caret.
    pub block_id: Option<String>,
    /// Github-style line reference, e.g. #L1 or #L1-L99.
    pub line: Option<String>,
}

impl ReadSectionRequest {
    fn into_parts(self) -> Result<(String, SectionSelector), String> {
        let Self {
            note,
            heading,
            block_id,
            line,
        } = self;

        let selector = match (heading, block_id, line) {
            (Some(heading), None, None) => SectionSelector::Heading { heading },
            (None, Some(block_id), None) => SectionSelector::Block { block_id },
            (None, None, Some(line)) => {
                let (line_start, line_end) = parse_line_range(&line)?;
                SectionSelector::Lines {
                    line_start,
                    line_end,
                }
            }
            _ => {
                return Err("provide exactly one selector: heading, block_id, or line".to_string());
            }
        };

        Ok((note, selector))
    }
}

fn parse_line_range(value: &str) -> Result<(u64, u64), String> {
    let value = value.trim();
    let value = value.strip_prefix('#').unwrap_or(value);
    let value = value.strip_prefix('L').unwrap_or(value);

    let (start, end) = match value.split_once("-L") {
        Some((start, end)) => (start, Some(end)),
        None => (value, None),
    };
    let line_start = start
        .parse::<u64>()
        .map_err(|_| "line must use #L1 or #L1-L99 format".to_string())?;
    let line_end = end
        .unwrap_or(start)
        .parse::<u64>()
        .map_err(|_| "line must use #L1 or #L1-L99 format".to_string())?;

    if line_start == 0 || line_end < line_start {
        return Err("invalid line range".to_string());
    }

    Ok((line_start, line_end))
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
        description = "Parse one Markdown note into compact headings, local links, embeds, tags, block ids, and frontmatter"
    )]
    fn parse_note(
        &self,
        Parameters(ParseNoteRequest { note }): Parameters<ParseNoteRequest>,
    ) -> Result<Json<ParseNoteResult>, String> {
        run_tool("parse_note", || self.queries().parse_note_result(&note))
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
        description = "Get outgoing local links from one note. Defaults to compact location output; set verbose=true for source spans and snippets"
    )]
    fn get_outlinks(
        &self,
        Parameters(OutlinksRequest { note, verbose }): Parameters<OutlinksRequest>,
    ) -> Result<Json<OutlinksOutput>, String> {
        run_tool("get_outlinks", || {
            self.queries().get_outlinks_output(&note, verbose)
        })
    }

    #[tool(
        description = "Get backlinks to a note or Obsidian reference. Defaults to compact location output; set verbose=true for source spans and snippets"
    )]
    fn get_backlinks(
        &self,
        Parameters(BacklinksRequest { target, verbose }): Parameters<BacklinksRequest>,
    ) -> Result<Json<BacklinksOutput>, String> {
        run_tool("get_backlinks", || {
            self.queries().get_backlinks_output(&target, verbose)
        })
    }

    #[tool(
        description = "List unique tag names across body tag nodes and frontmatter tags; use get_tags to locate selected tags"
    )]
    fn list_tags(
        &self,
        Parameters(ListTagsRequest { scope }): Parameters<ListTagsRequest>,
    ) -> Result<Json<ListTagsResult>, String> {
        run_tool("list_tags", || self.queries().list_tags(scope))
    }

    #[tool(
        description = "Locate selected tags and return note or line references; use read_section on returned paths to inspect context"
    )]
    fn get_tags(
        &self,
        Parameters(TagsRequest {
            tags,
            scope,
            verbose,
        }): Parameters<TagsRequest>,
    ) -> Result<Json<GetTagsResult>, String> {
        if tags.is_empty() {
            return Err(
                "provide at least one tag; use list_tags to discover tag names".to_string(),
            );
        }
        run_tool("get_tags", || {
            self.queries().get_tags(&tags, scope, verbose)
        })
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
        description = "Read exactly one heading section, block id, or line reference from a note with source span"
    )]
    fn read_section(
        &self,
        Parameters(request): Parameters<ReadSectionRequest>,
    ) -> Result<Json<ReadSectionResult>, String> {
        let (note, selector) = request.into_parts()?;
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
    fn get_vault_graph(
        &self,
        _: Parameters<EmptyRequest>,
    ) -> Result<Json<VaultGraphResult>, String> {
        run_tool("get_vault_graph", || self.queries().get_vault_graph())
    }

    #[tool(
        description = "Return a bounded local-link graph neighborhood around one note or reference; preferred graph tool for normal agent context"
    )]
    fn get_graph_neighborhood(
        &self,
        Parameters(request): Parameters<GraphNeighborhoodRequest>,
    ) -> Result<Json<VaultGraphResult>, String> {
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

#[cfg(test)]
mod tests {
    use super::{ObsidianVaultMcp, ReadSectionRequest};
    use crate::query::SectionSelector;

    const FORBIDDEN_TOP_LEVEL_SCHEMA_KEYS: [&str; 6] =
        ["oneOf", "anyOf", "allOf", "enum", "const", "not"];

    #[test]
    fn all_tool_input_schemas_have_plain_object_roots() {
        for tool in ObsidianVaultMcp::tool_definitions() {
            assert_eq!(
                tool.input_schema
                    .get("type")
                    .and_then(|value| value.as_str()),
                Some("object"),
                "{} input schema must have an object root",
                tool.name
            );

            for key in FORBIDDEN_TOP_LEVEL_SCHEMA_KEYS {
                assert!(
                    !tool.input_schema.contains_key(key),
                    "{} input schema must not have top-level {key}",
                    tool.name
                );
            }
        }
    }

    #[test]
    fn read_section_request_schema_has_plain_object_root() {
        let schema =
            serde_json::to_value(schemars::schema_for!(ReadSectionRequest)).expect("schema json");

        assert_eq!(schema["type"], "object");
        for key in FORBIDDEN_TOP_LEVEL_SCHEMA_KEYS {
            assert!(schema.get(key).is_none());
        }
    }

    #[test]
    fn read_section_request_accepts_obsidian_line_reference() {
        let (_, selector) = ReadSectionRequest {
            note: "note.md".to_string(),
            heading: None,
            block_id: None,
            line: Some("#L3-L5".to_string()),
        }
        .into_parts()
        .expect("line selector");

        assert!(matches!(
            selector,
            SectionSelector::Lines {
                line_start: 3,
                line_end: 5
            }
        ));
    }
}
