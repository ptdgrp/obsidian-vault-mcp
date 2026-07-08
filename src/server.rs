use crate::{
    mutation::{EditSectionResult, RenameResult, VaultMutations},
    query::{
        AmbiguousLinksResult, BacklinksOutput, ContextResult, FrontmatterQueryOptions,
        FrontmatterQueryResult, GetCategoriesResult, GetTagsResult, GraphNeighborhoodOptions,
        ListCategoriesResult, ListNotesResult, ListTagsResult, NoteOutlineResult, NoteStatsResult,
        NoteStructureResult, OutlinksOutput, ReadNoteResult, ReadSectionResult, SearchRegexResult,
        SearchTextResult, SectionSelector, TagScope, UnresolvedLinksResult, VaultFilesOptions,
        VaultFilesResult, VaultGraphResult, VaultQueries, WordCountMode,
    },
    resolver::ResolveResult,
    vault::Vault,
};
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
use std::{sync::Arc, time::Instant};

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
    pub mutations: VaultMutations,
}

impl ObsidianVaultMcp {
    pub fn new(vault: Vault) -> Self {
        let queries = VaultQueries::new(vault);
        Self {
            state: Arc::new(AppState {
                mutations: VaultMutations::new(queries.clone()),
                queries,
            }),
            tool_router: Self::tool_router(),
        }
    }

    fn queries(&self) -> VaultQueries {
        self.state.queries.clone()
    }

    fn mutations(&self) -> VaultMutations {
        self.state.mutations.clone()
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
    /// Optional byte limit for this request.
    pub max_bytes: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for inspecting one Markdown note's structure.
pub struct NoteStructureRequest {
    /// Vault-relative path, note stem, or alias.
    pub note: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NoteStatsRequest {
    /// Vault-relative path, note stem, or alias.
    pub note: String,
    /// Word counting strategy. Defaults to `source` for backward-compatible raw Markdown counts.
    #[serde(default)]
    pub word_count_mode: WordCountMode,
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
/// Input for locating folder-derived categories.
pub struct CategoriesRequest {
    /// Exact folder-derived category names to locate.
    pub categories: Vec<String>,
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
        section_parts(self.note, self.heading, self.block_id, self.line)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for appending content at the end of one selected section.
pub struct AppendSectionRequest {
    /// Vault-relative path, note stem, or alias.
    pub note: String,
    /// Heading text, heading anchor, or slash-separated heading path.
    pub heading: Option<String>,
    /// Block id without the leading caret.
    pub block_id: Option<String>,
    /// Github-style line reference, e.g. #L1-L99.
    pub line: Option<String>,
    /// Text appended at the selected section boundary.
    pub content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for replacing an entire selected section.
pub struct ReplaceSectionRequest {
    /// Vault-relative path, note stem, or alias.
    pub note: String,
    /// Heading text, heading anchor, or slash-separated heading path.
    pub heading: Option<String>,
    /// Block id without the leading caret.
    pub block_id: Option<String>,
    /// Github-style line reference, e.g. #L1-L99.
    pub line: Option<String>,
    /// Replacement content for the entire selected section.
    pub content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for deleting an entire selected section.
pub struct DeleteSectionRequest {
    /// Vault-relative path, note stem, or alias.
    pub note: String,
    /// Heading text, heading anchor, or slash-separated heading path.
    pub heading: Option<String>,
    /// Block id without the leading caret.
    pub block_id: Option<String>,
    /// Github-style line reference, e.g. #L1-L99.
    pub line: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for safely renaming one heading and its uniquely resolved wikilink references.
pub struct RenameHeadingRequest {
    /// Vault-relative path, note stem, or alias.
    pub note: String,
    /// Current heading text or anchor.
    pub old_heading: String,
    /// Replacement heading text.
    pub new_heading: String,
    /// Preview changed notes and references without writing. Defaults to true.
    #[serde(default = "default_dry_run")]
    pub dry_run: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for moving a note and repairing its uniquely resolved wikilinks.
pub struct RenameNoteRequest {
    /// Existing vault-relative path, note stem, or alias.
    pub note: String,
    /// New vault-relative Markdown path. Parent directories are created when applying.
    pub new_path: String,
    /// Preview changed notes and references without writing. Defaults to true.
    #[serde(default = "default_dry_run")]
    pub dry_run: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for safely renaming one block id and its uniquely resolved wikilink references.
pub struct RenameBlockIdRequest {
    /// Vault-relative path, note stem, or alias.
    pub note: String,
    /// Existing block id without the leading caret.
    pub old_block_id: String,
    /// Replacement block id without the leading caret.
    pub new_block_id: String,
    /// Preview changed notes and references without writing. Defaults to true.
    #[serde(default = "default_dry_run")]
    pub dry_run: bool,
}

pub(crate) fn section_parts(
    note: String,
    heading: Option<String>,
    block_id: Option<String>,
    line: Option<String>,
) -> Result<(String, SectionSelector), String> {
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

fn default_dry_run() -> bool {
    true
}

#[tool_router]
impl ObsidianVaultMcp {
    #[tool(
        description = "List visible Markdown notes with path, display title, and human-readable size. Title priority: first level-one heading, frontmatter title, then pathname."
    )]
    fn list_notes(&self, _: Parameters<EmptyRequest>) -> Result<Json<ListNotesResult>, String> {
        run_tool("list_notes", || self.queries().list_notes())
    }

    #[tool(
        description = "Read a truncated prefix of a note.\nUse only when:\n- You need to scan the beginning of a note and don't know its structure yet.\n- You've already ruled out get_note_outline (for structure) and read_section (for targeted access).\n\nFor any operation with a known line, heading, or block id - use read_section instead.\nIf provided, max_bytes controls this request's truncation boundary."
    )]
    fn read_note(
        &self,
        Parameters(ReadNoteRequest { note, max_bytes }): Parameters<ReadNoteRequest>,
    ) -> Result<Json<ReadNoteResult>, String> {
        run_tool("read_note", || self.queries().read_note(&note, max_bytes))
    }

    #[tool(
        description = "Return one Markdown note's compact structure: headings, local links, embeds, tags, block ids, and frontmatter"
    )]
    fn get_note_structure(
        &self,
        Parameters(NoteStructureRequest { note }): Parameters<NoteStructureRequest>,
    ) -> Result<Json<NoteStructureResult>, String> {
        run_tool("get_note_structure", || {
            self.queries().get_note_structure(&note)
        })
    }

    #[tool(description = "Return one note's word count, character count, and total backlink count")]
    fn get_note_stats(
        &self,
        Parameters(NoteStatsRequest {
            note,
            word_count_mode,
        }): Parameters<NoteStatsRequest>,
    ) -> Result<Json<NoteStatsResult>, String> {
        run_tool("get_note_stats", || {
            self.queries().get_note_stats(&note, word_count_mode)
        })
    }

    #[tool(
        description = "Return one note's selectable non-H1 heading tree without body text; use before selecting a section"
    )]
    fn get_note_outline(
        &self,
        Parameters(NoteStructureRequest { note }): Parameters<NoteStructureRequest>,
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
        description = "List unique folder-derived category names; use get_categories to locate selected categories"
    )]
    fn list_categories(
        &self,
        _: Parameters<EmptyRequest>,
    ) -> Result<Json<ListCategoriesResult>, String> {
        run_tool("list_categories", || self.queries().list_categories())
    }

    #[tool(
        description = "Locate selected folder-derived categories and return matching Markdown note files"
    )]
    fn get_categories(
        &self,
        Parameters(CategoriesRequest { categories }): Parameters<CategoriesRequest>,
    ) -> Result<Json<GetCategoriesResult>, String> {
        if categories.is_empty() {
            return Err(
                "provide at least one category; use list_categories to discover category names"
                    .to_string(),
            );
        }
        run_tool("get_categories", || {
            self.queries().get_categories(&categories)
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
        description = "Collect bounded navigation context grouped as current note, outlinks, and backlinks; use get_note_outline then read_section for note content"
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
        description = "Resolve a reference, then collect bounded navigation context; use get_note_outline then read_section for note content"
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
        let selector_count = [
            request.heading.is_some(),
            request.block_id.is_some(),
            request.line.is_some(),
        ]
        .into_iter()
        .filter(|selected| *selected)
        .count();
        if selector_count == 0 {
            let note = request.note;
            return run_tool("read_section", || {
                let message = self.queries().read_section_selector_hint(&note)?;
                anyhow::bail!("{message}")
            });
        }

        let (note, selector) = request.into_parts()?;
        run_tool("read_section", || {
            self.queries().read_section(&note, selector)
        })
    }

    #[tool(
        description = "Append content at the end of exactly one heading, block, or line section. This uses structural selection, not text matching."
    )]
    fn append_section(
        &self,
        Parameters(AppendSectionRequest {
            note,
            heading,
            block_id,
            line,
            content,
        }): Parameters<AppendSectionRequest>,
    ) -> Result<Json<EditSectionResult>, String> {
        let (note, selector) = section_parts(note, heading, block_id, line)?;
        run_tool("append_section", || {
            self.mutations().append_section(&note, selector, &content)
        })
    }

    #[tool(
        description = "Replace exactly one heading, block, or line section with new content. This uses structural selection, not text matching."
    )]
    fn replace_section(
        &self,
        Parameters(ReplaceSectionRequest {
            note,
            heading,
            block_id,
            line,
            content,
        }): Parameters<ReplaceSectionRequest>,
    ) -> Result<Json<EditSectionResult>, String> {
        let (note, selector) = section_parts(note, heading, block_id, line)?;
        run_tool("replace_section", || {
            self.mutations().replace_section(&note, selector, &content)
        })
    }

    #[tool(
        description = "Delete exactly one heading, block, or line section. This uses structural selection, not text matching."
    )]
    fn delete_section(
        &self,
        Parameters(DeleteSectionRequest {
            note,
            heading,
            block_id,
            line,
        }): Parameters<DeleteSectionRequest>,
    ) -> Result<Json<EditSectionResult>, String> {
        let (note, selector) = section_parts(note, heading, block_id, line)?;
        run_tool("delete_section", || {
            self.mutations().delete_section(&note, selector)
        })
    }

    #[tool(
        description = "Rename one heading and update uniquely resolved Obsidian wikilinks to it. Set dry_run to false to apply; preview is the default."
    )]
    fn rename_heading(
        &self,
        Parameters(RenameHeadingRequest {
            note,
            old_heading,
            new_heading,
            dry_run,
        }): Parameters<RenameHeadingRequest>,
    ) -> Result<Json<RenameResult>, String> {
        run_tool("rename_heading", || {
            self.mutations()
                .rename_heading(&note, &old_heading, &new_heading, dry_run)
        })
    }

    #[tool(
        description = "Move a note to a new vault-relative path and update uniquely resolved wikilinks. Set dry_run to false to apply."
    )]
    fn rename_note(
        &self,
        Parameters(RenameNoteRequest {
            note,
            new_path,
            dry_run,
        }): Parameters<RenameNoteRequest>,
    ) -> Result<Json<RenameResult>, String> {
        run_tool("rename_note", || {
            self.mutations().rename_note(&note, &new_path, dry_run)
        })
    }

    #[tool(
        description = "Rename one block id and update uniquely resolved Obsidian wikilinks. Set dry_run to false to apply."
    )]
    fn rename_block_id(
        &self,
        Parameters(RenameBlockIdRequest {
            note,
            old_block_id,
            new_block_id,
            dry_run,
        }): Parameters<RenameBlockIdRequest>,
    ) -> Result<Json<RenameResult>, String> {
        run_tool("rename_block_id", || {
            self.mutations()
                .rename_block_id(&note, &old_block_id, &new_block_id, dry_run)
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
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Obsidian vault structure tools for LLM agents, including structural section edits.",
        )
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
mod tests;
