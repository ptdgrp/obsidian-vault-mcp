use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::blueprint::{
    model::{NotReadyTodo, Todo},
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
    pub ready_todos: Vec<String>,
    pub not_ready_todos: Vec<NotReadyTodo>,
    pub todos: Vec<Todo>,
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
        let StoredBlueprint { id, etag, source } = self.store.create(&id, &source)?;
        Ok(BlueprintCreated { id, etag, source })
    }

    pub fn blueprint_list(&self) -> anyhow::Result<Vec<String>> {
        self.store.list_active()
    }

    pub fn blueprint_get(&self, id: &str) -> anyhow::Result<StoredBlueprint> {
        self.store.read(id)
    }

    pub fn blueprint_status(&self, id: &str) -> anyhow::Result<BlueprintStatus> {
        let stored = self.store.read(id)?;
        let parsed = ParsedBlueprintSource::parse(&format!("{id}.md"), &stored.source)?;
        let readiness = derive_readiness(&parsed.todos)?;
        Ok(BlueprintStatus {
            ready_todos: readiness.ready,
            not_ready_todos: readiness.not_ready,
            todos: parsed.todos,
        })
    }

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
        Ok(self.blueprint_status(blueprint_id)?.todos)
    }

    pub fn todo_assign(
        &self,
        blueprint_id: &str,
        todo_id: &str,
        owner: &str,
        expected_etag: Option<&str>,
    ) -> anyhow::Result<Todo> {
        require_text("owner", owner)?;
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
        let stored = self.store.write(blueprint_id, expected_etag, |source| {
            let source = replace_task_marker(source, todo_id, 'x')?;
            let source = replace_or_insert_todo_field(
                &source,
                todo_id,
                "Completed By",
                completed_by.trim(),
            )?;
            replace_or_insert_todo_field(
                &source,
                todo_id,
                "Result",
                &format!("Summary: {}", summary.trim()),
            )
        })?;
        todo_from_source(blueprint_id, &stored.source, todo_id)
    }

    pub fn dod_update(
        &self,
        blueprint_id: &str,
        dod_id: &str,
        completed: bool,
        expected_etag: Option<&str>,
    ) -> anyhow::Result<StoredBlueprint> {
        self.store.write(blueprint_id, expected_etag, |source| {
            replace_task_marker(source, dod_id, if completed { 'x' } else { ' ' })
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
        let stored = self.store.write(blueprint_id, expected_etag, |source| {
            let outcome = if reason.is_some() {
                "incomplete"
            } else {
                "complete"
            };
            let reason = reason
                .map(|value| format!("- Reason: {}\n", value.trim()))
                .unwrap_or_default();
            Ok(format!(
                "{source}\n### Closure\n\n- Outcome: {outcome}\n- Closed By: {}\n{reason}",
                closed_by.trim()
            ))
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
            Ok(format!(
                "{source}\n### Cancellation\n\n- Cancelled By: {}\n- Reason: {}\n",
                cancelled_by.trim(),
                reason.trim()
            ))
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
