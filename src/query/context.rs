use crate::resolver::{IndexedNote, RefResolver, ResolveResult};

use super::files::human_size;
use super::notes::note_title;
use super::{ContextGroup, ContextItem, ContextResult, VaultQueries, find_indexed_note};

impl VaultQueries {
    pub fn collect_note_context(&self, note: &str) -> anyhow::Result<ContextResult> {
        let notes = self.index_notes()?;
        let indexed = find_indexed_note(note, &notes)?;
        self.collect_context_for_path(&indexed.file.relative_path, &notes, note)
    }

    pub fn collect_reference_context(&self, reference: &str) -> anyhow::Result<ContextResult> {
        let notes = self.index_notes()?;
        match RefResolver::resolve(reference, &notes) {
            ResolveResult::Resolved { path, .. } => {
                self.collect_context_for_path(&path, &notes, reference)
            }
            _ => Ok(ContextResult {
                reference: reference.to_string(),
                groups: Vec::new(),
                truncated: false,
                omitted_count: 0,
            }),
        }
    }
    fn collect_context_for_path(
        &self,
        path: &str,
        notes: &[IndexedNote],
        reference: &str,
    ) -> anyhow::Result<ContextResult> {
        let Some(current) = notes.iter().find(|note| note.file.relative_path == path) else {
            return Ok(ContextResult {
                reference: reference.to_string(),
                groups: Vec::new(),
                truncated: false,
                omitted_count: 0,
            });
        };
        let max_items = self.vault.config.max_results;
        let mut returned = 1;
        let mut omitted = 0;
        let mut groups = Vec::new();

        groups.push(ContextGroup {
            kind: "current".to_string(),
            items: vec![context_item(current)],
        });

        let mut outlink_items = Vec::new();
        let mut append_item = |items: &mut Vec<ContextItem>, note: &IndexedNote| {
            if returned < max_items {
                items.push(context_item(note));
                returned += 1;
            } else {
                omitted += 1;
            }
        };
        for link in &current.parsed.links {
            if let ResolveResult::Resolved { path, .. } = RefResolver::resolve(&link.target, notes)
                && let Some(note) = notes.iter().find(|note| note.file.relative_path == path)
            {
                append_item(&mut outlink_items, note);
            }
        }
        if !outlink_items.is_empty() {
            groups.push(ContextGroup {
                kind: "outlinks".to_string(),
                items: outlink_items,
            });
        }

        let mut backlink_items = Vec::new();
        for note in notes {
            if note.file.relative_path == current.file.relative_path {
                continue;
            }
            if note
                .parsed
                .links
                .iter()
                .any(|link| RefResolver::link_matches(&link.target, path, notes))
            {
                append_item(&mut backlink_items, note);
            }
        }
        if !backlink_items.is_empty() {
            groups.push(ContextGroup {
                kind: "backlinks".to_string(),
                items: backlink_items,
            });
        }

        Ok(ContextResult {
            reference: reference.to_string(),
            groups,
            truncated: omitted > 0,
            omitted_count: omitted,
        })
    }
}

fn context_item(note: &IndexedNote) -> ContextItem {
    ContextItem {
        path: note.file.relative_path.clone(),
        title: Some(note_title(&note.parsed, &note.file.relative_path)),
        size: human_size(note.file.size_bytes),
    }
}
