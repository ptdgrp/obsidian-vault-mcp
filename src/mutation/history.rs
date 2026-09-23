use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    io::Write,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use camino::{Utf8Path, Utf8PathBuf};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::VaultMutations;

const MAX_RECORDS: usize = 100;
const MAX_HISTORY_BYTES: usize = 64 * 1024 * 1024;
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Serialize)]
pub(crate) struct FileChange {
    pub path: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

impl FileChange {
    pub(crate) fn replace(path: String, before: String, after: String) -> Self {
        Self {
            path,
            before: Some(before),
            after: Some(after),
        }
    }

    fn reversed(&self) -> Self {
        Self {
            path: self.path.clone(),
            before: self.after.clone(),
            after: self.before.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct EditRecord {
    id: String,
    operation: String,
    timestamp: String,
    changes: Vec<FileChange>,
}

struct StoredRecord {
    record: EditRecord,
    bytes: usize,
}

#[derive(Default)]
pub(super) struct MemoryHistory {
    undo: VecDeque<StoredRecord>,
    redo: VecDeque<StoredRecord>,
    bytes: usize,
}

impl MemoryHistory {
    fn remember(&mut self, record: EditRecord, bytes: usize) {
        while let Some(discarded) = self.redo.pop_front() {
            self.bytes -= discarded.bytes;
        }
        self.undo.push_back(StoredRecord { record, bytes });
        self.bytes += bytes;
        while self.undo.len() > MAX_RECORDS
            || (self.undo.len() > 1 && self.bytes > MAX_HISTORY_BYTES)
        {
            let oldest = self.undo.pop_front().expect("undo stack is nonempty");
            self.bytes -= oldest.bytes;
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct EditHistoryEntry {
    /// Unique identifier for this undo or redo operation.
    pub operation_id: String,
    /// Tool or command that created the edit.
    pub command: String,
    /// Time the edit was recorded.
    pub timestamp: String,
    /// Vault-relative paths affected by the edit.
    pub affected_files: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct EditHistoryResult {
    /// Most recent undoable operation first.
    pub undo: Vec<EditHistoryEntry>,
    /// Next operation to redo first.
    pub redo: Vec<EditHistoryEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct UndoEditResult {
    /// Identifiers of operations undone by this request.
    pub operation_ids: Vec<String>,
    /// Vault-relative paths changed by this request.
    pub changed_notes: Vec<String>,
    /// Conflicts that prevented one or more operations from being undone.
    pub conflicts: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct RedoEditResult {
    /// Identifiers of operations redone by this request.
    pub operation_ids: Vec<String>,
    /// Vault-relative paths changed by this request.
    pub changed_notes: Vec<String>,
    /// Conflicts that prevented one or more operations from being redone.
    pub conflicts: Vec<String>,
}

impl EditRecord {
    fn summary(&self) -> EditHistoryEntry {
        EditHistoryEntry {
            operation_id: self.id.clone(),
            command: self.operation.clone(),
            timestamp: self.timestamp.clone(),
            affected_files: self
                .changes
                .iter()
                .map(|change| change.path.clone())
                .collect(),
        }
    }
}

impl VaultMutations {
    pub(crate) fn commit_changes(
        &self,
        operation: &str,
        mut changes: Vec<FileChange>,
    ) -> anyhow::Result<String> {
        let mut history = self
            .history
            .lock()
            .map_err(|_| anyhow::anyhow!("edit history lock is poisoned"))?;
        changes.sort_by(|left, right| left.path.cmp(&right.path));
        let record = EditRecord {
            id: new_id(),
            operation: operation.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            changes,
        };
        let bytes = serde_json::to_vec(&record)?.len();
        self.apply_transaction(&record.changes)?;
        let id = record.id.clone();
        history.remember(record, bytes);
        Ok(id)
    }

    pub fn edit_history(&self) -> anyhow::Result<EditHistoryResult> {
        let history = self
            .history
            .lock()
            .map_err(|_| anyhow::anyhow!("edit history lock is poisoned"))?;
        Ok(EditHistoryResult {
            undo: history
                .undo
                .iter()
                .rev()
                .map(|entry| entry.record.summary())
                .collect(),
            redo: history
                .redo
                .iter()
                .rev()
                .map(|entry| entry.record.summary())
                .collect(),
        })
    }

    pub fn undo_edit(&self, steps: usize) -> anyhow::Result<UndoEditResult> {
        let mut history = self
            .history
            .lock()
            .map_err(|_| anyhow::anyhow!("edit history lock is poisoned"))?;
        validate_steps(steps, history.undo.len(), "undo")?;
        let selected: Vec<_> = history.undo.iter().rev().take(steps).collect();
        let operation_ids = selected
            .iter()
            .map(|entry| entry.record.id.clone())
            .collect();
        let (changes, conflicts) = self.plan_steps(selected.iter().copied(), true)?;
        if !conflicts.is_empty() {
            return Ok(UndoEditResult {
                operation_ids,
                changed_notes: Vec::new(),
                conflicts,
            });
        }
        if !changes.is_empty() {
            self.apply_transaction(&changes)?;
        }
        let changed_notes = changes.into_iter().map(|change| change.path).collect();
        for _ in 0..steps {
            let record = history
                .undo
                .pop_back()
                .expect("undo stack has requested steps");
            history.redo.push_back(record);
        }
        Ok(UndoEditResult {
            operation_ids,
            changed_notes,
            conflicts: Vec::new(),
        })
    }

    pub fn redo_edit(&self, steps: usize) -> anyhow::Result<RedoEditResult> {
        let mut history = self
            .history
            .lock()
            .map_err(|_| anyhow::anyhow!("edit history lock is poisoned"))?;
        validate_steps(steps, history.redo.len(), "redo")?;
        let selected: Vec<_> = history.redo.iter().rev().take(steps).collect();
        let operation_ids = selected
            .iter()
            .map(|entry| entry.record.id.clone())
            .collect();
        let (changes, conflicts) = self.plan_steps(selected.iter().copied(), false)?;
        if !conflicts.is_empty() {
            return Ok(RedoEditResult {
                operation_ids,
                changed_notes: Vec::new(),
                conflicts,
            });
        }
        if !changes.is_empty() {
            self.apply_transaction(&changes)?;
        }
        let changed_notes = changes.into_iter().map(|change| change.path).collect();
        for _ in 0..steps {
            let record = history
                .redo
                .pop_back()
                .expect("redo stack has requested steps");
            history.undo.push_back(record);
        }
        Ok(RedoEditResult {
            operation_ids,
            changed_notes,
            conflicts: Vec::new(),
        })
    }

    fn plan_steps<'a>(
        &self,
        records: impl Iterator<Item = &'a StoredRecord>,
        undo: bool,
    ) -> anyhow::Result<(Vec<FileChange>, Vec<String>)> {
        let mut initial = BTreeMap::<String, Option<String>>::new();
        let mut simulated = BTreeMap::<String, Option<String>>::new();
        let mut conflicts = BTreeSet::new();
        for entry in records {
            for change in &entry.record.changes {
                if !initial.contains_key(&change.path) {
                    let path = self.safe_note_path(&change.path)?;
                    let content = read_current(&path)?;
                    initial.insert(change.path.clone(), content.clone());
                    simulated.insert(change.path.clone(), content);
                }
                let (expected, replacement) = if undo {
                    (&change.after, &change.before)
                } else {
                    (&change.before, &change.after)
                };
                let current = simulated.get_mut(&change.path).expect("path initialized");
                if current != expected {
                    conflicts.insert(change.path.clone());
                } else {
                    *current = replacement.clone();
                }
            }
        }
        if !conflicts.is_empty() {
            return Ok((Vec::new(), conflicts.into_iter().collect()));
        }
        let changes = initial
            .into_iter()
            .filter_map(|(path, before)| {
                let after = simulated.remove(&path).expect("path initialized");
                (before != after).then_some(FileChange {
                    path,
                    before,
                    after,
                })
            })
            .collect();
        Ok((changes, Vec::new()))
    }

    fn apply_transaction(&self, changes: &[FileChange]) -> anyhow::Result<()> {
        self.apply_transaction_with_writer(changes, |this, change| {
            this.apply_state(change, &change.after)
        })
    }

    fn apply_transaction_with_writer(
        &self,
        changes: &[FileChange],
        mut write: impl FnMut(&Self, &FileChange) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        if changes.is_empty() {
            anyhow::bail!("edit produced no changes");
        }
        let mut paths = std::collections::HashSet::new();
        for change in changes {
            if !paths.insert(&change.path) {
                anyhow::bail!("duplicate changed note: {}", change.path);
            }
            if change.before == change.after {
                anyhow::bail!("edit produced no changes for `{}`", change.path);
            }
            let path = self.safe_note_path(&change.path)?;
            if read_current(&path)? != change.before {
                anyhow::bail!("note changed on disk before edit: `{}`", change.path);
            }
            if change
                .after
                .as_ref()
                .is_some_and(|content| content.len() > self.queries.vault.config().max_note_bytes)
            {
                anyhow::bail!("note exceeds configured maximum size: `{}`", change.path);
            }
        }
        let mut applied = Vec::new();
        for change in changes {
            let result = (|| {
                let path = self.safe_note_path(&change.path)?;
                if read_current(&path)? != change.before {
                    anyhow::bail!("note changed on disk during edit: `{}`", change.path);
                }
                write(self, change)
            })();
            if let Err(error) = result {
                return match self.rollback(&applied) {
                    Ok(()) => Err(error.context("edit failed; prior changes were restored")),
                    Err(rollback_error) => Err(error.context(format!(
                        "edit failed; rollback needs attention: {rollback_error}"
                    ))),
                };
            }
            applied.push(change);
        }
        Ok(())
    }

    fn rollback(&self, applied: &[&FileChange]) -> anyhow::Result<()> {
        for change in applied {
            let path = self.safe_note_path(&change.path)?;
            if read_current(&path)? != change.after {
                anyhow::bail!(
                    "rollback conflicts with external change to `{}`",
                    change.path
                );
            }
        }
        for change in applied.iter().rev() {
            let reversed = change.reversed();
            self.apply_state(&reversed, &reversed.after)?;
        }
        Ok(())
    }

    fn safe_note_path(&self, relative: &str) -> anyhow::Result<Utf8PathBuf> {
        let path = self.queries.vault.resolve_exact_note_path(relative)?;
        let root = fs::canonicalize(&self.queries.vault.root)?;
        let mut parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("note has no parent"))?;
        while !parent.exists() {
            parent = parent
                .parent()
                .ok_or_else(|| anyhow::anyhow!("note has no existing parent"))?;
        }
        if !fs::canonicalize(parent)?.starts_with(root) {
            anyhow::bail!("note path escapes vault root: {relative}");
        }
        if let Ok(metadata) = fs::symlink_metadata(&path)
            && (metadata.file_type().is_symlink() || !metadata.is_file())
        {
            anyhow::bail!("not a regular Markdown note: {relative}");
        }
        Ok(path)
    }

    fn apply_state(&self, change: &FileChange, state: &Option<String>) -> anyhow::Result<()> {
        let path = self.safe_note_path(&change.path)?;
        match state {
            Some(content) => {
                fs::create_dir_all(path.parent().expect("note has parent"))?;
                if change.before.is_none() {
                    let mut temporary =
                        tempfile::NamedTempFile::new_in(path.parent().expect("note has parent"))?;
                    temporary.write_all(content.as_bytes())?;
                    temporary.persist_noclobber(&path)?;
                    self.queries.parse_cache.invalidate(&change.path);
                } else {
                    self.write_note_atomic(&path, &change.path, content)?;
                }
            }
            None => {
                if path.exists() {
                    fs::remove_file(&path)?;
                }
                self.queries.parse_cache.invalidate(&change.path);
            }
        }
        Ok(())
    }
}

fn validate_steps(steps: usize, available: usize, action: &str) -> anyhow::Result<()> {
    if !(1..=MAX_RECORDS).contains(&steps) {
        anyhow::bail!("steps must be between 1 and {MAX_RECORDS}");
    }
    if available == 0 {
        anyhow::bail!("nothing to {action}");
    }
    if steps > available {
        anyhow::bail!("cannot {action} {steps} steps; only {available} available");
    }
    Ok(())
}

fn read_current(path: &Utf8Path) -> anyhow::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn new_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "{now:032x}-{:08x}-{:08x}",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        query::VaultQueries,
        vault::{Vault, VaultConfig},
    };
    use std::sync::Arc;
    use tempfile::tempdir;

    fn fixture() -> (tempfile::TempDir, VaultMutations) {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join("a.md"), "one\n").unwrap();
        fs::write(directory.path().join("b.md"), "two\n").unwrap();
        let root = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let vault = Arc::new(Vault::open(&root, VaultConfig::default()).unwrap());
        (directory, VaultMutations::new(VaultQueries::new(vault)))
    }

    #[test]
    fn undo_redo_multi_note_operation_and_reset_after_new_edit() {
        let (directory, mutations) = fixture();
        mutations
            .commit_changes(
                "first",
                vec![
                    FileChange::replace("a.md".into(), "one\n".into(), "ONE\n".into()),
                    FileChange::replace("b.md".into(), "two\n".into(), "TWO\n".into()),
                ],
            )
            .unwrap();
        let undone = mutations.undo_edit(1).unwrap();
        assert_eq!(undone.changed_notes, ["a.md", "b.md"]);
        assert_eq!(
            fs::read_to_string(directory.path().join("a.md")).unwrap(),
            "one\n"
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("b.md")).unwrap(),
            "two\n"
        );
        let redone = mutations.redo_edit(1).unwrap();
        assert_eq!(redone.operation_ids, undone.operation_ids);
        assert_eq!(
            fs::read_to_string(directory.path().join("a.md")).unwrap(),
            "ONE\n"
        );
        mutations.undo_edit(1).unwrap();
        mutations
            .commit_changes(
                "new",
                vec![FileChange::replace(
                    "a.md".into(),
                    "one\n".into(),
                    "different\n".into(),
                )],
            )
            .unwrap();
        assert!(
            mutations
                .redo_edit(1)
                .unwrap_err()
                .to_string()
                .contains("nothing to redo")
        );
    }

    #[test]
    fn consecutive_undos_redo_in_order() {
        let (directory, mutations) = fixture();
        mutations
            .commit_changes(
                "first",
                vec![FileChange::replace(
                    "a.md".into(),
                    "one\n".into(),
                    "ONE\n".into(),
                )],
            )
            .unwrap();
        mutations
            .commit_changes(
                "second",
                vec![FileChange::replace(
                    "a.md".into(),
                    "ONE\n".into(),
                    "FINAL\n".into(),
                )],
            )
            .unwrap();
        mutations.undo_edit(1).unwrap();
        mutations.undo_edit(1).unwrap();
        assert_eq!(
            fs::read_to_string(directory.path().join("a.md")).unwrap(),
            "one\n"
        );
        mutations.redo_edit(1).unwrap();
        mutations.redo_edit(1).unwrap();
        assert_eq!(
            fs::read_to_string(directory.path().join("a.md")).unwrap(),
            "FINAL\n"
        );
    }

    #[test]
    fn history_metadata_and_multiple_steps_follow_stack_order() {
        let (directory, mutations) = fixture();
        let first = mutations
            .commit_changes(
                "edit_note",
                vec![FileChange::replace(
                    "a.md".into(),
                    "one\n".into(),
                    "ONE\n".into(),
                )],
            )
            .unwrap();
        let second = mutations
            .commit_changes(
                "rename_heading",
                vec![
                    FileChange::replace("a.md".into(), "ONE\n".into(), "FINAL\n".into()),
                    FileChange::replace("b.md".into(), "two\n".into(), "TWO\n".into()),
                ],
            )
            .unwrap();
        let history = mutations.edit_history().unwrap();
        assert_eq!(history.undo[0].operation_id, second);
        assert_eq!(history.undo[0].command, "rename_heading");
        assert_eq!(history.undo[0].affected_files, ["a.md", "b.md"]);
        assert!(!history.undo[0].timestamp.is_empty());
        assert_eq!(history.undo[1].operation_id, first);
        assert!(history.redo.is_empty());

        let undone = mutations.undo_edit(2).unwrap();
        assert_eq!(undone.operation_ids, [second.clone(), first.clone()]);
        assert_eq!(undone.changed_notes, ["a.md", "b.md"]);
        assert_eq!(
            fs::read_to_string(directory.path().join("a.md")).unwrap(),
            "one\n"
        );
        let history = mutations.edit_history().unwrap();
        assert!(history.undo.is_empty());
        assert_eq!(history.redo[0].operation_id, first);
        assert_eq!(history.redo[1].operation_id, second);

        let redone = mutations.redo_edit(2).unwrap();
        assert_eq!(redone.operation_ids, [first, second]);
        assert_eq!(
            fs::read_to_string(directory.path().join("a.md")).unwrap(),
            "FINAL\n"
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("b.md")).unwrap(),
            "TWO\n"
        );
    }

    #[test]
    fn multi_step_conflict_and_insufficient_history_change_nothing() {
        let (directory, mutations) = fixture();
        mutations
            .commit_changes(
                "first",
                vec![FileChange::replace(
                    "a.md".into(),
                    "one\n".into(),
                    "ONE\n".into(),
                )],
            )
            .unwrap();
        mutations
            .commit_changes(
                "second",
                vec![FileChange::replace(
                    "b.md".into(),
                    "two\n".into(),
                    "TWO\n".into(),
                )],
            )
            .unwrap();
        fs::write(directory.path().join("a.md"), "external\n").unwrap();
        let result = mutations.undo_edit(2).unwrap();
        assert_eq!(result.conflicts, ["a.md"]);
        assert!(result.changed_notes.is_empty());
        assert_eq!(
            fs::read_to_string(directory.path().join("b.md")).unwrap(),
            "TWO\n"
        );
        assert_eq!(mutations.edit_history().unwrap().undo.len(), 2);
        assert!(
            mutations
                .undo_edit(3)
                .unwrap_err()
                .to_string()
                .contains("only 2 available")
        );
        assert!(
            mutations
                .undo_edit(0)
                .unwrap_err()
                .to_string()
                .contains("steps must be")
        );
    }

    #[test]
    fn undo_conflict_does_not_change_other_notes() {
        let (directory, mutations) = fixture();
        mutations
            .commit_changes(
                "edit",
                vec![
                    FileChange::replace("a.md".into(), "one\n".into(), "ONE\n".into()),
                    FileChange::replace("b.md".into(), "two\n".into(), "TWO\n".into()),
                ],
            )
            .unwrap();
        fs::write(directory.path().join("b.md"), "external\n").unwrap();
        let result = mutations.undo_edit(1).unwrap();
        assert_eq!(result.conflicts, ["b.md"]);
        assert!(result.changed_notes.is_empty());
        assert_eq!(
            fs::read_to_string(directory.path().join("a.md")).unwrap(),
            "ONE\n"
        );
    }

    #[test]
    fn failed_multi_note_write_rolls_back_without_recording() {
        let (directory, mutations) = fixture();
        let changes = [
            FileChange::replace("a.md".into(), "one\n".into(), "ONE\n".into()),
            FileChange::replace("b.md".into(), "two\n".into(), "TWO\n".into()),
        ];
        let error = mutations
            .apply_transaction_with_writer(&changes, |this, change| {
                if change.path == "b.md" {
                    anyhow::bail!("injected failure");
                }
                this.apply_state(change, &change.after)
            })
            .unwrap_err();
        assert!(error.to_string().contains("prior changes were restored"));
        assert_eq!(
            fs::read_to_string(directory.path().join("a.md")).unwrap(),
            "one\n"
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("b.md")).unwrap(),
            "two\n"
        );
        assert!(mutations.undo_edit(1).is_err());
    }

    #[test]
    fn history_is_memory_only_and_shared_by_clones() {
        let (directory, mutations) = fixture();
        mutations
            .commit_changes(
                "edit",
                vec![FileChange::replace(
                    "a.md".into(),
                    "one\n".into(),
                    "ONE\n".into(),
                )],
            )
            .unwrap();
        mutations.clone().undo_edit(1).unwrap();
        let root = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let fresh = VaultMutations::new(VaultQueries::new(Arc::new(
            Vault::open(&root, VaultConfig::default()).unwrap(),
        )));
        assert!(fresh.undo_edit(1).is_err());
        assert!(!directory.path().join(".obsidian-vault-mcp").exists());
    }

    #[test]
    fn creation_deletion_and_move_can_be_undone() {
        let (directory, mutations) = fixture();
        mutations.create_note("new.md", "new\n").unwrap();
        mutations.undo_edit(1).unwrap();
        assert!(!directory.path().join("new.md").exists());
        mutations.delete_note("b.md", false).unwrap();
        mutations.undo_edit(1).unwrap();
        assert_eq!(
            fs::read_to_string(directory.path().join("b.md")).unwrap(),
            "two\n"
        );
        mutations
            .rename_note("a.md", "folder/moved.md", false)
            .unwrap();
        mutations.undo_edit(1).unwrap();
        assert_eq!(
            fs::read_to_string(directory.path().join("a.md")).unwrap(),
            "one\n"
        );
        assert!(!directory.path().join("folder/moved.md").exists());
    }

    #[test]
    fn history_evicts_oldest_operation_at_limit() {
        let (_directory, mutations) = fixture();
        let mut before = "one\n".to_string();
        for index in 0..=MAX_RECORDS {
            let after = format!("version {index}\n");
            mutations
                .commit_changes(
                    "edit",
                    vec![FileChange::replace("a.md".into(), before, after.clone())],
                )
                .unwrap();
            before = after;
        }
        assert_eq!(mutations.history.lock().unwrap().undo.len(), MAX_RECORDS);
    }
}
