mod edit;
mod history;
mod note;
mod rename;
mod text;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::query::{McpNonNegativeInteger, VaultQueries};
pub use history::{EditHistoryResult, RedoEditResult, UndoEditResult};

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub struct VaultMutations {
    queries: VaultQueries,
    history: std::sync::Arc<std::sync::Mutex<history::MemoryHistory>>,
}

impl VaultMutations {
    pub fn new(queries: VaultQueries) -> Self {
        Self {
            queries,
            history: std::sync::Arc::new(std::sync::Mutex::new(history::MemoryHistory::default())),
        }
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
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct EditSectionResult {
    /// Vault-relative path of the note that was changed.
    pub changed: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct CreateNoteResult {
    /// Vault-relative path of the created note.
    pub path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct DeleteNoteResult {
    /// Vault-relative path of the note.
    pub path: String,
    /// Whether the note was only previewed.
    pub dry_run: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct EditNoteResult {
    /// Vault-relative path of the edited note.
    pub path: String,
    /// Whether changes were only previewed.
    pub dry_run: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ApplyPatchResult {
    /// Vault-relative paths that would change or were changed.
    pub changed_notes: Vec<String>,
    /// Whether changes were only previewed.
    pub dry_run: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct RenameResult {
    pub dry_run: bool,
    #[schemars(with = "McpNonNegativeInteger")]
    pub updated_references: usize,
    pub changed_notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SetBlockIdResult {
    /// Whether changes were only previewed.
    pub dry_run: bool,
    /// Block id present before this operation, if any.
    pub previous_block_id: Option<String>,
    /// Resulting block id, including a generated proposal during dry-run; null after deletion.
    pub block_id: Option<String>,
    #[schemars(with = "McpNonNegativeInteger")]
    pub updated_references: usize,
    /// Workspace-relative notes that would change or were changed.
    pub changed_notes: Vec<String>,
}
