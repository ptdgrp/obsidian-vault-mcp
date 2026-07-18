use camino::Utf8PathBuf;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::blueprint::{
    model::{
        BlueprintGetOutput, BlueprintResumeOutput, BlueprintState, NotReadyTodo, Todo, TodoStatus,
    },
    source::ParsedBlueprintSource,
    store::{BlueprintStore, StoredBlueprint},
    validate::derive_readiness,
};

pub use crate::blueprint::model::BlueprintCreateInput as BlueprintCreateRequest;

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct BlueprintCreated {
    pub id: String,
    #[schemars(with = "String")]
    pub path: Utf8PathBuf,
    pub etag: String,
    pub source: String,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct BlueprintStatus {
    pub state: String,
    pub ready_todos: Vec<String>,
    pub not_ready_todos: Vec<NotReadyTodo>,
    pub blocked_todos: Vec<String>,
    pub unassigned_todos: Vec<String>,
    pub open_todos: Vec<String>,
    pub open_definition_of_done: Vec<String>,
    pub todos: Vec<Todo>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CheckUpdate {
    pub text: String,
    #[serde(default)]
    pub completed: bool,
}

#[derive(Clone, Debug)]
pub struct BlueprintService {
    store: BlueprintStore,
}

impl BlueprintService {
    pub fn new(vault_root: Utf8PathBuf) -> Self {
        Self {
            store: BlueprintStore::new(vault_root),
        }
    }

    #[tracing::instrument(
        name = "blueprint.create",
        skip_all,
        fields(operation.kind = "mutation", operation.name = "create"),
        err
    )]
    pub fn blueprint_create(
        &self,
        request: BlueprintCreateRequest,
    ) -> anyhow::Result<BlueprintCreated> {
        require_text("title", &request.title)?;
        require_text("created_by", &request.created_by)?;
        require_text("intent", &request.intent)?;
        require_text("plan", &request.plan)?;
        require_text("rubric", &request.rubric)?;
        if request.definition_of_done.is_empty() {
            anyhow::bail!("definition_of_done must not be empty");
        }
        let id = format!("bp-{}", Ulid::new());
        let source = render_blueprint(&id, &request);
        let StoredBlueprint {
            id, etag, source, ..
        } = self.store.create(&id, &source)?;
        let path = self.store.read(&id)?.path;
        Ok(BlueprintCreated {
            id,
            path,
            etag,
            source,
        })
    }

    #[tracing::instrument(
        name = "blueprint.list",
        skip_all,
        fields(operation.kind = "query", operation.name = "list"),
        err
    )]
    pub fn blueprint_list(&self, state: &str) -> anyhow::Result<Vec<String>> {
        self.store.list(state)
    }

    pub fn blueprint_get(&self, id: &str) -> anyhow::Result<StoredBlueprint> {
        self.store.read(id)
    }
    #[tracing::instrument(
        name = "blueprint.view",
        skip_all,
        fields(operation.kind = "query", operation.name = "view"),
        err
    )]
    pub fn blueprint_view(
        &self,
        id: &str,
        view: Option<&str>,
    ) -> anyhow::Result<BlueprintGetOutput> {
        let stored = self.blueprint_get(id)?;
        match view.unwrap_or("full") {
            "full" => Ok(BlueprintGetOutput {
                id: stored.id,
                state: blueprint_state_name(stored.state).to_string(),
                etag: stored.etag,
                source: Some(stored.source),
                resume: None,
            }),
            "resume" => {
                let status = self.blueprint_status(id)?;
                let active_todos = status
                    .todos
                    .iter()
                    .filter(|todo| {
                        matches!(todo.status, TodoStatus::InProgress | TodoStatus::Blocked)
                            || status.ready_todos.iter().any(|ready| ready == &todo.id)
                    })
                    .cloned()
                    .collect();
                Ok(BlueprintGetOutput {
                    id: stored.id,
                    state: blueprint_state_name(stored.state).to_string(),
                    etag: stored.etag,
                    source: None,
                    resume: Some(BlueprintResumeOutput {
                        intent: section_body(&stored.source, "Intent"),
                        constraints: section_body(&stored.source, "Constraints"),
                        plan: section_body(&stored.source, "Plan"),
                        results: section_body(&stored.source, "Results"),
                        open_definition_of_done: status.open_definition_of_done,
                        active_todos,
                        ready_todos: status.ready_todos,
                        not_ready_todos: status.not_ready_todos,
                    }),
                })
            }
            other => anyhow::bail!("view must be full or resume, got: {other}"),
        }
    }
    #[tracing::instrument(
        name = "blueprint.status",
        skip_all,
        fields(operation.kind = "query", operation.name = "status"),
        err
    )]
    pub fn blueprint_status(&self, id: &str) -> anyhow::Result<BlueprintStatus> {
        let stored = self.store.read(id)?;
        let parsed = ParsedBlueprintSource::parse(&format!("{id}.md"), &stored.source)?;
        let readiness = derive_readiness(&parsed.todos)?;
        let flattened = flatten_todos(&parsed.todos);
        Ok(BlueprintStatus {
            state: blueprint_state_name(stored.state).to_string(),
            ready_todos: readiness.ready,
            not_ready_todos: readiness.not_ready,
            blocked_todos: flattened
                .iter()
                .filter(|todo| todo.status == TodoStatus::Blocked)
                .map(|todo| todo.id.clone())
                .collect(),
            unassigned_todos: flattened
                .iter()
                .filter(|todo| {
                    todo.status == TodoStatus::Pending
                        && todo.owner.as_deref().is_none_or(str::is_empty)
                })
                .map(|todo| todo.id.clone())
                .collect(),
            open_todos: flattened
                .iter()
                .filter(|todo| {
                    !matches!(todo.status, TodoStatus::Completed | TodoStatus::Cancelled)
                })
                .map(|todo| todo.id.clone())
                .collect(),
            open_definition_of_done: open_dod_ids(&stored.source),
            todos: parsed.todos,
        })
    }

    #[allow(clippy::too_many_arguments)]
    #[tracing::instrument(
        name = "blueprint.update",
        skip_all,
        fields(operation.kind = "mutation", operation.name = "update"),
        err
    )]
    pub fn blueprint_update(
        &self,
        id: &str,
        title: Option<&str>,
        intent: Option<&str>,
        constraints: Option<&[String]>,
        plan: Option<&str>,
        results: Option<&str>,
        notes: Option<&str>,
        expected_etag: Option<&str>,
    ) -> anyhow::Result<StoredBlueprint> {
        self.store.write_blueprint(id, expected_etag, |source| {
            let mut next = source.to_string();
            if let Some(title) = title {
                require_text("title", title)?;
                next = replace_title(&next, title)?;
            }
            if let Some(intent) = intent {
                require_text("intent", intent)?;
                next = replace_section(&next, "Intent", intent)?;
            }
            if let Some(constraints) = constraints {
                next = replace_section(
                    &next,
                    "Constraints",
                    &constraints
                        .iter()
                        .map(|v| format!("- {}", v.trim()))
                        .collect::<Vec<_>>()
                        .join("\n"),
                )?;
            }
            if let Some(plan) = plan {
                require_text("plan", plan)?;
                next = replace_section(&next, "Plan", plan)?;
            }
            if let Some(results) = results {
                next = replace_section(&next, "Results", results)?;
            }
            if let Some(notes) = notes {
                next = replace_section(&next, "Notes", notes)?;
            }
            ParsedBlueprintSource::parse(&format!("{id}.md"), &next)?;
            Ok(next)
        })
    }

    /// V2 section patch API. Semantic fields are recorded in the append-only revision history.
    pub fn blueprint_update_semantic(
        &self,
        id: &str,
        patch: crate::blueprint::model::BlueprintPatch,
        changed_by: Option<&str>,
        change_reason: Option<&str>,
        expected_etag: Option<&str>,
    ) -> anyhow::Result<StoredBlueprint> {
        let semantic = patch.intent.is_some()
            || patch.constraints.is_some()
            || patch.plan.is_some()
            || patch.rubric.is_some();
        if semantic {
            require_text("changed_by", changed_by.unwrap_or(""))?;
            require_text("change_reason", change_reason.unwrap_or(""))?;
        }
        self.store.write_blueprint(id, expected_etag, |source| {
            let mut next = source.to_string();
            if let Some(title) = patch.title.as_deref() {
                require_text("title", title)?;
                next = replace_title(&next, title)?;
            }
            if let Some(intent) = patch.intent.as_deref() {
                require_text("intent", intent)?;
                next = replace_section(&next, "Intent", intent)?;
            }
            if let Some(constraints) = patch.constraints.as_deref() {
                next = replace_section(
                    &next,
                    "Constraints",
                    &constraints
                        .iter()
                        .map(|item| format!("- {}", item.trim()))
                        .collect::<Vec<_>>()
                        .join("\n"),
                )?;
            }
            if let Some(plan) = patch.plan.as_deref() {
                require_text("plan", plan)?;
                next = replace_section(&next, "Plan", plan)?;
            }
            if let Some(rubric) = patch.rubric.as_deref() {
                require_text("rubric", rubric)?;
                next = replace_section(&next, "Rubric", rubric)?;
            }
            if let Some(results) = patch.results.as_deref() {
                next = replace_section(&next, "Results", results)?;
            }
            if let Some(notes) = patch.notes.as_deref() {
                next = replace_section(&next, "Notes", notes)?;
            }
            if semantic {
                let sections = [
                    patch.intent.as_ref().map(|_| "Intent"),
                    patch.constraints.as_ref().map(|_| "Constraints"),
                    patch.plan.as_ref().map(|_| "Plan"),
                    patch.rubric.as_ref().map(|_| "Rubric"),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(", ");
                let revision = format!(
                    "### Blueprint update ^revision-{}\n\n- Changed By: {}\n- Reason: {}\n- Changed Sections: {}\n",
                    Ulid::new(),
                    changed_by.expect("validated").trim(),
                    change_reason.expect("validated").trim(),
                    sections,
                );
                let history = section_body(&next, "Revision History");
                next = replace_section(
                    &next,
                    "Revision History",
                    &format!("{history}\n\n{revision}"),
                )?;
            }
            crate::blueprint::BlueprintSource::parse(&format!("{id}.md"), &next)?;
            Ok(next)
        })
    }

    #[allow(clippy::too_many_arguments)]
    #[tracing::instrument(
        name = "blueprint.todo_create",
        skip_all,
        fields(operation.kind = "mutation", operation.name = "todo_create"),
        err
    )]
    pub fn todo_create(
        &self,
        blueprint_id: &str,
        title: &str,
        created_by: &str,
        parent_id: Option<&str>,
        owner: Option<&str>,
        depends_on: &[String],
        completion_criteria: &[String],
        expected_etag: Option<&str>,
    ) -> anyhow::Result<Todo> {
        require_text("title", title)?;
        require_text("created_by", created_by)?;
        if let Some(parent_id) = parent_id {
            self.todo_get(blueprint_id, parent_id)?;
        }
        let todo_id = format!("todo-{}", Ulid::new());
        let mut block = format!(
            "- [ ] [{}](todos/{}.md) ^{}\n  - Created By: {}\n",
            title.trim(),
            todo_id,
            todo_id,
            created_by.trim()
        );
        if let Some(owner) = owner.filter(|v| !v.trim().is_empty()) {
            block.push_str(&format!("  - Owner: {}\n", owner.trim()));
        }
        if !depends_on.is_empty() {
            block.push_str(&format!("  - Depends On: {}\n", depends_on.join(", ")));
        }
        for criterion in completion_criteria {
            require_text("completion criterion", criterion)?;
            block.push_str("  - Completion Criteria:\n");
            break;
        }
        for criterion in completion_criteria {
            block.push_str(&format!("    - [ ] {}\n", criterion.trim()));
        }
        self.store.create_todo(
            blueprint_id,
            &todo_id,
            &render_todo(blueprint_id, &todo_id, title, completion_criteria),
        )?;
        self.store
            .write_blueprint(blueprint_id, expected_etag, |source| {
                let source = if let Some(parent_id) = parent_id {
                    ensure_children_label(source, parent_id)?
                } else {
                    source.to_string()
                };
                let insertion = if let Some(parent_id) = parent_id {
                    todo_children_insertion(&source, parent_id)?
                } else {
                    section_insertion(&source, "Todos")?
                };
                let indent = if parent_id.is_some() { "    " } else { "" };
                let block = block
                    .lines()
                    .map(|line| format!("{indent}{line}\n"))
                    .collect::<String>();
                let next = format!("{}{}{}", &source[..insertion], block, &source[insertion..]);
                let parsed = ParsedBlueprintSource::parse(&format!("{blueprint_id}.md"), &next)?;
                derive_readiness(&parsed.todos)?;
                Ok(next)
            })?;
        todo_from_active_source(&self.store, blueprint_id, &todo_id)
    }
    #[tracing::instrument(
        name = "blueprint.todo_start",
        skip_all,
        fields(operation.kind = "mutation", operation.name = "todo_start"),
        err
    )]
    pub fn todo_start(
        &self,
        blueprint_id: &str,
        todo_id: &str,
        expected_etag: Option<&str>,
    ) -> anyhow::Result<Todo> {
        let stored = self.store.read(blueprint_id)?;
        let parsed = ParsedBlueprintSource::parse(&format!("{blueprint_id}.md"), &stored.source)?;
        let todo = find_todo(&parsed.todos, todo_id)
            .ok_or_else(|| anyhow::anyhow!("unknown Todo: {todo_id}"))?;
        if todo.status != TodoStatus::Pending {
            anyhow::bail!("only pending Todo can be started");
        }
        if todo.owner.as_deref().is_none_or(str::is_empty) {
            anyhow::bail!("Todo must have an Owner before starting");
        }
        if !derive_readiness(&parsed.todos)?
            .ready
            .iter()
            .any(|id| id == todo_id)
        {
            anyhow::bail!("Todo is not ready to start");
        }
        let stored = self
            .store
            .write_blueprint(blueprint_id, expected_etag, |source| {
                let parsed = ParsedBlueprintSource::parse(&format!("{blueprint_id}.md"), source)?;
                let todo = find_todo(&parsed.todos, todo_id)
                    .ok_or_else(|| anyhow::anyhow!("unknown Todo: {todo_id}"))?;
                if todo.status != TodoStatus::Pending
                    || todo.owner.as_deref().is_none_or(str::is_empty)
                    || !derive_readiness(&parsed.todos)?
                        .ready
                        .iter()
                        .any(|id| id == todo_id)
                {
                    anyhow::bail!("Todo is not eligible to start");
                }
                replace_task_status(source, todo_id, TodoStatus::InProgress)
            })?;
        todo_from_source(blueprint_id, &stored.source, todo_id)
    }
    #[tracing::instrument(
        name = "blueprint.todo_get",
        skip_all,
        fields(operation.kind = "query", operation.name = "todo_get"),
        err
    )]
    pub fn todo_get(&self, blueprint_id: &str, todo_id: &str) -> anyhow::Result<Todo> {
        let stored = self.store.read(blueprint_id)?;
        todo_from_source(blueprint_id, &stored.source, todo_id)
    }

    #[tracing::instrument(
        name = "blueprint.todo_list",
        skip_all,
        fields(operation.kind = "query", operation.name = "todo_list"),
        err
    )]
    pub fn todo_list(
        &self,
        blueprint_id: &str,
        status: Option<TodoStatus>,
        owner: Option<&str>,
        ready: Option<bool>,
    ) -> anyhow::Result<Vec<Todo>> {
        let ready_todos = self.blueprint_status(blueprint_id)?.ready_todos;
        let mut todos = flatten_todos(&self.blueprint_status(blueprint_id)?.todos)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        todos.retain(|todo| {
            status.is_none_or(|value| todo.status == value)
                && owner.is_none_or(|value| todo.owner.as_deref() == Some(value))
                && ready.is_none_or(|value| ready_todos.iter().any(|id| id == &todo.id) == value)
        });
        Ok(todos)
    }
    #[tracing::instrument(
        name = "blueprint.todo_assign",
        skip_all,
        fields(operation.kind = "mutation", operation.name = "todo_assign"),
        err
    )]
    pub fn todo_assign(
        &self,
        blueprint_id: &str,
        todo_id: &str,
        owner: &str,
        expected_etag: Option<&str>,
    ) -> anyhow::Result<Todo> {
        require_text("owner", owner)?;
        let current = self.todo_get(blueprint_id, todo_id)?;
        if matches!(
            current.status,
            crate::blueprint::model::TodoStatus::Completed
                | crate::blueprint::model::TodoStatus::Cancelled
        ) {
            anyhow::bail!("completed or cancelled Todo cannot be assigned");
        }
        if matches!(
            current.status,
            crate::blueprint::model::TodoStatus::InProgress
                | crate::blueprint::model::TodoStatus::Blocked
        ) && current.owner.as_deref() != Some(owner.trim())
            && current.handoff.is_empty()
        {
            anyhow::bail!("in_progress or blocked Todo requires Handoff before reassignment");
        }
        let stored = self
            .store
            .write_blueprint(blueprint_id, expected_etag, |source| {
                let parsed = ParsedBlueprintSource::parse(&format!("{blueprint_id}.md"), source)?;
                let current = find_todo(&parsed.todos, todo_id)
                    .ok_or_else(|| anyhow::anyhow!("unknown Todo: {todo_id}"))?;
                if matches!(
                    current.status,
                    TodoStatus::Completed | TodoStatus::Cancelled
                ) || (matches!(current.status, TodoStatus::InProgress | TodoStatus::Blocked)
                    && current.owner.as_deref() != Some(owner.trim())
                    && current.handoff.is_empty())
                {
                    anyhow::bail!("Todo cannot be assigned in its current state");
                }
                replace_or_insert_todo_field(source, todo_id, "Owner", owner.trim())
            })?;
        todo_from_source(blueprint_id, &stored.source, todo_id)
    }
    #[tracing::instrument(
        name = "blueprint.todo_block",
        skip_all,
        fields(operation.kind = "mutation", operation.name = "todo_block"),
        err
    )]
    pub fn todo_block(
        &self,
        blueprint_id: &str,
        todo_id: &str,
        reason: &str,
        handoff: &str,
        expected_etag: Option<&str>,
    ) -> anyhow::Result<Todo> {
        require_text("reason", reason)?;
        require_text("handoff", handoff)?;
        let current = self.todo_get(blueprint_id, todo_id)?;
        if current.status != crate::blueprint::model::TodoStatus::InProgress {
            anyhow::bail!("only in_progress Todo can be blocked");
        }
        let stored = self
            .store
            .write_blueprint(blueprint_id, expected_etag, |source| {
                let parsed = ParsedBlueprintSource::parse(&format!("{blueprint_id}.md"), source)?;
                if find_todo(&parsed.todos, todo_id)
                    .ok_or_else(|| anyhow::anyhow!("unknown Todo: {todo_id}"))?
                    .status
                    != TodoStatus::InProgress
                {
                    anyhow::bail!("only in_progress Todo can be blocked");
                }
                let source = replace_task_status(source, todo_id, TodoStatus::Blocked)?;
                let source =
                    replace_or_insert_todo_field(&source, todo_id, "Block Reason", reason.trim())?;
                replace_or_insert_todo_field(&source, todo_id, "Handoff", handoff.trim())
            })?;
        todo_from_source(blueprint_id, &stored.source, todo_id)
    }
    #[tracing::instrument(
        name = "blueprint.todo_cancel",
        skip_all,
        fields(operation.kind = "mutation", operation.name = "todo_cancel"),
        err
    )]
    pub fn todo_cancel(
        &self,
        blueprint_id: &str,
        todo_id: &str,
        reason: &str,
        expected_etag: Option<&str>,
    ) -> anyhow::Result<Todo> {
        require_text("reason", reason)?;
        let current = self.todo_get(blueprint_id, todo_id)?;
        if !matches!(
            current.status,
            crate::blueprint::model::TodoStatus::Pending
                | crate::blueprint::model::TodoStatus::InProgress
                | crate::blueprint::model::TodoStatus::Blocked
        ) {
            anyhow::bail!("Todo cannot be cancelled from its current status");
        }
        let stored = self
            .store
            .write_blueprint(blueprint_id, expected_etag, |source| {
                let parsed = ParsedBlueprintSource::parse(&format!("{blueprint_id}.md"), source)?;
                let status = find_todo(&parsed.todos, todo_id)
                    .ok_or_else(|| anyhow::anyhow!("unknown Todo: {todo_id}"))?
                    .status;
                if !matches!(
                    status,
                    TodoStatus::Pending | TodoStatus::InProgress | TodoStatus::Blocked
                ) {
                    anyhow::bail!("Todo cannot be cancelled from its current status");
                }
                let source = replace_task_status(source, todo_id, TodoStatus::Cancelled)?;
                replace_or_insert_todo_field(&source, todo_id, "Cancel Reason", reason.trim())
            })?;
        todo_from_source(blueprint_id, &stored.source, todo_id)
    }
    #[tracing::instrument(
        name = "blueprint.todo_complete",
        skip_all,
        fields(operation.kind = "mutation", operation.name = "todo_complete"),
        err
    )]
    pub fn todo_complete(
        &self,
        blueprint_id: &str,
        todo_id: &str,
        completed_by: &str,
        summary: &str,
        expected_etag: Option<&str>,
    ) -> anyhow::Result<Todo> {
        require_text("completed_by", completed_by)?;
        require_text("summary", summary)?;
        let current = self.todo_get(blueprint_id, todo_id)?;
        if current.status != crate::blueprint::model::TodoStatus::InProgress {
            anyhow::bail!("only in_progress Todo can be completed");
        }
        if current
            .completion_criteria
            .iter()
            .any(|item| !item.completed)
        {
            anyhow::bail!("all Completion Criteria must be completed");
        }
        if current.children.iter().any(|child| {
            child.status != crate::blueprint::model::TodoStatus::Completed
                && child.status != crate::blueprint::model::TodoStatus::Cancelled
        }) {
            anyhow::bail!("all non-cancelled child Todos must be completed");
        }
        let stored = self
            .store
            .write_blueprint(blueprint_id, expected_etag, |source| {
                let parsed = ParsedBlueprintSource::parse(&format!("{blueprint_id}.md"), source)?;
                let current = find_todo(&parsed.todos, todo_id)
                    .ok_or_else(|| anyhow::anyhow!("unknown Todo: {todo_id}"))?;
                if current.status != TodoStatus::InProgress
                    || current
                        .completion_criteria
                        .iter()
                        .any(|item| !item.completed)
                    || current.children.iter().any(|child| {
                        !matches!(child.status, TodoStatus::Completed | TodoStatus::Cancelled)
                    })
                {
                    anyhow::bail!("Todo is not eligible to complete");
                }
                let source = replace_task_status(source, todo_id, TodoStatus::Completed)?;
                let source = replace_or_insert_todo_field(
                    &source,
                    todo_id,
                    "Completed By",
                    completed_by.trim(),
                )?;
                replace_or_insert_todo_field(&source, todo_id, "Result Summary", summary.trim())
            })?;
        todo_from_source(blueprint_id, &stored.source, todo_id)
    }

    #[allow(clippy::too_many_arguments)]
    #[tracing::instrument(
        name = "blueprint.todo_update",
        skip_all,
        fields(operation.kind = "mutation", operation.name = "todo_update"),
        err
    )]
    pub fn todo_update(
        &self,
        blueprint_id: &str,
        todo_id: &str,
        title: Option<&str>,
        depends_on: Option<&[String]>,
        completion_criteria: Option<&[CheckUpdate]>,
        handoff: Option<&[String]>,
        result_summary: Option<&str>,
        expected_etag: Option<&str>,
    ) -> anyhow::Result<Todo> {
        let stored = self
            .store
            .write_blueprint(blueprint_id, expected_etag, |source| {
                let mut next = source.to_string();
                if let Some(title) = title {
                    require_text("title", title)?;
                    next = replace_task_title(&next, todo_id, title)?;
                }
                if let Some(depends_on) = depends_on {
                    next = replace_or_insert_todo_field(
                        &next,
                        todo_id,
                        "Depends On",
                        &depends_on.join(", "),
                    )?;
                }
                if let Some(handoff) = handoff {
                    next = replace_repeated_todo_field(&next, todo_id, "Handoff", handoff)?;
                }
                if let Some(summary) = result_summary {
                    next = replace_or_insert_todo_field(
                        &next,
                        todo_id,
                        "Result Summary",
                        summary.trim(),
                    )?;
                }
                if let Some(criteria) = completion_criteria {
                    next = replace_completion_criteria(&next, todo_id, criteria)?;
                }
                let parsed = ParsedBlueprintSource::parse(&format!("{blueprint_id}.md"), &next)?;
                derive_readiness(&parsed.todos)?;
                Ok(next)
            })?;
        todo_from_source(blueprint_id, &stored.source, todo_id)
    }

    #[tracing::instrument(
        name = "blueprint.dod_update",
        skip_all,
        fields(operation.kind = "mutation", operation.name = "dod_update"),
        err
    )]
    pub fn dod_update(
        &self,
        blueprint_id: &str,
        dod_id: &str,
        completed: bool,
        note: Option<&str>,
        expected_etag: Option<&str>,
    ) -> anyhow::Result<StoredBlueprint> {
        self.store
            .write_blueprint(blueprint_id, expected_etag, |source| {
                let is_dod = source
                    .lines()
                    .any(|line| line.contains(&format!("^{dod_id}")) && line.contains("^dod-"));
                if !is_dod {
                    anyhow::bail!("unknown Definition of Done: {dod_id}");
                }
                let next = replace_task_status(
                    source,
                    dod_id,
                    if completed {
                        TodoStatus::Completed
                    } else {
                        TodoStatus::Pending
                    },
                )?;
                match note.filter(|value| !value.trim().is_empty()) {
                    Some(note) => replace_or_insert_todo_field(&next, dod_id, "Note", note.trim()),
                    None => Ok(next),
                }
            })
    }
    #[tracing::instrument(
        name = "blueprint.close",
        skip_all,
        fields(operation.kind = "mutation", operation.name = "close"),
        err
    )]
    pub fn blueprint_close(
        &self,
        blueprint_id: &str,
        closed_by: &str,
        reason: Option<&str>,
        expected_etag: Option<&str>,
    ) -> anyhow::Result<StoredBlueprint> {
        require_text("closed_by", closed_by)?;
        let before = self.store.read(blueprint_id)?;
        let parsed = ParsedBlueprintSource::parse(&format!("{blueprint_id}.md"), &before.source)?;
        let open_todos = flatten_todos(&parsed.todos)
            .into_iter()
            .filter(|todo| !matches!(todo.status, TodoStatus::Completed | TodoStatus::Cancelled))
            .map(|todo| todo.id.clone())
            .collect::<Vec<_>>();
        let open_dod = open_dod_ids(&before.source);
        if (!open_todos.is_empty() || !open_dod.is_empty())
            && reason.is_none_or(|value| value.trim().is_empty())
        {
            anyhow::bail!("reason is required when closing incomplete Blueprint");
        }
        let stored = self
            .store
            .write_blueprint(blueprint_id, expected_etag, |source| {
                let outcome = if !open_todos.is_empty() || !open_dod.is_empty() {
                    "incomplete"
                } else {
                    "complete"
                };
                let mut next =
                    replace_or_insert_record_field(source, "Closed By", closed_by.trim())?;
                let mut closure = format!(
                    "### Closure\n\n- Outcome: {outcome}\n- Closed By: {}\n",
                    closed_by.trim()
                );
                if let Some(reason) = reason.filter(|value| !value.trim().is_empty()) {
                    closure.push_str(&format!("- Reason: {}\n", reason.trim()));
                }
                if !open_dod.is_empty() {
                    closure.push_str("- Open Definition of Done:\n");
                    for id in &open_dod {
                        closure.push_str(&format!("  - {id}\n"));
                    }
                }
                if !open_todos.is_empty() {
                    closure.push_str("- Open Todos:\n");
                    for id in &open_todos {
                        closure.push_str(&format!("  - {id}\n"));
                    }
                }
                let results = section_body(&next, "Results");
                next = replace_section(&next, "Results", &format!("{results}\n\n{closure}"))?;
                Ok(next)
            })?;
        let stored =
            self.store
                .set_state(blueprint_id, BlueprintState::Closed, Some(&stored.etag))?;
        Ok(stored)
    }
    #[tracing::instrument(
        name = "blueprint.cancel",
        skip_all,
        fields(operation.kind = "mutation", operation.name = "cancel"),
        err
    )]
    pub fn blueprint_cancel(
        &self,
        blueprint_id: &str,
        cancelled_by: &str,
        reason: &str,
        expected_etag: Option<&str>,
    ) -> anyhow::Result<StoredBlueprint> {
        require_text("cancelled_by", cancelled_by)?;
        require_text("reason", reason)?;
        let stored = self
            .store
            .write_blueprint(blueprint_id, expected_etag, |source| {
                let next =
                    replace_or_insert_record_field(source, "Cancelled By", cancelled_by.trim())?;
                let results = section_body(&next, "Results");
                replace_section(
                    &next,
                    "Results",
                    &format!(
                        "{results}\n\n### Cancellation\n\n- Cancelled By: {}\n- Reason: {}\n",
                        cancelled_by.trim(),
                        reason.trim()
                    ),
                )
            })?;
        let stored =
            self.store
                .set_state(blueprint_id, BlueprintState::Cancelled, Some(&stored.etag))?;
        Ok(stored)
    }
}

fn todo_from_source(blueprint_id: &str, source: &str, todo_id: &str) -> anyhow::Result<Todo> {
    let parsed = ParsedBlueprintSource::parse(&format!("{blueprint_id}.md"), source)?;
    find_todo(&parsed.todos, todo_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Todo was not found after write: {todo_id}"))
}

fn todo_from_active_source(
    store: &BlueprintStore,
    blueprint_id: &str,
    todo_id: &str,
) -> anyhow::Result<Todo> {
    let stored = store.read(blueprint_id)?;
    todo_from_source(blueprint_id, &stored.source, todo_id)
}

fn flatten_todos(todos: &[Todo]) -> Vec<&Todo> {
    let mut all = Vec::new();
    for todo in todos {
        all.push(todo);
        all.extend(flatten_todos(&todo.children));
    }
    all
}

fn open_dod_ids(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| {
            (line.contains("^dod-") && !line.contains("- [x]") && !line.contains("- [X]"))
                .then(|| line.split('^').nth(1).map(str::trim).map(ToOwned::to_owned))
                .flatten()
        })
        .collect()
}

fn section_body(source: &str, section: &str) -> String {
    let heading = format!("## {section}");
    let Some(heading_start) = source
        .lines()
        .find(|line| **line == heading)
        .map(|line| line.as_ptr() as usize - source.as_ptr() as usize)
    else {
        return String::new();
    };
    let start = source[heading_start + heading.len()..]
        .find('\n')
        .map(|offset| heading_start + heading.len() + offset + 1)
        .unwrap_or(source.len());
    let end = source[start..]
        .find("\n## ")
        .map(|offset| start + offset)
        .unwrap_or(source.len());
    source[start..end].trim().to_string()
}

fn section_bounds(source: &str, section: &str) -> anyhow::Result<(usize, usize)> {
    let heading = format!("## {section}");
    let start = source
        .lines()
        .find(|line| **line == heading)
        .map(|line| line.as_ptr() as usize - source.as_ptr() as usize)
        .ok_or_else(|| anyhow::anyhow!("missing required section: {section}"))?;
    let body = source[start + heading.len()..]
        .find('\n')
        .map(|offset| start + heading.len() + offset + 1)
        .unwrap_or(source.len());
    let end = source[body..]
        .find("\n## ")
        .map(|offset| body + offset + 1)
        .unwrap_or(source.len());
    Ok((body, end))
}

fn section_insertion(source: &str, section: &str) -> anyhow::Result<usize> {
    let (start, _) = section_bounds(source, section)?;
    Ok(start)
}

fn replace_section(source: &str, section: &str, content: &str) -> anyhow::Result<String> {
    let (start, end) = section_bounds(source, section)?;
    let content = content.trim_end();
    Ok(format!(
        "{}\n{}\n{}",
        &source[..start],
        content,
        &source[end..]
    ))
}

fn append_to_section(source: &str, section: &str, content: &str) -> anyhow::Result<String> {
    let (_, end) = section_bounds(source, section)?;
    let prefix = if source[..end].ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    Ok(format!(
        "{}{}{}{}",
        &source[..end],
        prefix,
        content.trim_end(),
        &source[end..]
    ))
}

fn replace_title(source: &str, title: &str) -> anyhow::Result<String> {
    let start = source
        .lines()
        .find(|line| line.starts_with("# "))
        .map(|line| line.as_ptr() as usize - source.as_ptr() as usize)
        .ok_or_else(|| anyhow::anyhow!("Blueprint title must be H1"))?;
    let end = source[start..]
        .find('\n')
        .map(|offset| start + offset)
        .unwrap_or(source.len());
    Ok(format!(
        "{}# {}{}",
        &source[..start],
        title.trim(),
        &source[end..]
    ))
}

fn todo_children_insertion(source: &str, parent_id: &str) -> anyhow::Result<usize> {
    let line = source
        .lines()
        .find(|line| line.contains(&format!("^{parent_id}")))
        .ok_or_else(|| anyhow::anyhow!("unknown Todo: {parent_id}"))?;
    let task_offset = line.as_ptr() as usize - source.as_ptr() as usize;
    let after_task = task_offset + line.len();
    let indent = line.len() - line.trim_start().len();
    let label = format!("\n{}- Children:", " ".repeat(indent + 2));
    let offset = source[after_task..]
        .find(&label)
        .ok_or_else(|| anyhow::anyhow!("Todo Children label was not created: {parent_id}"))?;
    Ok(after_task + offset + label.len() + 1)
}

fn ensure_children_label(source: &str, parent_id: &str) -> anyhow::Result<String> {
    let line = source
        .lines()
        .find(|line| line.contains(&format!("^{parent_id}")))
        .ok_or_else(|| anyhow::anyhow!("unknown Todo: {parent_id}"))?;
    let start = line.as_ptr() as usize - source.as_ptr() as usize;
    let after = start + line.len();
    let indent = line.len() - line.trim_start().len();
    let mut insertion = after;
    for candidate in source[after..].lines() {
        if candidate.trim() == "- Children:" {
            return Ok(source.to_string());
        }
        let candidate_indent = candidate.len() - candidate.trim_start().len();
        if candidate.starts_with("## ")
            || (candidate.trim_start().starts_with("- [") && candidate_indent <= indent)
        {
            break;
        }
        insertion += candidate.len() + 1;
    }
    Ok(format!(
        "{}\n{}  - Children:\n{}",
        &source[..insertion],
        " ".repeat(indent),
        &source[insertion..]
    ))
}

fn replace_task_title(source: &str, todo_id: &str, title: &str) -> anyhow::Result<String> {
    let line = source
        .lines()
        .find(|line| line.contains(&format!("^{todo_id}")))
        .ok_or_else(|| anyhow::anyhow!("unknown Todo: {todo_id}"))?;
    let start = line.as_ptr() as usize - source.as_ptr() as usize;
    let marker_end = line
        .find("] ")
        .ok_or_else(|| anyhow::anyhow!("Todo has no task marker: {todo_id}"))?
        + 2;
    let title_start = marker_end
        + line[marker_end..]
            .find('[')
            .ok_or_else(|| anyhow::anyhow!("Todo has no document link: {todo_id}"))?
        + 1;
    let title_end = title_start
        + line[title_start..]
            .find("](")
            .ok_or_else(|| anyhow::anyhow!("Todo has no document link: {todo_id}"))?;
    let mut result = source.to_string();
    result.replace_range(start + title_start..start + title_end, title.trim());
    Ok(result)
}

fn replace_repeated_todo_field(
    source: &str,
    todo_id: &str,
    field: &str,
    values: &[String],
) -> anyhow::Result<String> {
    for value in values {
        require_text(field, value)?;
    }
    replace_todo_field_values(
        source,
        todo_id,
        field,
        &values.iter().map(|value| value.trim()).collect::<Vec<_>>(),
    )
}

fn replace_completion_criteria(
    source: &str,
    todo_id: &str,
    criteria: &[CheckUpdate],
) -> anyhow::Result<String> {
    let line = source
        .lines()
        .find(|line| line.contains(&format!("^{todo_id}")))
        .ok_or_else(|| anyhow::anyhow!("unknown Todo: {todo_id}"))?;
    let start = line.as_ptr() as usize - source.as_ptr() as usize;
    let indent = line.len() - line.trim_start().len();
    let end = source[start + line.len()..]
        .find("\n- [")
        .map(|offset| start + line.len() + offset + 1)
        .or_else(|| {
            source[start + line.len()..]
                .find("\n## ")
                .map(|offset| start + line.len() + offset + 1)
        })
        .unwrap_or(source.len());
    let existing = &source[start..end];
    let marker = "Completion Criteria:";
    let replacement = if existing.contains(marker) {
        let before = existing.split(marker).next().unwrap();
        let suffix = existing.split(marker).nth(1).unwrap();
        let tail = suffix
            .find("\n  - ")
            .map(|offset| &suffix[offset..])
            .unwrap_or("");
        format!(
            "{before}{marker}\n{}{}",
            criteria
                .iter()
                .map(|c| {
                    let _ = require_text("completion criterion", &c.text);
                    format!(
                        "{}    - [{}] {}\n",
                        " ".repeat(indent),
                        if c.completed { 'x' } else { ' ' },
                        c.text.trim()
                    )
                })
                .collect::<String>(),
            tail
        )
    } else {
        format!(
            "{}\n{}  - Completion Criteria:\n{}",
            existing.trim_end(),
            " ".repeat(indent),
            criteria
                .iter()
                .map(|c| format!(
                    "{}    - [{}] {}\n",
                    " ".repeat(indent),
                    if c.completed { 'x' } else { ' ' },
                    c.text.trim()
                ))
                .collect::<String>()
        )
    };
    Ok(format!(
        "{}{}\n{}",
        &source[..start],
        replacement.trim_end(),
        &source[end..]
    ))
}

fn replace_or_insert_record_field(
    source: &str,
    field: &str,
    value: &str,
) -> anyhow::Result<String> {
    let (start, end) = section_bounds(source, "Record")?;
    let body = &source[start..end];
    if let Some(offset) = body.find(&format!("- {field}:")) {
        let line_end = body[offset..]
            .find('\n')
            .map(|v| offset + v)
            .unwrap_or(body.len());
        return Ok(format!(
            "{}- {field}: {}{}",
            &source[..start + offset],
            value,
            &source[start + line_end..]
        ));
    }
    Ok(format!(
        "{}- {field}: {}\n{}",
        &source[..end],
        value,
        &source[end..]
    ))
}

fn find_todo<'a>(todos: &'a [Todo], id: &str) -> Option<&'a Todo> {
    for todo in todos {
        if todo.id == id {
            return Some(todo);
        }
        if let Some(child) = find_todo(&todo.children, id) {
            return Some(child);
        }
    }
    None
}

fn replace_task_status(source: &str, todo_id: &str, status: TodoStatus) -> anyhow::Result<String> {
    let line = source
        .lines()
        .find(|line| line.contains(&format!("^{todo_id}")))
        .ok_or_else(|| anyhow::anyhow!("unknown Todo: {todo_id}"))?;
    let offset = line.as_ptr() as usize - source.as_ptr() as usize;
    let marker_offset = line
        .find("[ ")
        .or_else(|| line.find("[/"))
        .or_else(|| line.find("[x"))
        .or_else(|| line.find("[?"))
        .or_else(|| line.find("[-"))
        .ok_or_else(|| anyhow::anyhow!("Todo has no supported task marker: {todo_id}"))?
        + 1;
    let absolute = offset + marker_offset;
    let mut next = source.to_string();
    next.replace_range(absolute..absolute + 1, status.marker());
    Ok(next)
}

fn replace_or_insert_todo_field(
    source: &str,
    todo_id: &str,
    field: &str,
    value: &str,
) -> anyhow::Result<String> {
    replace_todo_field_values(source, todo_id, field, &[value])
}

/// Replaces direct fields inside one Todo without touching nested criteria or child Todos.
fn replace_todo_field_values(
    source: &str,
    todo_id: &str,
    field: &str,
    values: &[&str],
) -> anyhow::Result<String> {
    let task_line = source
        .lines()
        .find(|line| line.contains(&format!("^{todo_id}")))
        .ok_or_else(|| anyhow::anyhow!("unknown Todo: {todo_id}"))?;
    let task_offset = task_line.as_ptr() as usize - source.as_ptr() as usize;
    let task_end = task_offset + task_line.len();
    let task_indent = task_line.len() - task_line.trim_start().len();
    let field_prefix = format!("{}- {field}:", " ".repeat(task_indent + 2));
    let body_start = task_end + usize::from(source.as_bytes().get(task_end) == Some(&b'\n'));
    let mut body_end = source.len();
    let mut offset = body_start;
    for line_with_ending in source[body_start..].split_inclusive('\n') {
        let line = line_with_ending
            .strip_suffix('\n')
            .unwrap_or(line_with_ending);
        let indent = line.len() - line.trim_start().len();
        if line.starts_with("## ") || (indent <= task_indent && is_task_line(line.trim_start())) {
            body_end = offset;
            break;
        }
        offset += line_with_ending.len();
    }

    let retained_body = source[body_start..body_end]
        .split_inclusive('\n')
        .filter(|line| {
            line.strip_suffix('\n')
                .unwrap_or(line)
                .trim_end()
                .strip_prefix(&field_prefix)
                .is_none_or(|suffix| !suffix.is_empty() && !suffix.starts_with(' '))
        })
        .collect::<String>();
    let fields = values
        .iter()
        .map(|value| format!("{field_prefix} {value}\n"))
        .collect::<String>();
    Ok(format!(
        "{}\n{}{}{}",
        &source[..task_end],
        fields,
        retained_body,
        &source[body_end..]
    ))
}

fn is_task_line(line: &str) -> bool {
    matches!(
        line.as_bytes(),
        [
            b'-',
            b' ',
            b'[',
            b' ' | b'/' | b'x' | b'X' | b'?' | b'-',
            b']',
            ..
        ]
    )
}

fn render_blueprint(id: &str, request: &BlueprintCreateRequest) -> String {
    let constraints = request
        .constraints
        .iter()
        .map(|constraint| format!("- {constraint}"))
        .collect::<Vec<_>>()
        .join("\n");
    let definition_of_done = request
        .definition_of_done
        .iter()
        .map(|item| format!("- [ ] {item} ^dod-{}", Ulid::new()))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "---\nschema: blueprint/v2\nid: {id}\nstate: active\n---\n\n# {title}\n\n## Record\n\n- Created By: {created_by}\n\n## Intent\n\n{intent}\n\n## Constraints\n\n{constraints}\n\n## Definition of Done\n\n{definition_of_done}\n\n## Plan\n\n{plan}\n\n## Rubric\n\n{rubric}\n\n## Todos\n\n## Results\n\n## Evidence\n\n## Revision History\n\n## Notes\n",
        id = id,
        title = request.title.trim(),
        created_by = request.created_by.trim(),
        intent = request.intent.trim(),
        constraints = constraints,
        definition_of_done = definition_of_done,
        plan = request.plan.trim(),
        rubric = request.rubric.trim(),
    )
}

fn render_todo(blueprint_id: &str, todo_id: &str, title: &str, criteria: &[String]) -> String {
    let criteria = criteria
        .iter()
        .map(|item| format!("- [ ] {}", item.trim()))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "---\nschema: blueprint/todo/v2\nid: {todo_id}\nblueprint: {blueprint_id}\n---\n\n# {title}\n\n## Intent\n\n{title}\n\n## Completion Criteria\n\n{criteria}\n\n## Plan\n\n\n## Handoff\n\n\n## Results\n\n\n## Evidence\n\n\n## Revision History\n\n\n## Notes\n"
    )
}

fn require_text(name: &str, value: &str) -> anyhow::Result<()> {
    if value.trim().is_empty() {
        anyhow::bail!("{name} must not be empty");
    }
    Ok(())
}

fn blueprint_state_name(state: BlueprintState) -> &'static str {
    match state {
        BlueprintState::Active => "active",
        BlueprintState::Closed => "closed",
        BlueprintState::Cancelled => "cancelled",
    }
}
