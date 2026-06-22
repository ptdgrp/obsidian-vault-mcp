use std::fs;

use rayon::prelude::*;

use crate::parser::ParsedNote;
use crate::resolver::{IndexedNote, RefResolver, ResolveResult};

use super::files::human_size;
use super::{
    ListNotesResult, NoteStatsResult, NoteSummary, ParseNoteResult, ReadNoteResult, VaultQueries,
    find_indexed_note, read_and_parse, truncate_utf8,
};

const READ_NOTE_NEXT_STEP: &str = "Use get_note_outline to discover structure, then read_section with a heading, line, or block selector for targeted access. If you still need a larger prefix, retry read_note with a larger max_bytes value.";

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

    pub fn read_note(
        &self,
        note: &str,
        max_bytes: Option<usize>,
    ) -> anyhow::Result<ReadNoteResult> {
        let path = self.resolve_note_path(note)?;
        let mut content = fs::read_to_string(&path)?;
        let mut truncated = false;
        let budget = max_bytes.unwrap_or(self.vault.config.max_read_note_bytes);
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

    pub fn get_note_stats(&self, note: &str) -> anyhow::Result<NoteStatsResult> {
        let path = self.resolve_note_path(note)?;
        let content = fs::read_to_string(&path)?;
        let note = self.vault.relative_path(&path);
        Ok(NoteStatsResult {
            note: note.clone(),
            word_count: count_words(&content),
            character_count: content.chars().count(),
            backlink_count: self.backlink_count_for_path(&note)?,
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

    pub(crate) fn resolve_note_path(&self, note: &str) -> anyhow::Result<camino::Utf8PathBuf> {
        if let Ok((path, _)) = self.vault.read_note(note) {
            return Ok(path);
        }
        let notes = self.index_notes()?;
        let indexed = find_indexed_note(note, &notes)?;
        Ok(indexed.file.path.clone())
    }
}

fn count_words(content: &str) -> usize {
    let mut in_word = false;
    let mut count = 0usize;

    for ch in content.chars() {
        if is_cjk_character(ch) {
            count += 1;
            in_word = false;
            continue;
        }
        if ch.is_ascii_alphanumeric() {
            if !in_word {
                count += 1;
                in_word = true;
            }
            continue;
        }
        if ch == '-' && in_word {
            continue;
        }
        in_word = false;
    }

    count
}

fn is_cjk_character(ch: char) -> bool {
    matches!(
        ch,
        '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{20000}'..='\u{2A6DF}'
            | '\u{2A700}'..='\u{2B73F}'
            | '\u{2B740}'..='\u{2B81F}'
            | '\u{2B820}'..='\u{2CEAF}'
            | '\u{2CEB0}'..='\u{2EBEF}'
            | '\u{30000}'..='\u{3134F}'
            | '\u{3040}'..='\u{309F}'
            | '\u{30A0}'..='\u{30FF}'
            | '\u{31F0}'..='\u{31FF}'
            | '\u{AC00}'..='\u{D7AF}'
    )
}
