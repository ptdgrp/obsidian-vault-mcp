mod edit;
mod rename;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::query::VaultQueries;

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
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct EditSectionResult {
    pub changed: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct RenameResult {
    pub dry_run: bool,
    pub updated_references: usize,
    pub changed_notes: Vec<String>,
}
