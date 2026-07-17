use std::{
    collections::hash_map::DefaultHasher,
    fs::{self, File, OpenOptions},
    hash::{Hash, Hasher},
    io::Write,
};

use camino::{Utf8Path, Utf8PathBuf};
use fs2::FileExt;

const MANIFEST: &str = "---\nschema: blueprint/v1\n---\n\n# Blueprint Workspace\n";

#[derive(Clone, Debug)]
pub struct StoredBlueprint {
    pub id: String,
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
        for directory in ["active", "closed", "cancelled", ".locks", ".tmp"] {
            fs::create_dir_all(root.join(directory))?;
        }
        let manifest = root.join("manifest.md");
        if !manifest.exists() {
            fs::write(manifest, MANIFEST)?;
        } else if fs::read_to_string(&manifest)? != MANIFEST {
            anyhow::bail!("invalid Blueprint workspace manifest");
        }
        Ok(())
    }

    pub fn create(&self, id: &str, source: &str) -> anyhow::Result<StoredBlueprint> {
        validate_id(id)?;
        self.ensure_workspace()?;
        let path = self.path(id);
        if path.exists() {
            anyhow::bail!("Blueprint already exists: {id}");
        }
        self.write_file_atomic(&path, source)?;
        self.read(id)
    }

    pub fn read(&self, id: &str) -> anyhow::Result<StoredBlueprint> {
        validate_id(id)?;
        self.ensure_workspace()?;
        let path = self.path(id);
        let source = fs::read_to_string(&path)
            .map_err(|error| anyhow::anyhow!("cannot read Blueprint {id}: {error}"))?;
        Ok(StoredBlueprint {
            id: id.to_string(),
            etag: etag(&source),
            source,
        })
    }

    pub fn write(
        &self,
        id: &str,
        expected_etag: Option<&str>,
        mutate: impl FnOnce(&str) -> anyhow::Result<String>,
    ) -> anyhow::Result<StoredBlueprint> {
        validate_id(id)?;
        self.ensure_workspace()?;
        let lock = self.lock(id)?;
        let result = (|| {
            let current = self.read(id)?;
            if let Some(expected_etag) = expected_etag
                && expected_etag != current.etag
            {
                anyhow::bail!("Blueprint etag does not match; re-read before writing");
            }
            let next = mutate(&current.source)?;
            self.write_file_atomic(&self.path(id), &next)?;
            Ok(StoredBlueprint {
                id: current.id,
                etag: etag(&next),
                source: next,
            })
        })();
        FileExt::unlock(&lock)?;
        result
    }

    fn path(&self, id: &str) -> Utf8PathBuf {
        self.workspace_root()
            .join("active")
            .join(format!("{id}.md"))
    }

    fn lock(&self, id: &str) -> anyhow::Result<File> {
        let lock_path = self
            .workspace_root()
            .join(".locks")
            .join(format!("{id}.lock"));
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(lock_path)?;
        file.lock_exclusive()?;
        Ok(file)
    }

    fn write_file_atomic(&self, path: &Utf8Path, source: &str) -> anyhow::Result<()> {
        let temporary = tempfile::NamedTempFile::new_in(self.workspace_root().join(".tmp"))?;
        let mut temporary = temporary;
        temporary.write_all(source.as_bytes())?;
        temporary.persist(path).map_err(|error| error.error)?;
        Ok(())
    }
}

fn validate_id(id: &str) -> anyhow::Result<()> {
    if !id.starts_with("bp-") || id.len() <= 3 || id.contains(['/', '\\']) {
        anyhow::bail!("invalid Blueprint ID: {id}");
    }
    Ok(())
}

fn etag(source: &str) -> String {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
