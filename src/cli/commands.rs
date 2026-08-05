use std::sync::Arc;

use crate::{server::section_parts, vault::Vault};
use camino::Utf8PathBuf;

#[derive(Debug, clap::Subcommand)]
pub(crate) enum Command {
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

    #[command(about = "Page through visible Markdown notes for lightweight navigation.")]
    ListNotes {
        /// Vault-relative glob patterns that notes must match when non-empty.
        #[arg(long)]
        include: Vec<String>,

        /// Vault-relative glob patterns that exclude matching notes.
        #[arg(long)]
        exclude: Vec<String>,

        #[arg(long, default_value_t = 1)]
        page: usize,
    },

    #[command(about = "Audit unresolved and ambiguous local links across the visible vault.")]
    AuditLinks {
        #[arg(long, default_value_t = 1)]
        page: usize,
    },

    #[command(about = "Return a bounded resolved-link neighborhood around one note reference.")]
    GetNoteNeighborhood {
        target: String,
        #[arg(long, default_value_t = 1)]
        depth: usize,
        #[arg(long, default_value = "both")]
        direction: String,
    },

    /// Read one Markdown note, heading section, block, or line range
    ReadNote {
        note: String,

        #[arg(long)]
        heading: Option<String>,

        #[arg(long)]
        block_id: Option<String>,

        #[arg(long)]
        line: Option<String>,

        #[arg(long)]
        max_chars: Option<usize>,
    },

    /// Print one note's extracted Obsidian structure
    GetNoteStructure { note: String },

    /// Print one note's selectable non-H1 heading tree or a selected heading's ancestor chain
    GetNoteOutline {
        note: String,

        #[arg(long, default_value_t = 1)]
        page: usize,
    },

    /// Return one note's word, character, and line counts
    GetNoteStats { note: String },

    /// Resolve an Obsidian reference, e.g. [[Note#Heading]]
    ResolveRef { reference: String },

    /// Get outgoing local links from one note
    GetOutlinks {
        note: String,

        #[arg(long, default_value_t = 1)]
        page: usize,
    },

    /// Find backlinks to a note or reference
    GetBacklinks {
        target: String,

        /// Vault-relative glob patterns that backlink source notes must match when non-empty.
        #[arg(long)]
        include: Vec<String>,

        /// Vault-relative glob patterns that exclude matching backlink source notes.
        #[arg(long)]
        exclude: Vec<String>,

        #[arg(long, default_value_t = 1)]
        page: usize,
    },

    /// List unique body and frontmatter tag names
    ListTags {
        #[arg(long, default_value = "note")]
        scope: String,

        /// Vault-relative glob patterns that notes must match when non-empty.
        #[arg(long)]
        include: Vec<String>,

        /// Vault-relative glob patterns that exclude matching notes.
        #[arg(long)]
        exclude: Vec<String>,

        #[arg(long, default_value_t = 1)]
        page: usize,
    },

    /// Locate one body or frontmatter tag
    GetTag {
        tag: String,

        #[arg(long, default_value = "note")]
        scope: String,

        /// Vault-relative glob patterns that notes must match when non-empty.
        #[arg(long)]
        include: Vec<String>,

        /// Vault-relative glob patterns that exclude matching notes.
        #[arg(long)]
        exclude: Vec<String>,

        #[arg(long, default_value_t = 1)]
        page: usize,
    },

    /// List unique folder-derived category names
    ListCategories {
        /// Vault-relative glob patterns that notes must match when non-empty.
        #[arg(long)]
        include: Vec<String>,

        /// Vault-relative glob patterns that exclude matching notes.
        #[arg(long)]
        exclude: Vec<String>,

        #[arg(long, default_value_t = 1)]
        page: usize,
    },

    /// Locate one folder-derived category
    GetCategory {
        category: String,

        /// Vault-relative glob patterns that notes must match when non-empty.
        #[arg(long)]
        include: Vec<String>,

        /// Vault-relative glob patterns that exclude matching notes.
        #[arg(long)]
        exclude: Vec<String>,

        #[arg(long, default_value_t = 1)]
        page: usize,
    },

    /// Query notes by a top-level frontmatter field
    QueryFrontmatter {
        field: String,

        #[arg(long, default_value = "exists")]
        mode: String,

        #[arg(long)]
        value: Option<String>,

        /// Vault-relative glob patterns that notes must match when non-empty.
        #[arg(long)]
        include: Vec<String>,

        /// Vault-relative glob patterns that exclude matching notes.
        #[arg(long)]
        exclude: Vec<String>,

        #[arg(long, default_value_t = 1)]
        page: usize,
    },

    /// Literal search
    SearchText {
        query: String,

        #[arg(long)]
        case_sensitive: bool,

        /// Vault-relative glob patterns that notes must match when non-empty.
        #[arg(long)]
        include: Vec<String>,

        /// Vault-relative glob patterns that exclude matching notes.
        #[arg(long)]
        exclude: Vec<String>,

        #[arg(long, default_value_t = 1)]
        page: usize,
    },

    /// Regex search
    SearchRegex {
        pattern: String,

        #[arg(long)]
        case_sensitive: bool,

        /// Vault-relative glob patterns that notes must match when non-empty.
        #[arg(long)]
        include: Vec<String>,

        /// Vault-relative glob patterns that exclude matching notes.
        #[arg(long)]
        exclude: Vec<String>,

        #[arg(long, default_value_t = 1)]
        page: usize,
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
        /// Existing safe vault-relative Markdown path.
        path: String,
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
impl Command {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Serve => "serve",
            Self::GenerateDocs { .. } => "generate_docs",
            Self::Doctor => "doctor",
            Self::ListNotes { .. } => "list_notes",
            Self::AuditLinks { .. } => "audit_links",
            Self::GetNoteNeighborhood { .. } => "get_note_neighborhood",
            Self::ReadNote { .. } => "read_note",
            Self::GetNoteStructure { .. } => "get_note_structure",
            Self::GetNoteOutline { .. } => "get_note_outline",
            Self::GetNoteStats { .. } => "get_note_stats",
            Self::ResolveRef { .. } => "resolve_ref",
            Self::GetOutlinks { .. } => "get_outlinks",
            Self::GetBacklinks { .. } => "get_backlinks",
            Self::ListTags { .. } => "list_tags",
            Self::GetTag { .. } => "get_tag",
            Self::ListCategories { .. } => "list_categories",
            Self::GetCategory { .. } => "get_category",
            Self::QueryFrontmatter { .. } => "query_frontmatter",
            Self::SearchText { .. } => "search_text",
            Self::SearchRegex { .. } => "search_regex",
            Self::AppendSection { .. } => "append_section",
            Self::ReplaceSection { .. } => "replace_section",
            Self::DeleteSection { .. } => "delete_section",
            Self::RenameHeading { .. } => "rename_heading",
            Self::RenameNote { .. } => "rename_note",
            Self::RenameBlockId { .. } => "rename_block_id",
        }
    }
    pub(crate) async fn run(self, vault: Vault) -> anyhow::Result<()> {
        let vault = Arc::new(vault);
        if let Command::GenerateDocs { check, output } = self {
            let content = crate::docs::render_docs(&crate::docs::all_tool_definitions())?;
            if check {
                crate::docs::check_tools_markdown(output, &content)?;
            } else {
                crate::docs::write_tools_markdown(output, &content)?;
            }
            return anyhow::Ok(());
        }

        let queries = crate::query::VaultQueries::new(vault.clone());
        let mutations = crate::mutation::VaultMutations::new(queries.clone());

        match self {
            Command::Serve => crate::server::run_mcp_server(vault).await?,
            Command::GenerateDocs { .. } => unreachable!("handled before opening vault"),
            Command::Doctor => {
                print_value(&queries.list_notes(&[], &[], 1)?)?;
            }
            Command::ListNotes {
                include,
                exclude,
                page,
            } => {
                print_value(&queries.list_notes(&include, &exclude, page)?)?;
            }
            Command::AuditLinks { page } => {
                print_value(&queries.audit_links(page)?)?;
            }
            Command::GetNoteNeighborhood {
                target,
                depth,
                direction,
            } => {
                print_value(&queries.get_note_neighborhood(
                    &target,
                    depth,
                    crate::query::NeighborhoodDirection::try_from(direction.as_str())?,
                )?)?;
            }
            Command::ReadNote {
                note,
                heading,
                block_id,
                line,
                max_chars,
            } => {
                let (note, selector) = crate::server::read_note_parts(
                    note.to_owned(),
                    heading.to_owned(),
                    block_id.clone(),
                    line.clone(),
                )
                .map_err(anyhow::Error::msg)?;
                print_value(&queries.read_note(&note, max_chars, selector)?)?;
            }
            Command::GetNoteStructure { note } => {
                print_value(&queries.get_note_structure(&note)?)?;
            }
            Command::GetNoteOutline { note, page } => {
                print_value(&queries.get_note_outline(&note, page)?)?;
            }
            Command::GetNoteStats { note } => {
                print_value(&queries.get_note_stats(&note)?)?;
            }
            Command::ResolveRef { reference } => {
                print_value(&queries.resolve_ref(&reference)?)?;
            }
            Command::GetOutlinks { note, page } => {
                print_value(&queries.get_outlinks(&note, page)?)?;
            }
            Command::GetBacklinks {
                target,
                include,
                exclude,
                page,
            } => {
                print_value(&queries.get_backlinks(&target, &include, &exclude, page)?)?;
            }
            Command::ListTags {
                scope,
                include,
                exclude,
                page,
            } => {
                print_value(&queries.list_tags(
                    crate::query::TagScope::try_from(scope.as_str())?,
                    &include,
                    &exclude,
                    page,
                )?)?;
            }
            Command::GetTag {
                tag,
                scope,
                include,
                exclude,
                page,
            } => {
                if tag.trim().trim_start_matches('#').is_empty() {
                    return Err(anyhow::anyhow!(
                        "provide a non-empty tag; use list_tags to discover tag names"
                    ));
                }
                print_value(&queries.get_tag(
                    &tag,
                    crate::query::TagScope::try_from(scope.as_str())?,
                    &include,
                    &exclude,
                    page,
                )?)?;
            }
            Command::ListCategories {
                include,
                exclude,
                page,
            } => {
                print_value(&queries.list_categories(&include, &exclude, page)?)?;
            }
            Command::GetCategory {
                category,
                include,
                exclude,
                page,
            } => {
                if category.trim().trim_matches('/').is_empty() {
                    return Err(anyhow::anyhow!(
                        "provide a non-empty category; use list_categories to discover category names"
                    ));
                }
                print_value(&queries.get_category(&category, &include, &exclude, page)?)?;
            }
            Command::QueryFrontmatter {
                field,
                mode,
                value,
                include,
                exclude,
                page,
            } => {
                print_value(&queries.query_frontmatter(
                    crate::query::FrontmatterQueryOptions {
                        field,
                        mode: crate::query::FrontmatterMatchMode::try_from(mode.as_str())?,
                        value,
                        include,
                        exclude,
                        page,
                    },
                )?)?;
            }
            Command::SearchText {
                query,
                case_sensitive,
                include,
                exclude,
                page,
            } => {
                print_value(&queries.search_text(
                    &query,
                    case_sensitive,
                    &include,
                    &exclude,
                    page,
                )?)?;
            }
            Command::SearchRegex {
                pattern,
                case_sensitive,
                include,
                exclude,
                page,
            } => {
                print_value(&queries.search_regex(
                    &pattern,
                    case_sensitive,
                    &include,
                    &exclude,
                    page,
                )?)?;
            }
            Command::AppendSection {
                note,
                heading,
                block_id,
                line,
                content,
            } => {
                let (note, selector) =
                    section_parts(note, heading, block_id, line).map_err(|_| {
                        anyhow::anyhow!(
                            "provide exactly one selector: --heading, --block-id, or --line"
                        )
                    })?;
                print_value(&mutations.append_section(&note, selector, &content)?)?
            }
            Command::ReplaceSection {
                note,
                heading,
                block_id,
                line,
                content,
            } => {
                let (note, selector) =
                    section_parts(note, heading, block_id, line).map_err(|_| {
                        anyhow::anyhow!(
                            "provide exactly one selector: --heading, --block-id, or --line"
                        )
                    })?;
                print_value(&mutations.replace_section(&note, selector, &content)?)?
            }
            Command::DeleteSection {
                note,
                heading,
                block_id,
                line,
            } => {
                let (note, selector) =
                    section_parts(note, heading, block_id, line).map_err(|_| {
                        anyhow::anyhow!(
                            "provide exactly one selector: --heading, --block-id, or --line"
                        )
                    })?;
                print_value(&mutations.delete_section(&note, selector)?)?
            }
            Command::RenameHeading {
                note,
                old_heading,
                new_heading,
                dry_run,
            } => print_value(&mutations.rename_heading(
                &note,
                &old_heading,
                &new_heading,
                dry_run,
            )?)?,
            Command::RenameNote {
                path,
                new_path,
                dry_run,
            } => print_value(&mutations.rename_note(&path, &new_path, dry_run)?)?,
            Command::RenameBlockId {
                note,
                old_block_id,
                new_block_id,
                dry_run,
            } => print_value(&mutations.rename_block_id(
                &note,
                &old_block_id,
                &new_block_id,
                dry_run,
            )?)?,
        }
        anyhow::Ok(())
    }
}

pub(crate) fn print_value<T: serde::Serialize>(value: &T) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
