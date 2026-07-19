use std::{
    collections::{HashMap, HashSet, hash_map::DefaultHasher},
    fs::{self, File, OpenOptions},
    hash::{Hash, Hasher},
    io::Write,
};

use camino::{Utf8Path, Utf8PathBuf};
use fs2::FileExt;

use crate::blueprint::{
    BlueprintSource, BlueprintState, TodoDetail, TodoStatus,
    document::{DocumentSchema, ParsedDocument},
};

const MANIFEST: &str = "---\nschema: blueprint/v3\n---\n\n# Blueprint Workspace\n";
const MANIFEST_SCHEMA: DocumentSchema = DocumentSchema {
    name: "blueprint/v3",
    required_sections: &[],
};

#[derive(Clone, Debug, schemars::JsonSchema, serde::Serialize)]
pub struct StoredTodoIndex {
    pub id: String,
    #[serde(skip)]
    #[schemars(skip)]
    #[schemars(with = "String")]
    pub path: Utf8PathBuf,
    pub etag: String,
}

#[derive(Clone, Debug, schemars::JsonSchema, serde::Serialize)]
pub struct StoredBlueprint {
    pub id: String,
    pub state: BlueprintState,
    #[serde(skip)]
    #[schemars(skip)]
    #[schemars(with = "String")]
    pub path: Utf8PathBuf,
    pub etag: String,
    #[serde(skip)]
    #[schemars(skip)]
    pub source: String,
    #[serde(skip)]
    #[schemars(skip)]
    pub todos: Vec<StoredTodoIndex>,
}

#[derive(Clone, Debug, schemars::JsonSchema, serde::Serialize)]
pub struct StoredTodo {
    pub blueprint_id: String,
    pub id: String,
    #[serde(skip)]
    #[schemars(skip)]
    #[schemars(with = "String")]
    pub path: Utf8PathBuf,
    pub etag: String,
    #[serde(skip)]
    #[schemars(skip)]
    pub source: String,
}

#[derive(Clone, Debug)]
pub struct BlueprintStore {
    vault_root: Utf8PathBuf,
}

/// Holds the one exclusive lock for a Blueprint aggregate and exposes only operations that do
/// not acquire that lock again. This lets a caller update the aggregate document and its Todo
/// documents atomically with respect to other Store writers.
pub(crate) struct LockedBlueprintStore<'store> {
    store: &'store BlueprintStore,
    id: &'store str,
    _lock: File,
}

impl BlueprintStore {
    pub fn new(vault_root: Utf8PathBuf) -> Self {
        Self { vault_root }
    }

    pub fn workspace_root(&self) -> Utf8PathBuf {
        self.vault_root.join(".blueprint")
    }

    pub fn ensure_workspace(&self) -> anyhow::Result<()> {
        let root = self.workspace_root();
        for directory in ["blueprints", ".locks", ".tmp"] {
            fs::create_dir_all(root.join(directory))?;
        }
        let manifest = root.join("manifest.md");
        if !manifest.exists() {
            fs::write(manifest, MANIFEST)?;
        } else {
            self.validate_manifest(&manifest)?;
        }
        Ok(())
    }

    pub fn create(&self, id: &str, source: &str) -> anyhow::Result<StoredBlueprint> {
        validate_blueprint_id(id)?;
        validate_blueprint_source(id, source)?;
        self.with_lock(id, |locked| locked.create_blueprint(source))
    }

    pub fn read(&self, id: &str) -> anyhow::Result<StoredBlueprint> {
        self.with_lock(id, |locked| locked.read_aggregate())
    }

    pub fn list(&self, state: &str) -> anyhow::Result<Vec<String>> {
        let state = BlueprintState::try_from(state)?;
        self.ensure_workspace()?;
        let mut ids = Vec::new();
        for entry in fs::read_dir(self.workspace_root().join("blueprints"))? {
            let path = Utf8PathBuf::from_path_buf(entry?.path())
                .map_err(|_| anyhow::anyhow!("Blueprint path is not UTF-8"))?;
            if !path.is_dir() {
                continue;
            }
            let id = path
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("Blueprint directory has no name"))?;
            validate_blueprint_id(id)?;
            if self.read(id)?.state == state {
                ids.push(id.to_string());
            }
        }
        ids.sort();
        Ok(ids)
    }

    pub fn write_blueprint(
        &self,
        id: &str,
        expected_etag: Option<&str>,
        mutate: impl FnOnce(&str) -> anyhow::Result<String>,
    ) -> anyhow::Result<StoredBlueprint> {
        self.with_lock(id, |locked| locked.write_blueprint(expected_etag, mutate))
    }

    #[cfg(test)]
    pub fn set_state(
        &self,
        id: &str,
        state: BlueprintState,
        expected_etag: Option<&str>,
    ) -> anyhow::Result<StoredBlueprint> {
        self.write_blueprint(id, expected_etag, |source| {
            let parsed = crate::blueprint::document::ParsedDocument::parse(
                &format!("blueprints/{id}/blueprint.md"),
                source,
                crate::blueprint::document::DocumentSchema {
                    name: "blueprint/v3",
                    required_sections: &[],
                },
            )?;
            parsed.replace_frontmatter_field(source, "state", state_name(state))
        })
    }

    #[cfg(test)]
    pub fn create_todo(
        &self,
        blueprint_id: &str,
        id: &str,
        source: &str,
    ) -> anyhow::Result<StoredTodo> {
        self.with_lock(blueprint_id, |locked| {
            let todo = locked.create_todo(id, source)?;
            let title = TodoDetail::parse(todo.path.as_str(), &todo.source)?.title;
            locked.write_blueprint_after_staging_todo(None, |blueprint| {
                let parsed = parse_blueprint(blueprint_id, blueprint)?;
                let insertion = crate::blueprint::document::ParsedDocument::parse(
                    "blueprint.md",
                    blueprint,
                    DocumentSchema {
                        name: "",
                        required_sections: &[],
                    },
                )?
                .section_body_range("Todo Graph")?
                .end;
                if flatten_todos(&parsed.todos)
                    .iter()
                    .any(|node| node.id == id)
                {
                    anyhow::bail!("Todo already exists: {id}");
                }
                Ok(format!(
                    "{}- [ ] [{title}](todos/{id}.md) ^{id}\n{}",
                    &blueprint[..insertion],
                    &blueprint[insertion..]
                ))
            })?;
            Ok(todo)
        })
    }

    pub fn read_todo(&self, blueprint_id: &str, id: &str) -> anyhow::Result<StoredTodo> {
        validate_blueprint_id(blueprint_id)?;
        validate_todo_id(id)?;
        self.ensure_workspace()?;
        self.read_todo_unlocked(blueprint_id, id)
    }

    #[cfg(test)]
    pub fn write_todo(
        &self,
        blueprint_id: &str,
        id: &str,
        expected_etag: Option<&str>,
        mutate: impl FnOnce(&str) -> anyhow::Result<String>,
    ) -> anyhow::Result<StoredTodo> {
        self.with_lock(blueprint_id, |locked| {
            locked.write_todo(id, expected_etag, mutate)
        })
    }

    pub fn validate_aggregate(&self, id: &str) -> anyhow::Result<()> {
        self.with_lock(id, |locked| locked.read_aggregate().map(|_| ()))
    }

    fn validate_aggregate_source(&self, id: &str, source: &str) -> anyhow::Result<()> {
        let linked = flatten_todos(&parse_blueprint(id, source)?.todos)
            .into_iter()
            .map(|todo| (todo.id, (todo.document, todo.title, todo.status)))
            .collect::<HashMap<_, _>>();
        let documents = self
            .todo_indexes(id)?
            .into_iter()
            .map(|todo| todo.id)
            .collect::<HashSet<_>>();

        for (todo_id, (document, title, status)) in &linked {
            if document != &format!("todos/{todo_id}.md") {
                anyhow::bail!("Todo {todo_id} must link to todos/{todo_id}.md");
            }
            if !documents.contains(todo_id) {
                anyhow::bail!("missing Todo document: {todo_id}");
            }
            let detail = self.read_todo_unlocked(id, todo_id)?;
            let detail = TodoDetail::parse(detail.path.as_str(), &detail.source)?;
            if detail.title != *title {
                anyhow::bail!("Todo {todo_id} title differs between graph and detail document");
            }
            match status {
                TodoStatus::Completed if detail.completed_by.is_none() => {
                    anyhow::bail!("completed Todo {todo_id} is missing completed_by frontmatter");
                }
                TodoStatus::Blocked if detail.block_reason.is_none() => {
                    anyhow::bail!("blocked Todo {todo_id} is missing block_reason frontmatter");
                }
                TodoStatus::Cancelled if detail.cancel_reason.is_none() => {
                    anyhow::bail!("cancelled Todo {todo_id} is missing cancel_reason frontmatter");
                }
                _ => {}
            }
        }
        for todo_id in documents {
            if !linked.contains_key(&todo_id) {
                anyhow::bail!("orphan Todo document: {todo_id}");
            }
        }
        Ok(())
    }

    pub fn todo_path(&self, blueprint_id: &str, id: &str) -> Utf8PathBuf {
        self.blueprint_dir(blueprint_id)
            .join("todos")
            .join(format!("{id}.md"))
    }

    pub(crate) fn with_lock<T>(
        &self,
        id: &str,
        operation: impl FnOnce(&LockedBlueprintStore<'_>) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        validate_blueprint_id(id)?;
        self.ensure_workspace()?;
        let locked = LockedBlueprintStore {
            store: self,
            id,
            _lock: self.lock(id)?,
        };
        operation(&locked)
    }

    fn read_unlocked(&self, id: &str) -> anyhow::Result<StoredBlueprint> {
        let path = self.blueprint_path(id);
        let source = fs::read_to_string(&path)
            .map_err(|error| anyhow::anyhow!("cannot read Blueprint {id}: {error}"))?;
        let parsed = validate_blueprint_source(id, &source)?;
        let todos = self.todo_indexes(id)?;
        Ok(StoredBlueprint {
            id: id.to_string(),
            state: parsed.state,
            path,
            etag: etag(&source),
            source,
            todos,
        })
    }

    fn read_todo_unlocked(&self, blueprint_id: &str, id: &str) -> anyhow::Result<StoredTodo> {
        let path = self.todo_path(blueprint_id, id);
        let source = fs::read_to_string(&path)
            .map_err(|error| anyhow::anyhow!("cannot read Todo {id}: {error}"))?;
        validate_todo_source(blueprint_id, id, &path, &source)?;
        Ok(StoredTodo {
            blueprint_id: blueprint_id.to_string(),
            id: id.to_string(),
            path,
            etag: etag(&source),
            source,
        })
    }

    fn blueprint_dir(&self, id: &str) -> Utf8PathBuf {
        self.workspace_root().join("blueprints").join(id)
    }

    fn blueprint_path(&self, id: &str) -> Utf8PathBuf {
        self.blueprint_dir(id).join("blueprint.md")
    }

    fn todo_indexes(&self, blueprint_id: &str) -> anyhow::Result<Vec<StoredTodoIndex>> {
        let todos_dir = self.blueprint_dir(blueprint_id).join("todos");
        if !todos_dir.exists() {
            return Ok(Vec::new());
        }
        let mut todos = Vec::new();
        for entry in fs::read_dir(&todos_dir)? {
            let path = Utf8PathBuf::from_path_buf(entry?.path())
                .map_err(|_| anyhow::anyhow!("Todo path is not UTF-8"))?;
            if !path.is_file() || path.extension() != Some("md") {
                continue;
            }
            let id = path
                .file_stem()
                .ok_or_else(|| anyhow::anyhow!("Todo document has no filename"))?;
            validate_todo_id(id)?;
            let source = fs::read_to_string(&path)
                .map_err(|error| anyhow::anyhow!("cannot read Todo document {path}: {error}"))?;
            validate_todo_source(blueprint_id, id, &path, &source)?;
            todos.push(StoredTodoIndex {
                id: id.to_string(),
                path,
                etag: etag(&source),
            });
        }
        todos.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(todos)
    }

    fn validate_manifest(&self, manifest: &Utf8Path) -> anyhow::Result<()> {
        let source = fs::read_to_string(manifest)?;
        match ParsedDocument::parse(manifest.as_str(), &source, MANIFEST_SCHEMA) {
            Ok(_) => Ok(()),
            Err(_) if source.contains("schema: blueprint/v1") => {
                anyhow::bail!("unsupported Blueprint workspace schema: blueprint/v1")
            }
            Err(error) => anyhow::bail!("invalid Blueprint workspace manifest: {error}"),
        }
    }

    fn lock(&self, id: &str) -> anyhow::Result<File> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(
                self.workspace_root()
                    .join(".locks")
                    .join(format!("{id}.lock")),
            )?;
        file.lock_exclusive()?;
        Ok(file)
    }

    fn write_file_atomic(&self, path: &Utf8Path, source: &str) -> anyhow::Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("document path has no parent: {path}"))?;
        fs::create_dir_all(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(source.as_bytes())?;
        temporary.persist(path).map_err(|error| error.error)?;
        Ok(())
    }
}

impl LockedBlueprintStore<'_> {
    pub(crate) fn require_active(&self) -> anyhow::Result<()> {
        if self.read_aggregate()?.state != BlueprintState::Active {
            anyhow::bail!("Blueprint is not active");
        }
        Ok(())
    }
    pub(crate) fn read_blueprint(&self) -> anyhow::Result<StoredBlueprint> {
        self.store.read_unlocked(self.id)
    }

    /// Reads the complete aggregate under the existing Blueprint lock. This deliberately does
    /// not call the public Store reader, which would attempt to acquire the same lock again.
    pub(crate) fn read_aggregate(&self) -> anyhow::Result<StoredBlueprint> {
        let blueprint = self.read_blueprint()?;
        self.store
            .validate_aggregate_source(self.id, &blueprint.source)?;
        Ok(blueprint)
    }

    pub(crate) fn create_blueprint(&self, source: &str) -> anyhow::Result<StoredBlueprint> {
        validate_blueprint_source(self.id, source)?;
        let directory = self.store.blueprint_dir(self.id);
        if directory.exists() {
            anyhow::bail!("Blueprint already exists: {}", self.id);
        }
        fs::create_dir_all(directory.join("todos"))?;
        self.store
            .write_file_atomic(&self.store.blueprint_path(self.id), source)?;
        self.read_aggregate()
    }

    pub(crate) fn write_blueprint(
        &self,
        expected_etag: Option<&str>,
        mutate: impl FnOnce(&str) -> anyhow::Result<String>,
    ) -> anyhow::Result<StoredBlueprint> {
        self.require_active()?;
        let current = self.read_aggregate()?;
        check_etag(expected_etag, &current.etag)?;
        let source = mutate(&current.source)?;
        validate_blueprint_source(self.id, &source)?;
        self.store.validate_aggregate_source(self.id, &source)?;
        self.store.write_file_atomic(&current.path, &source)?;
        self.read_aggregate()
    }

    /// Commits the central graph after a new detail file has been staged. The ordinary writer
    /// rejects that temporary orphan; this narrow path validates the candidate aggregate before
    /// making the graph link visible.
    pub(crate) fn write_blueprint_after_staging_todo(
        &self,
        expected_etag: Option<&str>,
        mutate: impl FnOnce(&str) -> anyhow::Result<String>,
    ) -> anyhow::Result<StoredBlueprint> {
        let current = self.read_blueprint()?;
        if current.state != BlueprintState::Active {
            anyhow::bail!("Blueprint is not active");
        }
        check_etag(expected_etag, &current.etag)?;
        let source = mutate(&current.source)?;
        validate_blueprint_source(self.id, &source)?;
        self.store.validate_aggregate_source(self.id, &source)?;
        self.store.write_file_atomic(&current.path, &source)?;
        self.read_aggregate()
    }

    pub(crate) fn create_todo(&self, id: &str, source: &str) -> anyhow::Result<StoredTodo> {
        self.require_active()?;
        validate_todo_id(id)?;
        validate_todo_source(self.id, id, &self.store.todo_path(self.id, id), source)?;
        self.read_aggregate()?;
        let path = self.store.todo_path(self.id, id);
        if path.exists() {
            anyhow::bail!("Todo already exists: {id}");
        }
        self.store.write_file_atomic(&path, source)?;
        self.read_todo(id)
    }

    pub(crate) fn remove_todo(&self, id: &str) -> anyhow::Result<()> {
        validate_todo_id(id)?;
        let path = self.store.todo_path(self.id, id);
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    pub(crate) fn read_todo(&self, id: &str) -> anyhow::Result<StoredTodo> {
        validate_todo_id(id)?;
        self.store.read_todo_unlocked(self.id, id)
    }

    pub(crate) fn write_todo(
        &self,
        id: &str,
        expected_etag: Option<&str>,
        mutate: impl FnOnce(&str) -> anyhow::Result<String>,
    ) -> anyhow::Result<StoredTodo> {
        self.require_active()?;
        self.read_aggregate()?;
        validate_todo_id(id)?;
        let current = self.read_todo(id)?;
        check_etag(expected_etag, &current.etag)?;
        let source = mutate(&current.source)?;
        validate_todo_source(self.id, id, &current.path, &source)?;
        self.store.write_file_atomic(&current.path, &source)?;
        self.read_todo(id)
    }
}

fn parse_blueprint(id: &str, source: &str) -> anyhow::Result<BlueprintSource> {
    BlueprintSource::parse(&format!("blueprints/{id}/blueprint.md"), source)
}

fn validate_blueprint_source(id: &str, source: &str) -> anyhow::Result<BlueprintSource> {
    let parsed = parse_blueprint(id, source)?;
    if parsed.id != id {
        anyhow::bail!("Blueprint ID does not match document frontmatter: {id}");
    }
    Ok(parsed)
}

fn validate_todo_source(
    blueprint_id: &str,
    id: &str,
    path: &Utf8Path,
    source: &str,
) -> anyhow::Result<()> {
    let parsed = TodoDetail::parse(path.as_str(), source)?;
    if parsed.id != id || parsed.blueprint_id != blueprint_id {
        anyhow::bail!("Todo frontmatter does not match its aggregate path");
    }
    Ok(())
}

fn flatten_todos(
    todos: &[crate::blueprint::TodoGraphNode],
) -> Vec<crate::blueprint::TodoGraphNode> {
    todos
        .iter()
        .flat_map(|todo| {
            let mut all = vec![todo.clone()];
            all.extend(flatten_todos(&todo.children));
            all
        })
        .collect()
}

fn check_etag(expected: Option<&str>, actual: &str) -> anyhow::Result<()> {
    if expected.is_some_and(|expected| expected != actual) {
        anyhow::bail!("document etag does not match; re-read before writing");
    }
    Ok(())
}

fn validate_blueprint_id(id: &str) -> anyhow::Result<()> {
    if !id.starts_with("bp-") || id.len() <= 3 || id.contains(['/', '\\']) {
        anyhow::bail!("invalid Blueprint ID: {id}");
    }
    Ok(())
}

fn validate_todo_id(id: &str) -> anyhow::Result<()> {
    if !id.starts_with("todo-") || id.len() <= 5 || id.contains(['/', '\\']) {
        anyhow::bail!("invalid Todo ID: {id}");
    }
    Ok(())
}

#[cfg(test)]
fn state_name(state: BlueprintState) -> &'static str {
    match state {
        BlueprintState::Active => "active",
        BlueprintState::Closed => "closed",
        BlueprintState::Cancelled => "cancelled",
    }
}

fn etag(source: &str) -> String {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
