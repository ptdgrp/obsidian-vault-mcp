use std::fs;

use crate::resolver::{IndexedNote, RefResolver, ResolveResult};
use crate::vault::NoteFile;

use super::{
    ContextGroup, ContextItem, ContextResult, SearchSource, VaultQueries, find_indexed_note,
    truncate_utf8,
};

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
        let mut budget = self.vault.config.max_output_bytes;
        let mut omitted = 0;
        let mut groups = Vec::new();

        groups.push(ContextGroup {
            kind: "current".to_string(),
            items: vec![context_item(&current.file, &mut budget, &mut omitted)],
        });

        let mut outlink_items = Vec::new();
        for link in &current.parsed.links {
            if let ResolveResult::Resolved { path, .. } = RefResolver::resolve(&link.target, notes)
                && let Some(note) = notes.iter().find(|note| note.file.relative_path == path)
            {
                outlink_items.push(context_item(&note.file, &mut budget, &mut omitted));
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
                backlink_items.push(context_item(&note.file, &mut budget, &mut omitted));
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

fn context_item(file: &NoteFile, budget: &mut usize, omitted: &mut usize) -> ContextItem {
    let content = fs::read_to_string(&file.path).unwrap_or_default();
    if *budget == 0 {
        *omitted += 1;
        return ContextItem {
            source: SearchSource {
                path: file.relative_path.clone(),
                line_start: 1,
                line_end: 1,
                section: None,
            },
            content: String::new(),
        };
    }
    let clipped = truncate_utf8(&content, *budget).to_string();
    *budget = budget.saturating_sub(clipped.len());
    ContextItem {
        source: SearchSource {
            path: file.relative_path.clone(),
            line_start: 1,
            line_end: content.lines().count().max(1) as u64,
            section: None,
        },
        content: clipped,
    }
}
