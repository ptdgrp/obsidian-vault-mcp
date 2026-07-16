mod edit;
mod rename;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::query::{McpNonNegativeInteger, VaultQueries, observe_operation};

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

    pub(crate) fn write_note_atomic(
        &self,
        path: &camino::Utf8Path,
        relative_path: &str,
        content: &str,
    ) -> anyhow::Result<()> {
        observe_operation(
            "mutation",
            "write_note",
            &(relative_path, content.len()),
            || {
                self.queries
                    .vault
                    .write_note_atomic(path, content)
                    .map_err(anyhow::Error::from)
            },
        )?;
        self.queries.parse_cache.invalidate(relative_path);
        Ok(())
    }

    pub(crate) fn rename_note_path(
        &self,
        from: &camino::Utf8Path,
        to: &camino::Utf8Path,
        relative_path: &str,
    ) -> anyhow::Result<()> {
        observe_operation("mutation", "rename_note_path", &(from, to), || {
            std::fs::rename(from, to)?;
            Ok(())
        })?;
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
