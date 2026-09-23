use std::io::Write;

use super::{CreateNoteResult, DeleteNoteResult, VaultMutations, rename::link_targets_note};

impl VaultMutations {
    pub fn create_note(&self, path: &str, content: &str) -> anyhow::Result<CreateNoteResult> {
        let destination = self.queries.vault.resolve_exact_note_path(path)?;
        let relative_path = self.queries.vault.relative_path(&destination);
        let maximum = self.queries.vault.config().max_note_bytes;
        if content.len() > maximum {
            anyhow::bail!(
                "note exceeds configured maximum size: {} > {maximum} bytes",
                content.len()
            );
        }
        let parent = destination
            .parent()
            .ok_or_else(|| anyhow::anyhow!("note has no parent directory"))?;
        let root = std::fs::canonicalize(&self.queries.vault.root)?;
        let mut existing = parent;
        while !existing.exists() {
            existing = existing
                .parent()
                .ok_or_else(|| anyhow::anyhow!("note has no existing parent directory"))?;
        }
        if !std::fs::canonicalize(existing)?.starts_with(&root) {
            anyhow::bail!("note path escapes vault root: {path}");
        }
        std::fs::create_dir_all(parent)?;
        if !std::fs::canonicalize(parent)?.starts_with(root) {
            anyhow::bail!("note path escapes vault root: {path}");
        }
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(content.as_bytes())?;
        temporary.flush()?;
        temporary.persist_noclobber(&destination).map_err(|error| {
            anyhow::anyhow!("cannot create note `{relative_path}`: {}", error.error)
        })?;
        self.queries.parse_cache.invalidate(&relative_path);
        Ok(CreateNoteResult {
            path: relative_path,
        })
    }

    pub fn delete_note(&self, path: &str, dry_run: bool) -> anyhow::Result<DeleteNoteResult> {
        let target_path = self.queries.vault.resolve_exact_note_path(path)?;
        let relative_path = self.queries.vault.relative_path(&target_path);
        let metadata = std::fs::symlink_metadata(&target_path)
            .map_err(|error| anyhow::anyhow!("note not found: {path}: {error}"))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            anyhow::bail!("not a regular Markdown note: {path}");
        }
        let root = std::fs::canonicalize(&self.queries.vault.root)?;
        if !std::fs::canonicalize(&target_path)?.starts_with(root) {
            anyhow::bail!("note path escapes vault root: {path}");
        }
        let notes = self.queries.index_notes()?;
        if !notes
            .iter()
            .any(|note| note.file.relative_path == relative_path)
        {
            anyhow::bail!("note is not visible in vault: {path}");
        }
        let mut inbound = Vec::new();
        for source_note in &notes {
            if source_note.file.relative_path == relative_path {
                continue;
            }
            let references = source_note
                .parsed
                .links
                .iter()
                .map(|link| (&link.target, &link.source, &link.raw))
                .chain(
                    source_note
                        .parsed
                        .embeds
                        .iter()
                        .map(|embed| (&embed.target, &embed.source, &embed.raw)),
                );
            for (target, source, raw) in references {
                if link_targets_note(
                    target,
                    &source_note.file.relative_path,
                    &relative_path,
                    &notes,
                ) {
                    inbound.push(format!("- {}: {raw}", source.path_with_line_ref()));
                }
            }
        }
        if !inbound.is_empty() {
            anyhow::bail!(
                "cannot delete referenced note `{relative_path}`:\n{}",
                inbound.join("\n")
            );
        }
        if !dry_run {
            std::fs::remove_file(&target_path)?;
            self.queries.parse_cache.invalidate(&relative_path);
        }
        Ok(DeleteNoteResult {
            path: relative_path,
            dry_run,
        })
    }
}
