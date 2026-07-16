mod edit;
mod rename;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::query::{McpNonNegativeInteger, VaultQueries};

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub struct VaultMutations {
    queries: VaultQueries,
}

impl VaultMutations {
    pub fn new(queries: VaultQueries) -> Self {
        Self { queries }
    }

    #[tracing::instrument(
        name = "vault.mutation.write_note",
        skip_all,
        fields(operation.kind = "mutation", operation.name = "write_note", content.bytes = content.len()),
        err
    )]
    pub(crate) fn write_note_atomic(
        &self,
        path: &camino::Utf8Path,
        relative_path: &str,
        content: &str,
    ) -> anyhow::Result<()> {
        self.queries
            .vault
            .write_note_atomic(path, content)
            .map_err(anyhow::Error::from)?;
        self.queries.parse_cache.invalidate(relative_path);
        Ok(())
    }

    #[tracing::instrument(
        name = "vault.mutation.rename_note_path",
        skip_all,
        fields(operation.kind = "mutation", operation.name = "rename_note_path"),
        err
    )]
    pub(crate) fn rename_note_path(
        &self,
        from: &camino::Utf8Path,
        to: &camino::Utf8Path,
        relative_path: &str,
    ) -> anyhow::Result<()> {
        std::fs::rename(from, to)?;
        self.queries.parse_cache.invalidate(relative_path);
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct EditSectionResult {
    pub changed: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct RenameResult {
    pub dry_run: bool,
    #[schemars(with = "McpNonNegativeInteger")]
    pub updated_references: usize,
    pub changed_notes: Vec<String>,
}
