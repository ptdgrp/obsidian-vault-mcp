use camino::Utf8PathBuf;
use schemars::JsonSchema;
use serde::Serialize;
use ulid::Ulid;

use crate::blueprint::{
    model::{
        BlueprintGetOutput, BlueprintResumeOutput, BlueprintState, NotReadyTodo, Todo,
        TodoCreateRequest, TodoGraphNode, TodoPatch, TodoStatus, TodoView,
    },
    source::ParsedBlueprintSource,
    store::{BlueprintStore, LockedBlueprintStore, StoredBlueprint},
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

pub use crate::blueprint::model::CheckUpdate;

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

    fn require_active(&self, blueprint_id: &str) -> anyhow::Result<()> {
        if self.store.read(blueprint_id)?.state != BlueprintState::Active {
            anyhow::bail!("Blueprint is not active");
        }
        Ok(())
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
        let todo_index = stored
            .todos
            .iter()
            .map(|todo| crate::blueprint::model::TodoDocumentIndex {
                id: todo.id.clone(),
                path: todo.path.clone(),
                etag: todo.etag.clone(),
            })
            .collect();
        match view.unwrap_or("full") {
            "full" => Ok(BlueprintGetOutput {
                id: stored.id,
                state: blueprint_state_name(stored.state).to_string(),
                etag: stored.etag,
                source: Some(stored.source),
                todo_index,
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
                    .map(|todo| hydrate_todo(&self.store, id, todo))
                    .collect::<anyhow::Result<Vec<_>>>()?;
                Ok(BlueprintGetOutput {
                    id: stored.id,
                    state: blueprint_state_name(stored.state).to_string(),
                    etag: stored.etag,
                    source: None,
                    todo_index,
                    resume: Some(BlueprintResumeOutput {
                        intent: section_body(&stored.source, "Intent"),
                        constraints: section_body(&stored.source, "Constraints"),
                        plan: section_body(&stored.source, "Plan"),
                        rubric: section_body(&stored.source, "Rubric"),
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
    pub(crate) fn blueprint_update(
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
        self.require_active(id)?;
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
        self.require_active(id)?;
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
    #[cfg(test)]
    pub(crate) fn todo_create_legacy(
        &self,
        blueprint_id: &str,
        title: &str,
        created_by: &str,
        parent_id: Option<&str>,
        owner: Option<&str>,
        depends_on: &[String],
        completion_criteria: &[String],
        expected_etag: Option<&str>,
    ) -> anyhow::Result<TodoView> {
        self.todo_create_with_detail(TodoCreateRequest {
            blueprint_id: blueprint_id.to_string(),
            title: title.to_string(),
            created_by: created_by.to_string(),
            intent: title.to_string(),
            plan: String::new(),
            parent_id: parent_id.map(ToOwned::to_owned),
            owner: owner.map(ToOwned::to_owned),
            depends_on: depends_on.to_vec(),
            completion_criteria: completion_criteria.to_vec(),
            expected_blueprint_etag: expected_etag.map(ToOwned::to_owned),
        })
    }

    /// Creates the Todo detail document before linking it from the central graph.
    pub fn todo_create(&self, request: TodoCreateRequest) -> anyhow::Result<TodoView> {
        require_text("intent", &request.intent)?;
        require_text("plan", &request.plan)?;
        self.todo_create_with_detail(request)
    }

    fn todo_create_with_detail(&self, request: TodoCreateRequest) -> anyhow::Result<TodoView> {
        require_text("title", &request.title)?;
        require_text("created_by", &request.created_by)?;
        for criterion in &request.completion_criteria {
            require_text("completion criterion", criterion)?;
        }
        let todo_id = format!("todo-{}", Ulid::new());
        let detail_source = render_todo(
            &request.blueprint_id,
            &todo_id,
            &request.title,
            &request.intent,
            &request.plan,
            &request.completion_criteria,
        );
        crate::blueprint::TodoDetail::parse(&format!("{todo_id}.md"), &detail_source)?;
        self.store.with_lock(&request.blueprint_id, |locked| {
            locked.require_active()?;
            let blueprint = locked.read_blueprint()?;
            if request
                .expected_blueprint_etag
                .as_deref()
                .is_some_and(|etag| etag != blueprint.etag)
            {
                anyhow::bail!("Blueprint ETag does not match; re-read before writing");
            }
            let parsed = ParsedBlueprintSource::parse(
                &format!("{}.md", request.blueprint_id),
                &blueprint.source,
            )?;
            if let Some(parent_id) = request.parent_id.as_deref() {
                find_todo(&parsed.todos, parent_id)
                    .ok_or_else(|| anyhow::anyhow!("unknown Todo: {parent_id}"))?;
            }
            let mut block = format!(
                "- [ ] [{}](todos/{}.md) ^{}\n  - Created By: {}\n",
                request.title.trim(),
                todo_id,
                todo_id,
                request.created_by.trim()
            );
            if let Some(owner) = request
                .owner
                .as_deref()
                .filter(|owner| !owner.trim().is_empty())
            {
                block.push_str(&format!("  - Owner: {}\n", owner.trim()));
            }
            if !request.depends_on.is_empty() {
                block.push_str(&format!(
                    "  - Depends On: {}\n",
                    request.depends_on.join(", ")
                ));
            }
            let source = if let Some(parent_id) = request.parent_id.as_deref() {
                ensure_children_label(&blueprint.source, parent_id)?
            } else {
                blueprint.source.clone()
            };
            let insertion = if let Some(parent_id) = request.parent_id.as_deref() {
                todo_children_insertion(&source, parent_id)?
            } else {
                section_insertion(&source, "Todos")?
            };
            let indent = if request.parent_id.is_some() {
                "    "
            } else {
                ""
            };
            let block = block
                .lines()
                .map(|line| format!("{indent}{line}\n"))
                .collect::<String>();
            let graph_candidate =
                format!("{}{}{}", &source[..insertion], block, &source[insertion..]);
            let parsed = ParsedBlueprintSource::parse(
                &format!("{}.md", request.blueprint_id),
                &graph_candidate,
            )?;
            derive_readiness(&parsed.todos)?;
            locked.create_todo(&todo_id, &detail_source)?;
            let result = locked.write_blueprint(request.expected_blueprint_etag.as_deref(), |_| {
                Ok(graph_candidate)
            });
            if result.is_err() {
                locked.remove_todo(&todo_id)?;
            }
            result.map(|_| ())
        })?;
        self.todo_get(&request.blueprint_id, &todo_id)
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
    ) -> anyhow::Result<TodoView> {
        self.require_active(blueprint_id)?;
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
        let _stored = self
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
        self.todo_get(blueprint_id, todo_id)
    }
    #[tracing::instrument(
        name = "blueprint.todo_get",
        skip_all,
        fields(operation.kind = "query", operation.name = "todo_get"),
        err
    )]
    pub fn todo_get(&self, blueprint_id: &str, todo_id: &str) -> anyhow::Result<TodoView> {
        todo_view_from_store(&self.store, blueprint_id, todo_id)
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
    ) -> anyhow::Result<Vec<TodoView>> {
        self.store.validate_aggregate(blueprint_id)?;
        let blueprint = self.store.read(blueprint_id)?;
        let parsed =
            ParsedBlueprintSource::parse(&format!("{blueprint_id}.md"), &blueprint.source)?;
        let readiness = derive_readiness(&parsed.todos)?;
        let mut todos = flatten_todos(&parsed.todos)
            .into_iter()
            .map(|todo| todo_view_from_parts(&self.store, &blueprint, todo, &readiness))
            .collect::<anyhow::Result<Vec<_>>>()?;
        todos.retain(|todo| {
            status.is_none_or(|value| todo.graph.status == value)
                && owner.is_none_or(|value| todo.graph.owner.as_deref() == Some(value))
                && ready.is_none_or(|value| todo.ready == value)
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
    ) -> anyhow::Result<TodoView> {
        require_text("owner", owner)?;
        self.store.with_lock(blueprint_id, |locked| {
            locked.require_active()?;
            let graph = locked.read_blueprint()?;
            if expected_etag.is_some_and(|etag| etag != graph.etag) {
                anyhow::bail!("Blueprint ETag does not match; re-read before writing");
            }
            let parsed =
                ParsedBlueprintSource::parse(&format!("{blueprint_id}.md"), &graph.source)?;
            let current = find_todo(&parsed.todos, todo_id)
                .ok_or_else(|| anyhow::anyhow!("unknown Todo: {todo_id}"))?;
            if matches!(
                current.status,
                TodoStatus::Completed | TodoStatus::Cancelled
            ) {
                anyhow::bail!("Todo cannot be assigned in its current state");
            }
            let detail = locked.read_todo(todo_id)?;
            let detail = crate::blueprint::TodoDetail::parse(detail.path.as_str(), &detail.source)?;
            if matches!(current.status, TodoStatus::InProgress | TodoStatus::Blocked)
                && current.owner.as_deref() != Some(owner.trim())
                && detail.handoff.trim().is_empty()
            {
                anyhow::bail!("in_progress or blocked Todo requires Handoff before reassignment");
            }
            let candidate =
                replace_or_insert_todo_field(&graph.source, todo_id, "Owner", owner.trim())?;
            let parsed = ParsedBlueprintSource::parse(&format!("{blueprint_id}.md"), &candidate)?;
            derive_readiness(&parsed.todos)?;
            locked.write_blueprint(expected_etag, |_| Ok(candidate))?;
            Ok(())
        })?;
        self.todo_get(blueprint_id, todo_id)
    }
    #[tracing::instrument(
        name = "blueprint.todo_block",
        skip_all,
        fields(operation.kind = "mutation", operation.name = "todo_block"),
        err
    )]
    #[cfg(test)]
    pub(crate) fn todo_block_legacy(
        &self,
        blueprint_id: &str,
        todo_id: &str,
        reason: &str,
        handoff: &str,
        expected_etag: Option<&str>,
    ) -> anyhow::Result<TodoView> {
        self.todo_block(blueprint_id, todo_id, reason, handoff, expected_etag, None)
    }

    pub fn todo_block(
        &self,
        blueprint_id: &str,
        todo_id: &str,
        reason: &str,
        handoff: &str,
        expected_blueprint_etag: Option<&str>,
        expected_todo_etag: Option<&str>,
    ) -> anyhow::Result<TodoView> {
        require_text("reason", reason)?;
        require_text("handoff", handoff)?;
        self.store.with_lock(blueprint_id, |locked| {
            locked.require_active()?;
            let graph = locked.read_blueprint()?;
            if expected_blueprint_etag.is_some_and(|etag| etag != graph.etag) {
                anyhow::bail!("Blueprint ETag does not match; re-read before writing");
            }
            let parsed =
                ParsedBlueprintSource::parse(&format!("{blueprint_id}.md"), &graph.source)?;
            if find_todo(&parsed.todos, todo_id)
                .ok_or_else(|| anyhow::anyhow!("unknown Todo: {todo_id}"))?
                .status
                != TodoStatus::InProgress
            {
                anyhow::bail!("only in_progress Todo can be blocked");
            }
            let detail = locked.read_todo(todo_id)?;
            if expected_todo_etag.is_some_and(|etag| etag != detail.etag) {
                anyhow::bail!("Todo ETag does not match; re-read before writing");
            }
            let detail_candidate =
                replace_section(&detail.source, "Handoff", &format!("- {}", handoff.trim()))?;
            crate::blueprint::TodoDetail::parse(detail.path.as_str(), &detail_candidate)?;
            let graph_candidate = {
                let source = replace_task_status(&graph.source, todo_id, TodoStatus::Blocked)?;
                replace_or_insert_todo_field(&source, todo_id, "Block Reason", reason.trim())?
            };
            let parsed =
                ParsedBlueprintSource::parse(&format!("{blueprint_id}.md"), &graph_candidate)?;
            derive_readiness(&parsed.todos)?;
            locked.write_todo(todo_id, expected_todo_etag, |_| Ok(detail_candidate))?;
            locked.write_blueprint(expected_blueprint_etag, |_| Ok(graph_candidate))?;
            Ok(())
        })?;
        self.todo_get(blueprint_id, todo_id)
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
    ) -> anyhow::Result<TodoView> {
        self.require_active(blueprint_id)?;
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
        let _stored = self
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
        self.todo_get(blueprint_id, todo_id)
    }
    #[tracing::instrument(
        name = "blueprint.todo_complete",
        skip_all,
        fields(operation.kind = "mutation", operation.name = "todo_complete"),
        err
    )]
    #[cfg(test)]
    pub(crate) fn todo_complete_legacy(
        &self,
        blueprint_id: &str,
        todo_id: &str,
        completed_by: &str,
        summary: &str,
        expected_etag: Option<&str>,
    ) -> anyhow::Result<TodoView> {
        self.todo_complete(
            blueprint_id,
            todo_id,
            completed_by,
            summary,
            expected_etag,
            None,
        )
    }

    pub fn todo_complete(
        &self,
        blueprint_id: &str,
        todo_id: &str,
        completed_by: &str,
        summary: &str,
        expected_blueprint_etag: Option<&str>,
        expected_todo_etag: Option<&str>,
    ) -> anyhow::Result<TodoView> {
        require_text("completed_by", completed_by)?;
        require_text("summary", summary)?;
        self.store.with_lock(blueprint_id, |locked| {
            locked.require_active()?;
            let graph = locked.read_blueprint()?;
            if expected_blueprint_etag.is_some_and(|etag| etag != graph.etag) {
                anyhow::bail!("Blueprint ETag does not match; re-read before writing");
            }
            let parsed =
                ParsedBlueprintSource::parse(&format!("{blueprint_id}.md"), &graph.source)?;
            let current = find_todo(&parsed.todos, todo_id)
                .ok_or_else(|| anyhow::anyhow!("unknown Todo: {todo_id}"))?;
            let detail_source = locked.read_todo(todo_id)?;
            if expected_todo_etag.is_some_and(|etag| etag != detail_source.etag) {
                anyhow::bail!("Todo ETag does not match; re-read before writing");
            }
            let detail = crate::blueprint::TodoDetail::parse(
                detail_source.path.as_str(),
                &detail_source.source,
            )?;
            if current.status != TodoStatus::InProgress {
                anyhow::bail!("only in_progress Todo can be completed");
            }
            if detail
                .completion_criteria
                .iter()
                .any(|item| !item.completed)
            {
                anyhow::bail!("all Completion Criteria must be completed");
            }
            if current
                .children
                .iter()
                .any(|child| !matches!(child.status, TodoStatus::Completed | TodoStatus::Cancelled))
            {
                anyhow::bail!("all non-cancelled child Todos must be completed");
            }
            let detail_candidate = {
                let next = replace_section(&detail_source.source, "Results", summary.trim())?;
                let evidence = section_body(&next, "Evidence");
                let evidence = if evidence.is_empty() {
                    format!(
                        "### Completion summary ^evidence-{}\n\n- Summary: {}",
                        Ulid::new(),
                        summary.trim()
                    )
                } else {
                    evidence
                };
                replace_section(&next, "Evidence", &evidence)
            }?;
            crate::blueprint::TodoDetail::parse(detail_source.path.as_str(), &detail_candidate)?;
            let graph_candidate = {
                let source = replace_task_status(&graph.source, todo_id, TodoStatus::Completed)?;
                replace_or_insert_todo_field(&source, todo_id, "Completed By", completed_by.trim())?
            };
            let parsed =
                ParsedBlueprintSource::parse(&format!("{blueprint_id}.md"), &graph_candidate)?;
            derive_readiness(&parsed.todos)?;
            locked.write_todo(todo_id, expected_todo_etag, |_| Ok(detail_candidate))?;
            locked.write_blueprint(expected_blueprint_etag, |_| Ok(graph_candidate))?;
            Ok(())
        })?;
        self.todo_get(blueprint_id, todo_id)
    }

    #[allow(clippy::too_many_arguments)]
    #[tracing::instrument(
        name = "blueprint.todo_update",
        skip_all,
        fields(operation.kind = "mutation", operation.name = "todo_update"),
        err
    )]
    #[cfg(test)]
    pub(crate) fn todo_update_legacy(
        &self,
        blueprint_id: &str,
        todo_id: &str,
        title: Option<&str>,
        depends_on: Option<&[String]>,
        completion_criteria: Option<&[CheckUpdate]>,
        handoff: Option<&[String]>,
        result_summary: Option<&str>,
        expected_etag: Option<&str>,
    ) -> anyhow::Result<TodoView> {
        self.store.with_lock(blueprint_id, |locked| {
            locked.require_active()?;
            let graph = locked.read_blueprint()?;
            if expected_etag.is_some_and(|etag| etag != graph.etag) {
                anyhow::bail!("document etag does not match; re-read before writing");
            }
            let detail = locked.read_todo(todo_id)?;
            let detail_candidate = todo_update_detail_candidate(
                &detail.source,
                title,
                completion_criteria,
                handoff,
                result_summary,
            )?;
            crate::blueprint::TodoDetail::parse(detail.path.as_str(), &detail_candidate)?;
            let graph_candidate = todo_update_graph_candidate(
                blueprint_id,
                &graph.source,
                todo_id,
                title,
                depends_on,
            )?;
            locked.write_todo(todo_id, None, |_| Ok(detail_candidate))?;
            locked.write_blueprint(expected_etag, |_| Ok(graph_candidate))?;
            Ok(())
        })?;
        self.todo_get(blueprint_id, todo_id)
    }

    /// Updates graph and detail fields under the Blueprint lock, checking each document ETag.
    pub fn todo_update(
        &self,
        blueprint_id: &str,
        todo_id: &str,
        patch: TodoPatch,
        changed_by: Option<&str>,
        change_reason: Option<&str>,
        expected_blueprint_etag: Option<&str>,
        expected_todo_etag: Option<&str>,
    ) -> anyhow::Result<TodoView> {
        let semantic = patch.depends_on.is_some()
            || patch.intent.is_some()
            || patch.completion_criteria.is_some()
            || patch.plan.is_some();
        if semantic {
            require_text("changed_by", changed_by.unwrap_or(""))?;
            require_text("change_reason", change_reason.unwrap_or(""))?;
        }
        self.store.with_lock(blueprint_id, |locked| {
            locked.require_active()?;
            let graph = locked.read_blueprint()?;
            if expected_blueprint_etag.is_some_and(|etag| etag != graph.etag) {
                anyhow::bail!("Blueprint ETag does not match; re-read before writing");
            }
            let detail = locked.read_todo(todo_id)?;
            if expected_todo_etag.is_some_and(|etag| etag != detail.etag) {
                anyhow::bail!("Todo ETag does not match; re-read before writing");
            }
            let mut detail_candidate = todo_patch_detail_candidate(&detail.source, &patch)?;
            if semantic {
                let sections = [
                    patch.depends_on.as_ref().map(|_| "Depends On"),
                    patch.intent.as_ref().map(|_| "Intent"),
                    patch
                        .completion_criteria
                        .as_ref()
                        .map(|_| "Completion Criteria"),
                    patch.plan.as_ref().map(|_| "Plan"),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(", ");
                detail_candidate = append_revision(
                    &detail_candidate,
                    changed_by.expect("validated"),
                    change_reason.expect("validated"),
                    &format!("Todo {todo_id} update ({sections})"),
                )?;
            }
            crate::blueprint::TodoDetail::parse(detail.path.as_str(), &detail_candidate)?;
            let graph_candidate = todo_update_graph_candidate(
                blueprint_id,
                &graph.source,
                todo_id,
                patch.title.as_deref(),
                patch.depends_on.as_deref(),
            )?;
            locked.write_todo(todo_id, expected_todo_etag, |_| Ok(detail_candidate))?;
            locked.write_blueprint(expected_blueprint_etag, |_| Ok(graph_candidate))?;
            Ok(())
        })?;
        self.todo_get(blueprint_id, todo_id)
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
        self.require_active(blueprint_id)?;
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
        self.store.with_lock(blueprint_id, |locked| {
            locked.require_active()?;
            let before = locked.read_blueprint()?;
            let parsed =
                ParsedBlueprintSource::parse(&format!("{blueprint_id}.md"), &before.source)?;
            let open_todos = flatten_todos(&parsed.todos)
                .into_iter()
                .filter(|todo| {
                    !matches!(todo.status, TodoStatus::Completed | TodoStatus::Cancelled)
                })
                .map(|todo| todo.id.clone())
                .collect::<Vec<_>>();
            let open_dod = open_dod_ids(&before.source);
            let complete = open_todos.is_empty() && open_dod.is_empty();
            if complete {
                validate_complete_close_evidence(locked, &before.source)?;
            }
            if !complete && reason.is_none_or(|value| value.trim().is_empty()) {
                anyhow::bail!("reason is required when closing incomplete Blueprint");
            }
            locked.write_blueprint(expected_etag, |source| {
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
                next = append_revision(
                    &next,
                    closed_by,
                    reason.unwrap_or("closure"),
                    "Blueprint closure",
                )?;
                replace_state(&next, BlueprintState::Closed)
            })
        })
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
        self.store.with_lock(blueprint_id, |locked| {
            locked.require_active()?;
            locked.write_blueprint(expected_etag, |source| {
                let next =
                    replace_or_insert_record_field(source, "Cancelled By", cancelled_by.trim())?;
                let results = section_body(&next, "Results");
                let next = replace_section(
                    &next,
                    "Results",
                    &format!(
                        "{results}\n\n### Cancellation\n\n- Cancelled By: {}\n- Reason: {}\n",
                        cancelled_by.trim(),
                        reason.trim()
                    ),
                )?;
                let next = append_revision(&next, cancelled_by, reason, "Blueprint cancellation")?;
                replace_state(&next, BlueprintState::Cancelled)
            })
        })
    }
}

fn todo_from_source(blueprint_id: &str, source: &str, todo_id: &str) -> anyhow::Result<Todo> {
    let parsed = ParsedBlueprintSource::parse(&format!("{blueprint_id}.md"), source)?;
    find_todo(&parsed.todos, todo_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Todo was not found after write: {todo_id}"))
}

fn hydrate_todo(store: &BlueprintStore, blueprint_id: &str, todo: &Todo) -> anyhow::Result<Todo> {
    let detail_source = store.read_todo(blueprint_id, &todo.id)?;
    let detail =
        crate::blueprint::TodoDetail::parse(detail_source.path.as_str(), &detail_source.source)?;
    let mut hydrated = todo.clone();
    hydrated.completion_criteria = detail.completion_criteria;
    hydrated.handoff = detail
        .handoff
        .lines()
        .filter_map(|line| line.trim().strip_prefix("- ").map(ToOwned::to_owned))
        .collect();
    hydrated.result_summary = (!detail.results.trim().is_empty()).then_some(detail.results);
    hydrated.children = todo
        .children
        .iter()
        .map(|child| hydrate_todo(store, blueprint_id, child))
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(hydrated)
}

fn todo_from_store(
    store: &BlueprintStore,
    blueprint_id: &str,
    todo_id: &str,
) -> anyhow::Result<Todo> {
    let stored = store.read(blueprint_id)?;
    let central = todo_from_source(blueprint_id, &stored.source, todo_id)?;
    hydrate_todo(store, blueprint_id, &central)
}

fn todo_view_from_store(
    store: &BlueprintStore,
    blueprint_id: &str,
    todo_id: &str,
) -> anyhow::Result<TodoView> {
    store.validate_aggregate(blueprint_id)?;
    let blueprint = store.read(blueprint_id)?;
    let parsed = ParsedBlueprintSource::parse(&format!("{blueprint_id}.md"), &blueprint.source)?;
    let readiness = derive_readiness(&parsed.todos)?;
    let todo = find_todo(&parsed.todos, todo_id)
        .ok_or_else(|| anyhow::anyhow!("unknown Todo: {todo_id}"))?;
    todo_view_from_parts(store, &blueprint, todo, &readiness)
}

fn todo_view_from_parts(
    store: &BlueprintStore,
    blueprint: &StoredBlueprint,
    todo: &Todo,
    readiness: &crate::blueprint::model::Readiness,
) -> anyhow::Result<TodoView> {
    let detail_source = store.read_todo(&blueprint.id, &todo.id)?;
    let detail =
        crate::blueprint::TodoDetail::parse(detail_source.path.as_str(), &detail_source.source)?;
    if detail.title != todo.title {
        anyhow::bail!(
            "Todo {} title differs between graph and detail document",
            todo.id
        );
    }
    let unsatisfied_dependencies = readiness
        .not_ready
        .iter()
        .find(|item| item.id == todo.id)
        .map(|item| item.unsatisfied_dependencies.clone())
        .unwrap_or_default();
    let projection = hydrate_todo(store, &blueprint.id, todo)?;
    Ok(TodoView::new(
        todo_graph_node(todo),
        detail,
        blueprint.etag.clone(),
        detail_source.etag,
        readiness.ready.iter().any(|id| id == &todo.id),
        unsatisfied_dependencies,
        projection,
    ))
}

fn todo_graph_node(todo: &Todo) -> TodoGraphNode {
    TodoGraphNode {
        id: todo.id.clone(),
        title: todo.title.clone(),
        document: format!("todos/{}.md", todo.id),
        status: todo.status,
        created_by: todo.created_by.clone(),
        owner: todo.owner.clone(),
        completed_by: todo.completed_by.clone(),
        depends_on: todo.depends_on.clone(),
        block_reason: todo.block_reason.clone(),
        cancel_reason: todo.cancel_reason.clone(),
        children: todo.children.iter().map(todo_graph_node).collect(),
    }
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

fn todo_update_detail_candidate(
    source: &str,
    title: Option<&str>,
    completion_criteria: Option<&[CheckUpdate]>,
    handoff: Option<&[String]>,
    result_summary: Option<&str>,
) -> anyhow::Result<String> {
    let mut next = source.to_string();
    if let Some(title) = title {
        require_text("title", title)?;
        next = replace_title(&next, title)?;
    }
    if let Some(criteria) = completion_criteria {
        for item in criteria {
            require_text("completion criterion", &item.text)?;
        }
        next = replace_section(
            &next,
            "Completion Criteria",
            &criteria
                .iter()
                .map(|item| {
                    format!(
                        "- [{}] {}",
                        if item.completed { 'x' } else { ' ' },
                        item.text.trim()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
        )?;
    }
    if let Some(handoff) = handoff {
        next = replace_section(
            &next,
            "Handoff",
            &handoff
                .iter()
                .map(|item| format!("- {}", item.trim()))
                .collect::<Vec<_>>()
                .join("\n"),
        )?;
    }
    if let Some(summary) = result_summary {
        next = replace_section(&next, "Results", summary)?;
    }
    Ok(next)
}

fn todo_patch_detail_candidate(source: &str, patch: &TodoPatch) -> anyhow::Result<String> {
    let mut next = todo_update_detail_candidate(
        source,
        patch.title.as_deref(),
        patch.completion_criteria.as_deref(),
        None,
        patch.results.as_deref(),
    )?;
    if let Some(intent) = patch.intent.as_deref() {
        require_text("intent", intent)?;
        next = replace_section(&next, "Intent", intent)?;
    }
    if let Some(plan) = patch.plan.as_deref() {
        require_text("plan", plan)?;
        next = replace_section(&next, "Plan", plan)?;
    }
    if let Some(handoff) = patch.handoff.as_deref() {
        next = replace_section(&next, "Handoff", handoff)?;
    }
    if let Some(notes) = patch.notes.as_deref() {
        next = replace_section(&next, "Notes", notes)?;
    }
    Ok(next)
}

fn todo_update_graph_candidate(
    blueprint_id: &str,
    source: &str,
    todo_id: &str,
    title: Option<&str>,
    depends_on: Option<&[String]>,
) -> anyhow::Result<String> {
    let mut next = source.to_string();
    if let Some(title) = title {
        require_text("title", title)?;
        next = replace_task_title(&next, todo_id, title)?;
    }
    if let Some(depends_on) = depends_on {
        next = replace_or_insert_todo_field(&next, todo_id, "Depends On", &depends_on.join(", "))?;
    }
    let parsed = ParsedBlueprintSource::parse(&format!("{blueprint_id}.md"), &next)?;
    derive_readiness(&parsed.todos)?;
    Ok(next)
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

fn render_todo(
    blueprint_id: &str,
    todo_id: &str,
    title: &str,
    intent: &str,
    plan: &str,
    criteria: &[String],
) -> String {
    let criteria = criteria
        .iter()
        .map(|item| format!("- [ ] {}", item.trim()))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "---\nschema: blueprint/todo/v2\nid: {todo_id}\nblueprint: {blueprint_id}\n---\n\n# {title}\n\n## Intent\n\n{intent}\n\n## Completion Criteria\n\n{criteria}\n\n## Plan\n\n{plan}\n\n## Handoff\n\n\n## Results\n\n\n## Evidence\n\n\n## Revision History\n\n\n## Notes\n"
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

fn replace_state(source: &str, state: BlueprintState) -> anyhow::Result<String> {
    let replacement = format!("state: {}", blueprint_state_name(state));
    if !source.contains("state: active") {
        anyhow::bail!("Blueprint is not active");
    }
    Ok(source.replacen("state: active", &replacement, 1))
}

fn append_revision(
    source: &str,
    changed_by: &str,
    reason: &str,
    summary: &str,
) -> anyhow::Result<String> {
    let revision = format!(
        "### {summary} ^revision-{}\n\n- Changed By: {}\n- Reason: {}\n",
        Ulid::new(),
        changed_by.trim(),
        reason.trim()
    );
    let history = section_body(source, "Revision History");
    replace_section(
        source,
        "Revision History",
        &format!("{history}\n\n{revision}"),
    )
}

fn validate_complete_close_evidence(
    locked: &LockedBlueprintStore<'_>,
    source: &str,
) -> anyhow::Result<()> {
    let results = section_body(source, "Results");
    let references = evidence_references(&results)?;
    if results.is_empty() || references.is_empty() {
        anyhow::bail!("complete Blueprint close requires Results with a valid Evidence reference");
    }
    let mut defined = std::collections::HashSet::new();
    let blueprint = crate::blueprint::BlueprintSource::parse("blueprint.md", source)?;
    for evidence in blueprint.evidence {
        if !defined.insert(evidence.id) {
            anyhow::bail!("duplicate Evidence ID");
        }
    }
    for index in locked.read_blueprint()?.todos {
        let todo = locked.read_todo(&index.id)?;
        for evidence in
            crate::blueprint::TodoDetail::parse(todo.path.as_str(), &todo.source)?.evidence
        {
            if !defined.insert(evidence.id) {
                anyhow::bail!("duplicate Evidence ID");
            }
        }
    }
    for reference in references {
        if !defined.contains(&reference) {
            anyhow::bail!("dangling Evidence reference: {reference}");
        }
    }
    Ok(())
}

fn evidence_references(results: &str) -> anyhow::Result<Vec<String>> {
    let mut references = Vec::new();
    let mut rest = results;
    while let Some(start) = rest.find("](") {
        let target = &rest[start + 2..];
        let Some(end) = target.find(')') else {
            anyhow::bail!("malformed Evidence reference");
        };
        let destination = &target[..end];
        if let Some((_, id)) = destination.rsplit_once("#^") {
            if !id.starts_with("evidence-") || id.is_empty() || id.contains(char::is_whitespace) {
                anyhow::bail!("malformed Evidence reference");
            }
            references.push(id.to_string());
        }
        rest = &target[end + 1..];
    }
    if results.contains("#^evidence-") && references.is_empty() {
        anyhow::bail!("malformed Evidence reference");
    }
    Ok(references)
}
