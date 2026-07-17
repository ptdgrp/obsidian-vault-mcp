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
