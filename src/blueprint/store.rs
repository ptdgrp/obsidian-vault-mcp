use std::{
    collections::{HashMap, HashSet, hash_map::DefaultHasher},
    fs::{self, File, OpenOptions},
    hash::{Hash, Hasher},
    io::Write,
};

use camino::{Utf8Path, Utf8PathBuf};
use fs2::FileExt;

use crate::blueprint::{
    BlueprintSource, BlueprintState, TodoDetail,
    document::{DocumentSchema, ParsedDocument},
};

const MANIFEST: &str = "---\nschema: blueprint/v2\n---\n\n# Blueprint Workspace\n";
const MANIFEST_SCHEMA: DocumentSchema = DocumentSchema {
    name: "blueprint/v2",
    required_sections: &[],
};

#[derive(Clone, Debug, schemars::JsonSchema, serde::Serialize)]
pub struct StoredTodoIndex {
    pub id: String,
    #[schemars(with = "String")]
    pub path: Utf8PathBuf,
    pub etag: String,
}

#[derive(Clone, Debug, schemars::JsonSchema, serde::Serialize)]
pub struct StoredBlueprint {
    pub id: String,
    pub state: BlueprintState,
    #[schemars(with = "String")]
    pub path: Utf8PathBuf,
    pub etag: String,
    pub source: String,
    pub todos: Vec<StoredTodoIndex>,
}

#[derive(Clone, Debug, schemars::JsonSchema, serde::Serialize)]
pub struct StoredTodo {
    pub blueprint_id: String,
    pub id: String,
    #[schemars(with = "String")]
    pub path: Utf8PathBuf,
    pub etag: String,
    pub source: String,
}

#[derive(Clone, Debug)]
pub struct BlueprintStore {
    vault_root: Utf8PathBuf,
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
        self.ensure_workspace()?;
        let parsed = parse_blueprint(id, source)?;
        if parsed.id != id {
            anyhow::bail!("Blueprint ID does not match document frontmatter: {id}");
        }
        self.with_lock(id, || {
            let directory = self.blueprint_dir(id);
            if directory.exists() {
                anyhow::bail!("Blueprint already exists: {id}");
            }
            fs::create_dir_all(directory.join("todos"))?;
            self.write_file_atomic(&self.blueprint_path(id), source)?;
            self.read(id)
        })
    }

    pub fn read(&self, id: &str) -> anyhow::Result<StoredBlueprint> {
        validate_blueprint_id(id)?;
        self.ensure_workspace()?;
        let path = self.blueprint_path(id);
        let source = fs::read_to_string(&path)
            .map_err(|error| anyhow::anyhow!("cannot read Blueprint {id}: {error}"))?;
        let parsed = parse_blueprint(id, &source)?;
        if parsed.id != id {
            anyhow::bail!("Blueprint ID does not match document frontmatter: {id}");
        }
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
        validate_blueprint_id(id)?;
        self.ensure_workspace()?;
        self.with_lock(id, || {
            let current = self.read(id)?;
            check_etag(expected_etag, &current.etag)?;
            let source = mutate(&current.source)?;
            let parsed = parse_blueprint(id, &source)?;
            if parsed.id != id {
                anyhow::bail!("Blueprint ID does not match document frontmatter: {id}");
            }
            self.write_file_atomic(&current.path, &source)?;
            self.read(id)
        })
    }

    pub fn set_state(
        &self,
        id: &str,
        state: BlueprintState,
        expected_etag: Option<&str>,
    ) -> anyhow::Result<StoredBlueprint> {
        self.write_blueprint(id, expected_etag, |source| {
            let parsed = ParsedDocument::parse(
                &format!("blueprints/{id}/blueprint.md"),
                source,
                DocumentSchema {
                    name: "blueprint/v2",
                    required_sections: &[],
                },
            )?;
            parsed.replace_frontmatter_field(source, "state", state_name(state))
        })
    }

    pub fn create_todo(
        &self,
        blueprint_id: &str,
        id: &str,
        source: &str,
    ) -> anyhow::Result<StoredTodo> {
        validate_blueprint_id(blueprint_id)?;
        validate_todo_id(id)?;
        self.ensure_workspace()?;
        let parsed = TodoDetail::parse(&format!("{id}.md"), source)?;
        if parsed.id != id || parsed.blueprint_id != blueprint_id {
            anyhow::bail!("Todo frontmatter does not match its aggregate path");
        }
        self.with_lock(blueprint_id, || {
            self.read(blueprint_id)?;
            let path = self.todo_path(blueprint_id, id);
            if path.exists() {
                anyhow::bail!("Todo already exists: {id}");
            }
            self.write_file_atomic(&path, source)?;
            self.read_todo(blueprint_id, id)
        })
    }

    pub fn read_todo(&self, blueprint_id: &str, id: &str) -> anyhow::Result<StoredTodo> {
        validate_blueprint_id(blueprint_id)?;
        validate_todo_id(id)?;
        self.ensure_workspace()?;
        let path = self.todo_path(blueprint_id, id);
        let source = fs::read_to_string(&path)
            .map_err(|error| anyhow::anyhow!("cannot read Todo {id}: {error}"))?;
        let parsed = TodoDetail::parse(path.as_str(), &source)?;
        if parsed.id != id || parsed.blueprint_id != blueprint_id {
            anyhow::bail!("Todo frontmatter does not match its aggregate path");
        }
        Ok(StoredTodo {
            blueprint_id: blueprint_id.to_string(),
            id: id.to_string(),
            path,
            etag: etag(&source),
            source,
        })
    }

    pub fn write_todo(
        &self,
        blueprint_id: &str,
        id: &str,
        expected_etag: Option<&str>,
        mutate: impl FnOnce(&str) -> anyhow::Result<String>,
    ) -> anyhow::Result<StoredTodo> {
        validate_blueprint_id(blueprint_id)?;
        validate_todo_id(id)?;
        self.ensure_workspace()?;
        self.with_lock(blueprint_id, || {
            let current = self.read_todo(blueprint_id, id)?;
            check_etag(expected_etag, &current.etag)?;
            let source = mutate(&current.source)?;
            let parsed = TodoDetail::parse(current.path.as_str(), &source)?;
            if parsed.id != id || parsed.blueprint_id != blueprint_id {
                anyhow::bail!("Todo frontmatter does not match its aggregate path");
            }
            self.write_file_atomic(&current.path, &source)?;
            self.read_todo(blueprint_id, id)
        })
    }

    pub fn validate_aggregate(&self, id: &str) -> anyhow::Result<()> {
        let blueprint = self.read(id)?;
        let linked = flatten_todos(&parse_blueprint(id, &blueprint.source)?.todos)
            .into_iter()
            .map(|todo| (todo.id, todo.document))
            .collect::<HashMap<_, _>>();
        let documents = self
            .todo_indexes(id)?
            .into_iter()
            .map(|todo| todo.id)
            .collect::<HashSet<_>>();

        for (todo_id, document) in &linked {
            if document != &format!("todos/{todo_id}.md") {
                anyhow::bail!("Todo {todo_id} must link to todos/{todo_id}.md");
            }
            if !documents.contains(todo_id) {
                anyhow::bail!("missing Todo document: {todo_id}");
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

    pub fn with_lock<T>(
        &self,
        id: &str,
        operation: impl FnOnce() -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        validate_blueprint_id(id)?;
        self.ensure_workspace()?;
        let lock = self.lock(id)?;
        let result = operation();
        FileExt::unlock(&lock)?;
        result
    }

    // These legacy v1 Service entry points are intentionally isolated.  A v2 Store must not
    // hand a v2 document to ParsedBlueprintSource, whose schema does not identify v2 documents.
    pub fn read_active(&self, _id: &str) -> anyhow::Result<StoredBlueprint> {
        anyhow::bail!("legacy Blueprint v1 Service cannot read blueprint/v2 documents")
    }

    pub fn write(
        &self,
        _id: &str,
        _expected_etag: Option<&str>,
        _mutate: impl FnOnce(&str) -> anyhow::Result<String>,
    ) -> anyhow::Result<StoredBlueprint> {
        anyhow::bail!("legacy Blueprint v1 Service cannot write blueprint/v2 documents")
    }

    pub fn move_to(
        &self,
        _id: &str,
        _destination: &str,
        _expected_etag: Option<&str>,
        _mutate: impl FnOnce(&str) -> anyhow::Result<String>,
    ) -> anyhow::Result<StoredBlueprint> {
        anyhow::bail!("legacy Blueprint v1 Service cannot write blueprint/v2 documents")
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
        let mut todos = fs::read_dir(todos_dir)?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = Utf8PathBuf::from_path_buf(entry.path()).ok()?;
                let id = path.file_name()?.strip_suffix(".md")?.to_string();
                let source = fs::read_to_string(&path).ok()?;
                Some(StoredTodoIndex {
                    id,
                    path,
                    etag: etag(&source),
                })
            })
            .collect::<Vec<_>>();
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
        let mut temporary = tempfile::NamedTempFile::new_in(self.workspace_root().join(".tmp"))?;
        temporary.write_all(source.as_bytes())?;
        temporary.persist(path).map_err(|error| error.error)?;
        Ok(())
    }
}

fn parse_blueprint(id: &str, source: &str) -> anyhow::Result<BlueprintSource> {
    BlueprintSource::parse(&format!("blueprints/{id}/blueprint.md"), source)
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
