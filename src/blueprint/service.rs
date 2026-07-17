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
