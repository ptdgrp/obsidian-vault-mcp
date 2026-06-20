use std::fs;

use rayon::prelude::*;

use crate::parser::ParsedNote;
use crate::resolver::{IndexedNote, RefResolver, ResolveResult};

use super::files::human_size;
use super::{
    ListNotesResult, NoteSummary, ParseNoteResult, ReadNoteResult, VaultQueries, find_indexed_note,
    read_and_parse, truncate_utf8,
};

const READ_NOTE_NEXT_STEP: &str = "Use get_note_outline, then read_section with a heading or line selector for the remaining content. To read a larger prefix, increase max-read-note-bytes.";

impl VaultQueries {
    pub fn list_notes(&self) -> anyhow::Result<ListNotesResult> {
        let mut notes = Vec::new();
        for file in self.vault.list_notes()? {
            let title = read_and_parse(self, &file).ok().and_then(|note| {
                note.parsed
                    .headings
                    .first()
                    .map(|heading| heading.text.clone())
            });
            notes.push(NoteSummary {
                path: file.relative_path,
                title,
                size: human_size(file.size_bytes),
            });
        }
        Ok(ListNotesResult { notes })
    }

    pub fn read_note(&self, note: &str) -> anyhow::Result<ReadNoteResult> {
        let path = self.resolve_note_path(note)?;
        let mut content = fs::read_to_string(&path)?;
        let mut truncated = false;
        let budget = self
            .vault
            .config
            .max_read_note_bytes
            .min(self.vault.config.max_output_bytes);
        if content.len() > budget {
            content = truncate_utf8(&content, budget).to_string();
            truncated = true;
        }
        Ok(ReadNoteResult {
            path: self.vault.relative_path(&path),
            content,
            truncated,
            next_step: truncated.then_some(READ_NOTE_NEXT_STEP.to_string()),
        })
    }

    pub fn parse_note(&self, note: &str) -> anyhow::Result<ParsedNote> {
        let path = self.resolve_note_path(note)?;
        Ok(self
            .parse_file_cached(&path, self.vault.relative_path(&path))?
            .as_ref()
            .clone())
    }

    pub fn parse_note_result(&self, note: &str) -> anyhow::Result<ParseNoteResult> {
        Ok(self.parse_note(note)?.into())
    }

    pub fn resolve_ref(&self, reference: &str) -> anyhow::Result<ResolveResult> {
        let notes = self.index_notes()?;
        Ok(RefResolver::resolve(reference, &notes))
    }
    pub fn index_notes(&self) -> anyhow::Result<Vec<IndexedNote>> {
        let mut notes: Vec<IndexedNote> = self
            .vault
            .list_notes()?
            .into_par_iter()
            .filter_map(|file| read_and_parse(self, &file).ok())
            .collect();
        notes.sort_by(|a, b| natord::compare(&a.file.relative_path, &b.file.relative_path));
        Ok(notes)
    }

    pub(super) fn resolve_note_path(&self, note: &str) -> anyhow::Result<camino::Utf8PathBuf> {
        if let Ok((path, _)) = self.vault.read_note(note) {
            return Ok(path);
        }
        let notes = self.index_notes()?;
        let indexed = find_indexed_note(note, &notes)?;
        Ok(indexed.file.path.clone())
    }
}
