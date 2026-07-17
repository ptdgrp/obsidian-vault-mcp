use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::blueprint::{
    model::{NotReadyTodo, Todo, TodoStatus},
    source::ParsedBlueprintSource,
    store::{BlueprintStore, StoredBlueprint},
    validate::derive_readiness,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BlueprintCreateRequest {
    pub title: String,
    pub created_by: String,
    pub intent: String,
    pub constraints: Vec<String>,
    pub definition_of_done: Vec<String>,
    pub plan: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct BlueprintCreated {
    pub id: String,
    pub etag: String,
    pub source: String,
}

#[derive(Clone, Debug, Serialize)]
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

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn workspace_root(&self) -> Utf8PathBuf {
        self.store.workspace_root()
    }

    pub fn blueprint_create(
        &self,
        request: BlueprintCreateRequest,
    ) -> anyhow::Result<BlueprintCreated> {
        require_text("title", &request.title)?;
        require_text("created_by", &request.created_by)?;
        require_text("intent", &request.intent)?;
        require_text("plan", &request.plan)?;
        if request.definition_of_done.is_empty() {
            anyhow::bail!("definition_of_done must not be empty");
        }
        let id = format!("bp-{}", Ulid::new());
        let source = render_blueprint(&request);
        let StoredBlueprint {
            id, etag, source, ..
        } = self.store.create(&id, &source)?;
        Ok(BlueprintCreated { id, etag, source })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn blueprint_list(&self) -> anyhow::Result<Vec<String>> {
        self.store.list_active()
    }

    pub fn blueprint_list_in(&self, state: &str) -> anyhow::Result<Vec<String>> {
        self.store.list(state)
    }

    pub fn blueprint_get(&self, id: &str) -> anyhow::Result<StoredBlueprint> {
        self.store.read(id)
    }

    pub fn blueprint_status(&self, id: &str) -> anyhow::Result<BlueprintStatus> {
        let stored = self.store.read(id)?;
        let parsed = ParsedBlueprintSource::parse(&format!("{id}.md"), &stored.source)?;
        let readiness = derive_readiness(&parsed.todos)?;
        let flattened = flatten_todos(&parsed.todos);
        Ok(BlueprintStatus {
            state: stored.state,
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
        self.store.write(id, expected_etag, |source| {
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

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn todo_create(
        &self,
        blueprint_id: &str,
        title: &str,
        created_by: &str,
        owner: Option<&str>,
        depends_on: &[String],
        expected_etag: Option<&str>,
    ) -> anyhow::Result<Todo> {
        require_text("title", title)?;
        require_text("created_by", created_by)?;
        let todo_id = format!("todo-{}", Ulid::new());
        let mut block = format!(
            "- [ ] {} ^{}\n  - Created By: {}\n",
            title.trim(),
            todo_id,
            created_by.trim()
        );
        if let Some(owner) = owner.filter(|owner| !owner.trim().is_empty()) {
            block.push_str(&format!("  - Owner: {}\n", owner.trim()));
        }
        if !depends_on.is_empty() {
            block.push_str(&format!("  - Depends On: {}\n", depends_on.join(", ")));
        }
        let stored = self.store.write(blueprint_id, expected_etag, |source| {
            let marker = "## Todos\n\n";
            let offset = source
                .find(marker)
                .ok_or_else(|| anyhow::anyhow!("missing required section: Todos"))?
                + marker.len();
            Ok(format!(
                "{}{}{}",
                &source[..offset],
                block,
                &source[offset..]
            ))
        })?;
        todo_from_source(blueprint_id, &stored.source, &todo_id)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn todo_create_full(
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
            "- [ ] {} ^{}\n  - Created By: {}\n",
            title.trim(),
            todo_id,
            created_by.trim()
        );
        if let Some(owner) = owner.filter(|v| !v.trim().is_empty()) {
            block.push_str(&format!("  - Owner: {}\n", owner.trim()));
        }
        if !depends_on.is_empty() {
            block.push_str(&format!("  - Depends On: {}\n", depends_on.join(", ")));
        }
        if !completion_criteria.is_empty() {
            block.push_str("  - Completion Criteria:\n");
            for criterion in completion_criteria {
                require_text("completion criterion", criterion)?;
                block.push_str(&format!("    - [ ] {}\n", criterion.trim()));
            }
        }
        self.store.write(blueprint_id, expected_etag, |source| {
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
        let stored = self.store.write(blueprint_id, expected_etag, |source| {
            replace_task_marker(source, todo_id, '/')
        })?;
        todo_from_source(blueprint_id, &stored.source, todo_id)
    }

    pub fn todo_get(&self, blueprint_id: &str, todo_id: &str) -> anyhow::Result<Todo> {
        let stored = self.store.read(blueprint_id)?;
        todo_from_source(blueprint_id, &stored.source, todo_id)
    }

    pub fn todo_list(&self, blueprint_id: &str) -> anyhow::Result<Vec<Todo>> {
        Ok(flatten_todos(&self.blueprint_status(blueprint_id)?.todos)
            .into_iter()
            .cloned()
            .collect())
    }

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
        let stored = self.store.write(blueprint_id, expected_etag, |source| {
            replace_or_insert_todo_field(source, todo_id, "Owner", owner.trim())
        })?;
        todo_from_source(blueprint_id, &stored.source, todo_id)
    }

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
        let stored = self.store.write(blueprint_id, expected_etag, |source| {
            let source = replace_task_marker(source, todo_id, '?')?;
            let source =
                replace_or_insert_todo_field(&source, todo_id, "Block Reason", reason.trim())?;
            replace_or_insert_todo_field(&source, todo_id, "Handoff", handoff.trim())
        })?;
        todo_from_source(blueprint_id, &stored.source, todo_id)
    }

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
        let stored = self.store.write(blueprint_id, expected_etag, |source| {
            let source = replace_task_marker(source, todo_id, '-')?;
            replace_or_insert_todo_field(&source, todo_id, "Cancel Reason", reason.trim())
        })?;
        todo_from_source(blueprint_id, &stored.source, todo_id)
    }

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
        let stored = self.store.write(blueprint_id, expected_etag, |source| {
            let source = replace_task_marker(source, todo_id, 'x')?;
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
        let stored = self.store.write(blueprint_id, expected_etag, |source| {
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
                next =
                    replace_or_insert_todo_field(&next, todo_id, "Result Summary", summary.trim())?;
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

    #[allow(dead_code)]
    pub fn dod_update(
        &self,
        blueprint_id: &str,
        dod_id: &str,
        completed: bool,
        expected_etag: Option<&str>,
    ) -> anyhow::Result<StoredBlueprint> {
        self.dod_update_with_note(blueprint_id, dod_id, completed, None, expected_etag)
    }

    pub fn dod_update_with_note(
        &self,
        blueprint_id: &str,
        dod_id: &str,
        completed: bool,
        note: Option<&str>,
        expected_etag: Option<&str>,
    ) -> anyhow::Result<StoredBlueprint> {
        self.store.write(blueprint_id, expected_etag, |source| {
            let next = replace_task_marker(source, dod_id, if completed { 'x' } else { ' ' })?;
            match note.filter(|value| !value.trim().is_empty()) {
                Some(note) => replace_or_insert_todo_field(&next, dod_id, "Note", note.trim()),
                None => Ok(next),
            }
        })
    }

    pub fn blueprint_close(
        &self,
        blueprint_id: &str,
        closed_by: &str,
        reason: Option<&str>,
        expected_etag: Option<&str>,
    ) -> anyhow::Result<StoredBlueprint> {
        require_text("closed_by", closed_by)?;
        let before = self.store.read_active(blueprint_id)?;
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
        let stored = self.store.write(blueprint_id, expected_etag, |source| {
            let outcome = if !open_todos.is_empty() || !open_dod.is_empty() {
                "incomplete"
            } else {
                "complete"
            };
            let mut next = replace_or_insert_record_field(source, "Closed By", closed_by.trim())?;
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
            next = append_to_section(&next, "Results", &closure)?;
            Ok(next)
        })?;
        self.store.move_to(blueprint_id, "closed")?;
        Ok(stored)
    }

    pub fn blueprint_cancel(
        &self,
        blueprint_id: &str,
        cancelled_by: &str,
        reason: &str,
        expected_etag: Option<&str>,
    ) -> anyhow::Result<StoredBlueprint> {
        require_text("cancelled_by", cancelled_by)?;
        require_text("reason", reason)?;
        let stored = self.store.write(blueprint_id, expected_etag, |source| {
            let next = replace_or_insert_record_field(source, "Cancelled By", cancelled_by.trim())?;
            append_to_section(
                &next,
                "Results",
                &format!(
                    "### Cancellation\n\n- Cancelled By: {}\n- Reason: {}\n",
                    cancelled_by.trim(),
                    reason.trim()
                ),
            )
        })?;
        self.store.move_to(blueprint_id, "cancelled")?;
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
    let stored = store.read_active(blueprint_id)?;
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

fn section_bounds(source: &str, section: &str) -> anyhow::Result<(usize, usize)> {
    let heading = format!("## {section}");
    let start = source
        .find(&heading)
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
    let end = source
        .find('\n')
        .ok_or_else(|| anyhow::anyhow!("Blueprint title is missing"))?;
    if !source.starts_with("# ") {
        anyhow::bail!("Blueprint title must be H1");
    }
    Ok(format!("# {}{}", title.trim(), &source[end..]))
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
    let id_start = line.find(&format!("^{todo_id}")).unwrap();
    let mut result = source.to_string();
    result.replace_range(
        start + marker_end..start + id_start,
        &format!("{} ", title.trim()),
    );
    Ok(result)
}

fn replace_repeated_todo_field(
    source: &str,
    todo_id: &str,
    field: &str,
    values: &[String],
) -> anyhow::Result<String> {
    let mut next = source.to_string();
    for value in values.iter().rev() {
        require_text(field, value)?;
        next = replace_or_insert_todo_field(&next, todo_id, field, value.trim())?;
    }
    Ok(next)
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

fn replace_task_marker(source: &str, todo_id: &str, marker: char) -> anyhow::Result<String> {
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
    next.replace_range(absolute..absolute + 1, &marker.to_string());
    Ok(next)
}

fn replace_or_insert_todo_field(
    source: &str,
    todo_id: &str,
    field: &str,
    value: &str,
) -> anyhow::Result<String> {
    let task_line = source
        .lines()
        .find(|line| line.contains(&format!("^{todo_id}")))
        .ok_or_else(|| anyhow::anyhow!("unknown Todo: {todo_id}"))?;
    let task_offset = task_line.as_ptr() as usize - source.as_ptr() as usize;
    let task_indent = task_line.len() - task_line.trim_start().len();
    let field_prefix = format!("{}- {field}:", " ".repeat(task_indent + 2));
    let after = &source[task_offset + task_line.len()..];
    let mut offset = task_offset + task_line.len();
    for line in after.lines() {
        offset += 1;
        if line.trim_start().starts_with("- [")
            || (line.starts_with("## ") && !line.starts_with("### "))
        {
            break;
        }
        if line.trim_start().starts_with(&format!("- {field}:")) {
            let start = offset;
            let end = offset + line.len();
            let mut result = source.to_string();
            result.replace_range(start..end, &format!("{field_prefix} {value}"));
            return Ok(result);
        }
        offset += line.len();
    }
    let insert = task_offset + task_line.len();
    Ok(format!(
        "{}\n{field_prefix} {value}{}",
        &source[..insert],
        &source[insert..]
    ))
}

fn render_blueprint(request: &BlueprintCreateRequest) -> String {
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
        "# {title}\n\n## Record\n\n- Created By: {created_by}\n\n## Intent\n\n{intent}\n\n## Constraints\n\n{constraints}\n\n## Definition of Done\n\n{definition_of_done}\n\n## Plan\n\n{plan}\n\n## Todos\n\n## Results\n\n## Notes\n",
        title = request.title.trim(),
        created_by = request.created_by.trim(),
        intent = request.intent.trim(),
        constraints = constraints,
        definition_of_done = definition_of_done,
        plan = request.plan.trim(),
    )
}

fn require_text(name: &str, value: &str) -> anyhow::Result<()> {
    if value.trim().is_empty() {
        anyhow::bail!("{name} must not be empty");
    }
    Ok(())
}
