use std::sync::Arc;

use crate::{server::section_parts, vault::Vault};
use camino::Utf8PathBuf;
use tracing::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;

#[derive(Debug, clap::Subcommand)]
pub(crate) enum Command {
    /// Run MCP server over stdio
    Serve,

    /// Run the independent Blueprint MCP service, or execute a Blueprint operation directly
    Blueprint {
        #[command(subcommand)]
        command: Option<BlueprintCommand>,
    },

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

    /// Return one note's word count, character count, and total backlink count
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
            Self::Blueprint { .. } => "blueprint",
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
            let content =
                crate::docs::render_docs(&crate::server::ObsidianVaultMcp::tool_definitions())?;
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
            Command::Blueprint { command } => {
                let service = crate::blueprint::BlueprintService::new(vault.root.clone());
                if let Some(command) = command {
                    let command_name = command.name();
                    let command_span =
                        tracing::info_span!("cli.blueprint_command", command = command_name);
                    tracing::info!(parent: &command_span, command = command_name, "cli.blueprint_command.start");
                    let result = command.run(&service).instrument(command_span.clone()).await;
                    match result {
                        Ok(()) => {
                            tracing::info!(parent: &command_span, command = command_name, "cli.blueprint_command.ok");
                        }
                        Err(error) => {
                            command_span.set_attribute("error.type", "command.error");
                            command_span.set_status(opentelemetry::trace::Status::error(
                                "blueprint_command failed",
                            ));
                            let error = crate::format_error_chain(&error);
                            tracing::error!(
                                parent: &command_span,
                                command = command_name,
                                error = %error,
                                "cli.blueprint_command.error"
                            );
                        }
                    }
                } else {
                    crate::blueprint::run_blueprint_mcp_server(service).await?
                }
            }
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
                .map_err(|_| {
                    anyhow::anyhow!(
                        "provide exactly one selector: --heading, --block-id, or --line"
                    )
                })?;
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

#[derive(Debug, clap::Subcommand)]
pub(crate) enum BlueprintCommand {
    /// Create a Blueprint.
    Create {
        /// Blueprint title.
        #[arg(long)]
        title: String,
        /// User who created the Blueprint.
        #[arg(long)]
        created_by: String,
        /// Blueprint intent.
        #[arg(long)]
        intent: String,
        /// Blueprint constraints.
        #[arg(long)]
        constraints: Vec<String>,
        /// Definition of done items.
        #[arg(long, required = true)]
        definition_of_done: Vec<String>,
        /// Blueprint plan.
        #[arg(long)]
        plan: String,
        /// Evaluation rubric for this Blueprint.
        #[arg(long)]
        rubric: String,
    },
    /// Get a Blueprint.
    Get {
        #[arg(long)]
        blueprint_id: String,
        #[arg(long)]
        view: Option<String>,
    },
    /// List Blueprints.
    List {
        #[arg(long, default_value = "active")]
        state: String,
    },
    /// Update a Blueprint.
    Update {
        #[arg(long)]
        blueprint_id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        intent: Option<String>,
        #[arg(long)]
        constraints: Option<Vec<String>>,
        #[arg(long)]
        plan: Option<String>,
        #[arg(long)]
        rubric: Option<String>,
        #[arg(long)]
        changed_by: Option<String>,
        #[arg(long)]
        change_reason: Option<String>,
        #[arg(long)]
        results: Option<String>,
        #[arg(long)]
        notes: Option<String>,
        #[arg(long)]
        expected_etag: Option<String>,
    },
    /// Get Blueprint status.
    Status {
        #[arg(long)]
        blueprint_id: String,
    },
    /// Close a Blueprint.
    Close {
        #[arg(long)]
        blueprint_id: String,
        #[arg(long)]
        closed_by: String,
        #[arg(long)]
        reason: Option<String>,
        #[arg(long)]
        expected_etag: Option<String>,
    },
    /// Cancel a Blueprint.
    Cancel {
        #[arg(long)]
        blueprint_id: String,
        #[arg(long)]
        cancelled_by: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        expected_etag: Option<String>,
    },
    /// Update a Definition of Done item.
    DodUpdate {
        #[arg(long)]
        blueprint_id: String,
        #[arg(long)]
        dod_id: String,
        #[arg(long, default_value_t = false, action = clap::ArgAction::Set)]
        completed: bool,
        #[arg(long)]
        note: Option<String>,
        #[arg(long)]
        expected_etag: Option<String>,
    },
    /// Create a todo.
    TodoCreate {
        #[arg(long)]
        blueprint_id: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        created_by: String,
        #[arg(long)]
        parent_id: Option<String>,
        #[arg(long)]
        owner: Option<String>,
        #[arg(long)]
        depends_on: Vec<String>,
        #[arg(long)]
        completion_criteria: Vec<String>,
        #[arg(long)]
        expected_etag: Option<String>,
    },
    /// Get a todo.
    TodoGet {
        #[arg(long)]
        blueprint_id: String,
        #[arg(long)]
        todo_id: String,
        #[arg(long)]
        expected_etag: Option<String>,
    },
    /// List todos.
    TodoList {
        #[arg(long)]
        blueprint_id: String,
        #[arg(long)]
        status: Option<crate::blueprint::TodoStatus>,
        #[arg(long)]
        owner: Option<String>,
        #[arg(long)]
        ready: Option<bool>,
    },
    /// Update a todo.
    TodoUpdate {
        #[arg(long)]
        blueprint_id: String,
        #[arg(long)]
        todo_id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        depends_on: Option<Vec<String>>,
        #[arg(long)]
        completion_criterion: Vec<String>,
        #[arg(long)]
        completed_criterion: Vec<String>,
        #[arg(long)]
        handoff: Option<Vec<String>>,
        #[arg(long)]
        result_summary: Option<String>,
        #[arg(long)]
        expected_etag: Option<String>,
    },
    /// Assign a todo.
    TodoAssign {
        #[arg(long)]
        blueprint_id: String,
        #[arg(long)]
        todo_id: String,
        #[arg(long)]
        owner: String,
        #[arg(long)]
        expected_etag: Option<String>,
    },
    /// Start a todo.
    TodoStart {
        #[arg(long)]
        blueprint_id: String,
        #[arg(long)]
        todo_id: String,
        #[arg(long)]
        expected_etag: Option<String>,
    },
    /// Complete a todo.
    TodoComplete {
        #[arg(long)]
        blueprint_id: String,
        #[arg(long)]
        todo_id: String,
        #[arg(long)]
        completed_by: String,
        #[arg(long)]
        summary: String,
        #[arg(long)]
        expected_etag: Option<String>,
    },
    /// Block a todo.
    TodoBlock {
        #[arg(long)]
        blueprint_id: String,
        #[arg(long)]
        todo_id: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        handoff: String,
        #[arg(long)]
        expected_etag: Option<String>,
    },
    /// Cancel a todo.
    TodoCancel {
        #[arg(long)]
        blueprint_id: String,
        #[arg(long)]
        todo_id: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        expected_etag: Option<String>,
    },
}

impl BlueprintCommand {
    fn name(&self) -> &'static str {
        match self {
            Self::Create { .. } => "create",
            Self::Get { .. } => "get",
            Self::List { .. } => "list",
            Self::Update { .. } => "update",
            Self::Status { .. } => "status",
            Self::Close { .. } => "close",
            Self::Cancel { .. } => "cancel",
            Self::DodUpdate { .. } => "dod_update",
            Self::TodoCreate { .. } => "todo_create",
            Self::TodoGet { .. } => "todo_get",
            Self::TodoList { .. } => "todo_list",
            Self::TodoUpdate { .. } => "todo_update",
            Self::TodoAssign { .. } => "todo_assign",
            Self::TodoStart { .. } => "todo_start",
            Self::TodoComplete { .. } => "todo_complete",
            Self::TodoBlock { .. } => "todo_block",
            Self::TodoCancel { .. } => "todo_cancel",
        }
    }
    async fn run(self, service: &crate::blueprint::BlueprintService) -> anyhow::Result<()> {
        match self {
            BlueprintCommand::Create {
                title,
                created_by,
                intent,
                constraints,
                definition_of_done,
                plan,
                rubric,
            } => {
                print_value(&service.blueprint_create(
                    crate::blueprint::BlueprintCreateInput {
                        title,
                        created_by,
                        intent,
                        constraints,
                        definition_of_done,
                        plan,
                        rubric,
                    },
                )?)?;
            }
            BlueprintCommand::Get { blueprint_id, view } => {
                print_value(&service.blueprint_view(&blueprint_id, view.as_deref())?)?;
            }
            BlueprintCommand::List { state } => {
                print_value(&service.blueprint_list(&state).map(|blueprint_ids| {
                    crate::blueprint::BlueprintListOutput { blueprint_ids }
                })?)?;
            }
            BlueprintCommand::Update {
                blueprint_id,
                title,
                intent,
                constraints,
                plan,
                rubric,
                changed_by,
                change_reason,
                results,
                notes,
                expected_etag,
            } => {
                print_value(&service.blueprint_update_semantic(
                    &blueprint_id,
                    crate::blueprint::BlueprintPatch {
                        title,
                        intent,
                        constraints,
                        plan,
                        rubric,
                        results,
                        notes,
                    },
                    changed_by.as_deref(),
                    change_reason.as_deref(),
                    expected_etag.as_deref(),
                )?)?;
            }
            BlueprintCommand::Status { blueprint_id } => {
                print_value(&service.blueprint_status(&blueprint_id)?)?;
            }
            BlueprintCommand::Close {
                blueprint_id,
                closed_by,
                reason,
                expected_etag,
            } => {
                print_value(&service.blueprint_close(
                    &blueprint_id,
                    &closed_by,
                    reason.as_deref(),
                    expected_etag.as_deref(),
                )?)?;
            }
            BlueprintCommand::Cancel {
                blueprint_id,
                cancelled_by,
                reason,
                expected_etag,
            } => {
                print_value(&service.blueprint_cancel(
                    &blueprint_id,
                    &cancelled_by,
                    &reason,
                    expected_etag.as_deref(),
                )?)?;
            }
            BlueprintCommand::DodUpdate {
                blueprint_id,
                dod_id,
                completed,
                note,
                expected_etag,
            } => {
                print_value(&service.dod_update(
                    &blueprint_id,
                    &dod_id,
                    completed,
                    note.as_deref(),
                    expected_etag.as_deref(),
                )?)?;
            }
            BlueprintCommand::TodoCreate {
                blueprint_id,
                title,
                created_by,
                parent_id,
                owner,
                depends_on,
                completion_criteria,
                expected_etag,
            } => {
                print_value(&service.todo_create_legacy(
                    &blueprint_id,
                    &title,
                    &created_by,
                    parent_id.as_deref(),
                    owner.as_deref(),
                    &depends_on,
                    &completion_criteria,
                    expected_etag.as_deref(),
                )?)?;
            }
            BlueprintCommand::TodoGet {
                blueprint_id,
                todo_id,
                ..
            } => {
                print_value(&service.todo_get(&blueprint_id, &todo_id)?)?;
            }
            BlueprintCommand::TodoList {
                blueprint_id,
                status,
                owner,
                ready,
            } => {
                print_value(
                    &service
                        .todo_list(&blueprint_id, status, owner.as_deref(), ready)
                        .map(|todos| crate::blueprint::TodoListOutput { todos })?,
                )?;
            }
            BlueprintCommand::TodoUpdate {
                blueprint_id,
                todo_id,
                title,
                depends_on,
                completion_criterion,
                completed_criterion,
                handoff,
                result_summary,
                expected_etag,
            } => {
                let criteria = (!completion_criterion.is_empty()
                    || !completed_criterion.is_empty())
                .then(|| {
                    completion_criterion
                        .iter()
                        .map(|text| crate::blueprint::CheckUpdate {
                            text: text.clone(),
                            completed: false,
                        })
                        .chain(completed_criterion.iter().map(|text| {
                            crate::blueprint::CheckUpdate {
                                text: text.clone(),
                                completed: true,
                            }
                        }))
                        .collect::<Vec<_>>()
                });
                print_value(&service.todo_update_legacy(
                    &blueprint_id,
                    &todo_id,
                    title.as_deref(),
                    depends_on.as_deref(),
                    criteria.as_deref(),
                    handoff.as_deref(),
                    result_summary.as_deref(),
                    expected_etag.as_deref(),
                )?)?;
            }
            BlueprintCommand::TodoAssign {
                blueprint_id,
                todo_id,
                owner,
                expected_etag,
            } => {
                print_value(&service.todo_assign(
                    &blueprint_id,
                    &todo_id,
                    &owner,
                    expected_etag.as_deref(),
                )?)?;
            }
            BlueprintCommand::TodoStart {
                blueprint_id,
                todo_id,
                expected_etag,
            } => {
                print_value(&service.todo_start(
                    &blueprint_id,
                    &todo_id,
                    expected_etag.as_deref(),
                )?)?;
            }
            BlueprintCommand::TodoComplete {
                blueprint_id,
                todo_id,
                completed_by,
                summary,
                expected_etag,
            } => {
                print_value(&service.todo_complete(
                    &blueprint_id,
                    &todo_id,
                    &completed_by,
                    &summary,
                    expected_etag.as_deref(),
                )?)?;
            }
            BlueprintCommand::TodoBlock {
                blueprint_id,
                todo_id,
                reason,
                handoff,
                expected_etag,
            } => {
                print_value(&service.todo_block(
                    &blueprint_id,
                    &todo_id,
                    &reason,
                    &handoff,
                    expected_etag.as_deref(),
                )?)?;
            }
            BlueprintCommand::TodoCancel {
                blueprint_id,
                todo_id,
                reason,
                expected_etag,
            } => {
                print_value(&service.todo_cancel(
                    &blueprint_id,
                    &todo_id,
                    &reason,
                    expected_etag.as_deref(),
                )?)?;
            }
        }
        anyhow::Ok(())
    }
}

pub(crate) fn print_value<T: serde::Serialize>(value: &T) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
