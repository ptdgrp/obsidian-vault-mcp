use crate::{
    mutation::{EditSectionResult, RenameResult, VaultMutations},
    query::{
        AuditLinksResult, BacklinksResult, FrontmatterQueryOptions, FrontmatterQueryResult,
        GetCategoryResult, GetTagResult, ListCategoriesResult, ListNotesResult, ListTagsResult,
        McpNonNegativeInteger, NeighborhoodDirection, NeighborhoodResult, NoteOutlineResult,
        NoteStatsResult, NoteStructureResult, OutlinksResult, ReadNoteResult, ResolveRefResult,
        SearchRegexResult, SearchTextResult, SectionSelector, TagScope, VaultQueries,
    },
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

pub async fn run_mcp_server(vault: Arc<Vault>) -> anyhow::Result<()> {
    let service = ObsidianVaultMcp::new(vault);
    let server = service
        .serve((tokio::io::stdin(), tokio::io::stdout()))
        .await?;
    server.waiting().await?;
    Ok(())
}

#[derive(Clone)]
pub struct AppState {
    pub queries: VaultQueries,
    pub mutations: VaultMutations,
}

#[derive(Clone)]
pub struct ObsidianVaultMcp {
    state: Arc<AppState>,
    tool_router: ToolRouter<Self>,
}

impl ObsidianVaultMcp {
    pub fn new(vault: Arc<Vault>) -> Self {
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
/// Page through visible Markdown notes after optional vault-relative path filters.
pub struct ListNotesRequest {
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default = "default_page")]
    #[schemars(with = "McpNonNegativeInteger")]
    pub page: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AuditLinksRequest {
    #[serde(default = "default_page")]
    #[schemars(with = "McpNonNegativeInteger")]
    pub page: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NeighborhoodRequest {
    pub target: String,
    #[serde(default = "default_neighborhood_depth")]
    #[schemars(with = "McpNonNegativeInteger")]
    pub depth: usize,
    #[serde(default)]
    pub direction: NeighborhoodDirection,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for reading one Markdown note.
pub struct ReadNoteRequest {
    /// Vault-relative path, note stem, alias, or bare Obsidian reference.
    pub note: String,
    /// Optional character limit for this request.
    #[schemars(with = "Option<McpNonNegativeInteger>")]
    pub max_chars: Option<usize>,
    /// Heading text, heading anchor, or slash-separated heading path.
    pub heading: Option<String>,
    /// Block id without the leading caret.
    pub block_id: Option<String>,
    /// Line reference with an optional second `L`, e.g. #L1, #L1-L99, or #L1-99.
    pub line: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for inspecting one Markdown note's structure.
pub struct NoteStructureRequest {
    /// Vault-relative path, note stem, or alias.
    pub note: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for inspecting one note's heading outline.
pub struct NoteOutlineRequest {
    /// Vault-relative path, note stem, or alias.
    pub note: String,
    /// One-based page number. Each page contains up to 100 headings.
    #[serde(default = "default_page")]
    #[schemars(with = "McpNonNegativeInteger")]
    pub page: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NoteStatsRequest {
    /// Vault-relative path, note stem, alias, or bare heading/block reference.
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
    /// Vault-relative glob patterns. A backlink source note must match at least one when non-empty.
    #[serde(default)]
    pub include: Vec<String>,
    /// Vault-relative glob patterns. Matching backlink source notes are excluded.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// One-based page number. Each page contains up to 50 backlink occurrences.
    #[serde(default = "default_page")]
    #[schemars(with = "McpNonNegativeInteger")]
    pub page: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for outgoing local link lookup.
pub struct OutlinksRequest {
    /// Vault-relative path, note stem, or alias.
    pub note: String,
    /// One-based page number. Each page contains up to 50 link occurrences.
    #[serde(default = "default_page")]
    #[schemars(with = "McpNonNegativeInteger")]
    pub page: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for listing tags.
pub struct ListTagsRequest {
    /// Scope to search: note, frontmatter, body, section, or line. Defaults to note.
    #[serde(default)]
    pub scope: TagScope,
    /// Vault-relative glob patterns. A note must match at least one when non-empty.
    #[serde(default)]
    pub include: Vec<String>,
    /// Vault-relative glob patterns. Matching notes are excluded.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// One-based page number. Each page contains up to 100 tags.
    #[serde(default = "default_page")]
    #[schemars(with = "McpNonNegativeInteger")]
    pub page: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for locating one tag.
pub struct GetTagRequest {
    /// Exact tag to locate. Both "状态/身体" and "#状态/身体" are accepted.
    pub tag: String,
    /// Scope to search: note, frontmatter, body, section, or line. Defaults to note.
    #[serde(default)]
    pub scope: TagScope,
    /// Vault-relative glob patterns. A note must match at least one when non-empty.
    #[serde(default)]
    pub include: Vec<String>,
    /// Vault-relative glob patterns. Matching notes are excluded.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// One-based page number. Each page contains up to 100 locators.
    #[serde(default = "default_page")]
    #[schemars(with = "McpNonNegativeInteger")]
    pub page: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for listing folder-derived categories.
pub struct ListCategoriesRequest {
    /// Vault-relative glob patterns. A note must match at least one when non-empty.
    #[serde(default)]
    pub include: Vec<String>,
    /// Vault-relative glob patterns. Matching notes are excluded.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// One-based page number. Each page contains up to 100 categories.
    #[serde(default = "default_page")]
    #[schemars(with = "McpNonNegativeInteger")]
    pub page: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for locating one folder-derived category.
pub struct GetCategoryRequest {
    /// Exact folder-derived category name to locate.
    pub category: String,
    /// Vault-relative glob patterns. A note must match at least one when non-empty.
    #[serde(default)]
    pub include: Vec<String>,
    /// Vault-relative glob patterns. Matching notes are excluded.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// One-based page number. Each page contains up to 100 notes.
    #[serde(default = "default_page")]
    #[schemars(with = "McpNonNegativeInteger")]
    pub page: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for literal text search.
pub struct SearchTextRequest {
    /// Literal text to search for.
    pub query: String,
    /// Whether matching should be case-sensitive.
    #[serde(default)]
    pub case_sensitive: bool,
    /// Vault-relative glob patterns. A note must match at least one when non-empty.
    #[serde(default)]
    pub include: Vec<String>,
    /// Vault-relative glob patterns. Matching notes are excluded.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// One-based page number. Each page contains up to 50 matching lines.
    #[serde(default = "default_page")]
    #[schemars(with = "McpNonNegativeInteger")]
    pub page: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Input for regular expression search.
pub struct SearchRegexRequest {
    /// Rust regex pattern matched line by line.
    pub pattern: String,
    /// Whether matching should be case-sensitive.
    #[serde(default)]
    pub case_sensitive: bool,
    /// Vault-relative glob patterns. A note must match at least one when non-empty.
    #[serde(default)]
    pub include: Vec<String>,
    /// Vault-relative glob patterns. Matching notes are excluded.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// One-based page number. Each page contains up to 50 matching lines.
    #[serde(default = "default_page")]
    #[schemars(with = "McpNonNegativeInteger")]
    pub page: usize,
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
    /// Line reference with an optional second `L`, e.g. #L1-L99 or #L1-99.
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
    /// Line reference with an optional second `L`, e.g. #L1-L99 or #L1-99.
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
    /// Line reference with an optional second `L`, e.g. #L1-L99 or #L1-99.
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
    /// Existing safe vault-relative Markdown path.
    pub path: String,
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

pub(crate) fn read_note_parts(
    note: String,
    heading: Option<String>,
    block_id: Option<String>,
    line: Option<String>,
) -> Result<(String, Option<SectionSelector>), String> {
    if heading.is_none() && block_id.is_none() && line.is_none() {
        return Ok((note, None));
    }
    section_parts(note, heading, block_id, line).map(|(note, selector)| (note, Some(selector)))
}

fn parse_line_range(value: &str) -> Result<(u64, u64), String> {
    let value = value.trim();
    let value = value.strip_prefix('#').unwrap_or(value);
    let value = value.strip_prefix('L').unwrap_or(value);

    let (start, end) = match value.split_once('-') {
        Some((start, end)) => (start, Some(end)),
        None => (value, None),
    };
    let line_start = start
        .parse::<u64>()
        .map_err(|_| "line must use #L1, #L1-L99, or #L1-99 format".to_string())?;
    let line_end = end
        .map(|end| end.strip_prefix('L').unwrap_or(end))
        .unwrap_or(start)
        .parse::<u64>()
        .map_err(|_| "line must use #L1, #L1-L99, or #L1-99 format".to_string())?;

    if line_start == 0 || line_end < line_start {
        return Err("invalid line range".to_string());
    }

    Ok((line_start, line_end))
}

pub type FrontmatterQueryRequest = FrontmatterQueryOptions;

fn default_dry_run() -> bool {
    true
}

fn default_page() -> usize {
    1
}

fn default_neighborhood_depth() -> usize {
    1
}

#[tool_router]
impl ObsidianVaultMcp {
    #[tool(description = "Page through visible Markdown notes for lightweight navigation.")]
    fn list_notes(
        &self,
        Parameters(request): Parameters<ListNotesRequest>,
    ) -> Result<Json<ListNotesResult>, String> {
        run_tool(
            "list_notes",
            request,
            |ListNotesRequest {
                 include,
                 exclude,
                 page,
             }| { self.queries().list_notes(&include, &exclude, page) },
        )
    }

    #[tool(description = "Audit unresolved and ambiguous local links across the visible vault.")]
    fn audit_links(
        &self,
        Parameters(request): Parameters<AuditLinksRequest>,
    ) -> Result<Json<AuditLinksResult>, String> {
        run_tool("audit_links", request, |AuditLinksRequest { page }| {
            self.queries().audit_links(page)
        })
    }

    #[tool(description = "Return a bounded resolved-link neighborhood around one note reference.")]
    fn get_note_neighborhood(
        &self,
        Parameters(request): Parameters<NeighborhoodRequest>,
    ) -> Result<Json<NeighborhoodResult>, String> {
        run_tool(
            "get_note_neighborhood",
            request,
            |NeighborhoodRequest {
                 target,
                 depth,
                 direction,
             }| {
                self.queries()
                    .get_note_neighborhood(&target, depth, direction)
            },
        )
    }

    #[tool(
        description = "Read a note, heading section, block, or line range. Use a bare reference such as Note#Heading, Note#^block, Note#L1-L20, Note#L1-20, or exactly one explicit heading, block_id, or line selector. If provided, max_chars controls this request's Unicode-character truncation boundary."
    )]
    fn read_note(
        &self,
        Parameters(request): Parameters<ReadNoteRequest>,
    ) -> Result<Json<ReadNoteResult>, String> {
        run_tool(
            "read_note",
            request,
            |ReadNoteRequest {
                 note,
                 max_chars,
                 heading,
                 block_id,
                 line,
             }| {
                let (note, selector) =
                    read_note_parts(note, heading, block_id, line).map_err(anyhow::Error::msg)?;
                self.queries().read_note(&note, max_chars, selector)
            },
        )
    }

    #[tool(
        description = "Return one Markdown note's compact structure: headings, local links, embeds, tags, block ids, and frontmatter"
    )]
    fn get_note_structure(
        &self,
        Parameters(request): Parameters<NoteStructureRequest>,
    ) -> Result<Json<NoteStructureResult>, String> {
        run_tool(
            "get_note_structure",
            request,
            |NoteStructureRequest { note }| self.queries().get_note_structure(&note),
        )
    }

    #[tool(
        description = "Return one note or bare heading/block reference's word, character, and line counts"
    )]
    fn get_note_stats(
        &self,
        Parameters(request): Parameters<NoteStatsRequest>,
    ) -> Result<Json<NoteStatsResult>, String> {
        run_tool("get_note_stats", request, |NoteStatsRequest { note }| {
            self.queries().get_note_stats(&note)
        })
    }

    #[tool(
        description = "Return one note's selectable non-H1 headings as a flat paged list without body text; use before read_note."
    )]
    fn get_note_outline(
        &self,
        Parameters(request): Parameters<NoteOutlineRequest>,
    ) -> Result<Json<NoteOutlineResult>, String> {
        run_tool(
            "get_note_outline",
            request,
            |NoteOutlineRequest { note, page }| self.queries().get_note_outline(&note, page),
        )
    }

    #[tool(
        description = "Search visible Markdown notes with literal text. Optional include and exclude use vault-relative glob patterns; include patterns are unioned, empty arrays do not restrict, and excludes take precedence."
    )]
    fn search_text(
        &self,
        Parameters(request): Parameters<SearchTextRequest>,
    ) -> Result<Json<SearchTextResult>, String> {
        run_tool(
            "search_text",
            request,
            |SearchTextRequest {
                 query,
                 case_sensitive,
                 include,
                 exclude,
                 page,
             }| {
                self.queries()
                    .search_text(&query, case_sensitive, &include, &exclude, page)
            },
        )
    }

    #[tool(
        description = "Search visible Markdown notes with a Rust regular expression. Optional include and exclude use vault-relative glob patterns; include patterns are unioned, empty arrays do not restrict, and excludes take precedence."
    )]
    fn search_regex(
        &self,
        Parameters(request): Parameters<SearchRegexRequest>,
    ) -> Result<Json<SearchRegexResult>, String> {
        run_tool(
            "search_regex",
            request,
            |SearchRegexRequest {
                 pattern,
                 case_sensitive,
                 include,
                 exclude,
                 page,
             }| {
                self.queries()
                    .search_regex(&pattern, case_sensitive, &include, &exclude, page)
            },
        )
    }

    #[tool(
        description = "Resolve an Obsidian reference such as [[Note#Heading]] to a note, heading, or block without guessing ambiguous targets"
    )]
    fn resolve_ref(
        &self,
        Parameters(request): Parameters<ResolveRefRequest>,
    ) -> Result<Json<ResolveRefResult>, String> {
        run_tool("resolve_ref", request, |ResolveRefRequest { reference }| {
            self.queries().resolve_ref(&reference)
        })
    }

    #[tool(
        description = "Get outgoing local links from one note as compact resolved, ambiguous, and unresolved target groups"
    )]
    fn get_outlinks(
        &self,
        Parameters(request): Parameters<OutlinksRequest>,
    ) -> Result<Json<OutlinksResult>, String> {
        run_tool("get_outlinks", request, |OutlinksRequest { note, page }| {
            self.queries().get_outlinks(&note, page)
        })
    }

    #[tool(
        description = "Get backlinks to a uniquely resolved note, heading, or block. Optional include and exclude filter source-note paths; excludes take precedence."
    )]
    fn get_backlinks(
        &self,
        Parameters(request): Parameters<BacklinksRequest>,
    ) -> Result<Json<BacklinksResult>, String> {
        run_tool(
            "get_backlinks",
            request,
            |BacklinksRequest {
                 target,
                 include,
                 exclude,
                 page,
             }| {
                self.queries()
                    .get_backlinks(&target, &include, &exclude, page)
            },
        )
    }

    #[tool(
        description = "List unique tag names across visible notes. Optional include and exclude use vault-relative glob patterns; include patterns are unioned, empty arrays do not restrict, and excludes take precedence."
    )]
    fn list_tags(
        &self,
        Parameters(request): Parameters<ListTagsRequest>,
    ) -> Result<Json<ListTagsResult>, String> {
        run_tool(
            "list_tags",
            request,
            |ListTagsRequest {
                 scope,
                 include,
                 exclude,
                 page,
             }| { self.queries().list_tags(scope, &include, &exclude, page) },
        )
    }

    #[tool(
        description = "Locate one tag in visible notes. Optional include and exclude use vault-relative glob patterns; include patterns are unioned, empty arrays do not restrict, and excludes take precedence."
    )]
    fn get_tag(
        &self,
        Parameters(request): Parameters<GetTagRequest>,
    ) -> Result<Json<GetTagResult>, String> {
        run_tool(
            "get_tag",
            request,
            |GetTagRequest {
                 tag,
                 scope,
                 include,
                 exclude,
                 page,
             }| {
                if tag.trim().trim_start_matches('#').is_empty() {
                    return Err(anyhow::anyhow!(
                        "provide a non-empty tag; use list_tags to discover tag names"
                    ));
                }
                self.queries()
                    .get_tag(&tag, scope, &include, &exclude, page)
            },
        )
    }

    #[tool(
        description = "List folder-derived categories from visible notes. Optional include and exclude use vault-relative glob patterns; include patterns are unioned, empty arrays do not restrict, and excludes take precedence."
    )]
    fn list_categories(
        &self,
        Parameters(request): Parameters<ListCategoriesRequest>,
    ) -> Result<Json<ListCategoriesResult>, String> {
        run_tool(
            "list_categories",
            request,
            |ListCategoriesRequest {
                 include,
                 exclude,
                 page,
             }| { self.queries().list_categories(&include, &exclude, page) },
        )
    }

    #[tool(
        description = "Locate one folder-derived category in visible notes. Optional include and exclude use vault-relative glob patterns; include patterns are unioned, empty arrays do not restrict, and excludes take precedence."
    )]
    fn get_category(
        &self,
        Parameters(request): Parameters<GetCategoryRequest>,
    ) -> Result<Json<GetCategoryResult>, String> {
        run_tool(
            "get_category",
            request,
            |GetCategoryRequest {
                 category,
                 include,
                 exclude,
                 page,
             }| {
                if category.trim().trim_matches('/').is_empty() {
                    return Err(anyhow::anyhow!(
                        "provide a non-empty category; use list_categories to discover category names"
                    ));
                }
                self.queries()
                    .get_category(&category, &include, &exclude, page)
            },
        )
    }

    #[tool(
        description = "Query notes by a top-level frontmatter field using explicit exists, equals, or regex mode"
    )]
    fn query_frontmatter(
        &self,
        Parameters(request): Parameters<FrontmatterQueryRequest>,
    ) -> Result<Json<FrontmatterQueryResult>, String> {
        run_tool("query_frontmatter", request, |request| {
            self.queries().query_frontmatter(request)
        })
    }

    #[tool(
        description = "Append content at the end of exactly one heading, block, or line section. This uses structural selection, not text matching."
    )]
    fn append_section(
        &self,
        Parameters(request): Parameters<AppendSectionRequest>,
    ) -> Result<Json<EditSectionResult>, String> {
        run_tool(
            "append_section",
            request,
            |AppendSectionRequest {
                 note,
                 heading,
                 block_id,
                 line,
                 content,
             }| {
                let (note, selector) =
                    section_parts(note, heading, block_id, line).map_err(anyhow::Error::msg)?;
                self.mutations().append_section(&note, selector, &content)
            },
        )
    }

    #[tool(
        description = "Replace exactly one heading, block, or line section with new content. This uses structural selection, not text matching."
    )]
    fn replace_section(
        &self,
        Parameters(request): Parameters<ReplaceSectionRequest>,
    ) -> Result<Json<EditSectionResult>, String> {
        run_tool(
            "replace_section",
            request,
            |ReplaceSectionRequest {
                 note,
                 heading,
                 block_id,
                 line,
                 content,
             }| {
                let (note, selector) =
                    section_parts(note, heading, block_id, line).map_err(anyhow::Error::msg)?;
                self.mutations().replace_section(&note, selector, &content)
            },
        )
    }

    #[tool(
        description = "Delete exactly one heading, block, or line section. This uses structural selection, not text matching."
    )]
    fn delete_section(
        &self,
        Parameters(request): Parameters<DeleteSectionRequest>,
    ) -> Result<Json<EditSectionResult>, String> {
        run_tool(
            "delete_section",
            request,
            |DeleteSectionRequest {
                 note,
                 heading,
                 block_id,
                 line,
             }| {
                let (note, selector) =
                    section_parts(note, heading, block_id, line).map_err(anyhow::Error::msg)?;
                self.mutations().delete_section(&note, selector)
            },
        )
    }

    #[tool(
        description = "Rename one heading and update uniquely resolved Obsidian wikilinks to it. Set dry_run to false to apply; preview is the default."
    )]
    fn rename_heading(
        &self,
        Parameters(request): Parameters<RenameHeadingRequest>,
    ) -> Result<Json<RenameResult>, String> {
        run_tool(
            "rename_heading",
            request,
            |RenameHeadingRequest {
                 note,
                 old_heading,
                 new_heading,
                 dry_run,
             }| {
                self.mutations()
                    .rename_heading(&note, &old_heading, &new_heading, dry_run)
            },
        )
    }

    #[tool(
        description = "Move a note to a new vault-relative path and update uniquely resolved wikilinks. Set dry_run to false to apply."
    )]
    fn rename_note(
        &self,
        Parameters(request): Parameters<RenameNoteRequest>,
    ) -> Result<Json<RenameResult>, String> {
        run_tool(
            "rename_note",
            request,
            |RenameNoteRequest {
                 path,
                 new_path,
                 dry_run,
             }| { self.mutations().rename_note(&path, &new_path, dry_run) },
        )
    }

    #[tool(
        description = "Rename one block id and update uniquely resolved Obsidian wikilinks. Set dry_run to false to apply."
    )]
    fn rename_block_id(
        &self,
        Parameters(request): Parameters<RenameBlockIdRequest>,
    ) -> Result<Json<RenameResult>, String> {
        run_tool(
            "rename_block_id",
            request,
            |RenameBlockIdRequest {
                 note,
                 old_block_id,
                 new_block_id,
                 dry_run,
             }| {
                self.mutations()
                    .rename_block_id(&note, &old_block_id, &new_block_id, dry_run)
            },
        )
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

pub(crate) fn run_tool<A, T>(
    tool_name: &'static str,
    arguments: A,
    run: impl FnOnce(A) -> anyhow::Result<T>,
) -> Result<Json<T>, String>
where
    A: std::fmt::Debug,
{
    let span = tracing::info_span!(parent: None, "mcp.tool", tool.name = tool_name);
    let _enter = span.enter();
    let started = Instant::now();

    let (input_preview, input_truncated) = crate::logging::input_preview(&arguments);
    tracing::info!(input.preview = %input_preview, input.truncated = input_truncated, "tool.call.start");
    match run(arguments) {
        Ok(result) => {
            tracing::info!(
                duration_ms = started.elapsed().as_millis() as u64,
                "tool.call.ok"
            );
            Ok(Json(result))
        }
        Err(error) => {
            let message = error.to_string();
            let error = crate::format_error_chain(&error);
            tracing::error!(
                duration_ms = started.elapsed().as_millis() as u64,
                error.type = "mcp.tool.error",
                error = %error,
                "tool.call.error"
            );
            Err(message)
        }
    }
}

#[cfg(test)]
mod tests;
