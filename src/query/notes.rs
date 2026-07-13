use std::fs;

use rayon::prelude::*;

use crate::parser::{ParsedNote, slice_text, source_for_line};
use crate::resolver::{IndexedNote, ObsidianRef, RefResolver, ResolveResult};

use super::files::human_size;
use super::section::{section_source, selector_from_reference};
use super::{
    ListNotesResult, NoteStatsResult, NoteStructureResult, NoteSummary, ReadNoteResult,
    SectionSelector, VaultQueries, WordCountMode, find_indexed_note, read_and_parse,
    truncate_chars,
};

const READ_NOTE_NEXT_STEP: &str = "Retry read_note with a bare heading, block, or line reference for targeted access. If you still need more content, retry read_note with a larger max_chars value.";

impl VaultQueries {
    pub fn list_notes(&self) -> anyhow::Result<ListNotesResult> {
        let mut notes = Vec::new();
        for file in self.vault.list_notes()? {
            let title = read_and_parse(self, &file)
                .ok()
                .map(|note| note_title(&note.parsed, &file.relative_path));
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
        max_chars: Option<usize>,
        explicit_selector: Option<SectionSelector>,
    ) -> anyhow::Result<ReadNoteResult> {
        let reference = RefResolver::parse_ref(note);
        if reference.reference.is_some() && explicit_selector.is_some() {
            anyhow::bail!("reference fragment cannot be combined with an explicit selector");
        }
        let selector = explicit_selector.or(selector_from_reference(&reference.reference)?);
        let path = self.resolve_reference_note_path(&reference)?;
        let content = fs::read_to_string(&path)?;
        let relative_path = self.vault.relative_path(&path);
        let parsed = self.parse_file_cached(&path, relative_path.clone())?;
        let total_lines = content.lines().count().max(1) as u64;
        let source = match selector.as_ref() {
            Some(selector) => section_source(&relative_path, &content, &parsed, selector)?,
            None => source_for_line(&relative_path, &content, &parsed, 1, total_lines),
        };
        let selected = slice_text(&content, source.byte_start, source.byte_end);
        let budget = max_chars.unwrap_or(self.vault.config.max_read_note_chars);
        let truncated = selected.chars().count() > budget;
        let content = if truncated {
            truncate_chars(&selected, budget)
        } else {
            selected
        };
        Ok(ReadNoteResult {
            path: relative_path,
            source,
            content,
            truncated,
            next_step: truncated.then_some(READ_NOTE_NEXT_STEP.to_string()),
        })
    }

    pub fn get_note_stats(
        &self,
        note: &str,
        word_count_mode: WordCountMode,
    ) -> anyhow::Result<NoteStatsResult> {
        let reference = RefResolver::parse_ref(note);
        let selector = selector_from_reference(&reference.reference)?;
        if matches!(selector, Some(SectionSelector::Lines { .. })) {
            anyhow::bail!("get_note_stats supports heading and block references only");
        }
        let path = self.resolve_reference_note_path(&reference)?;
        let content = fs::read_to_string(&path)?;
        let note = self.vault.relative_path(&path);
        let parsed = self.parse_file_cached(&path, note.clone())?;
        let source = selector
            .as_ref()
            .map(|selector| section_source(&note, &content, &parsed, selector))
            .transpose()?;
        let selected = source
            .as_ref()
            .map(|source| slice_text(&content, source.byte_start, source.byte_end))
            .unwrap_or(content);
        let word_count = match word_count_mode {
            WordCountMode::Source => count_words(&selected),
            WordCountMode::Visible => count_words(&visible_markdown_text(&selected)),
        };
        let backlink_count = match (&selector, &source) {
            (Some(selector), Some(source)) => {
                self.backlink_count_for_scope(&note, &parsed, selector, source)?
            }
            _ => self.backlink_count_for_path(&note)?,
        };
        Ok(NoteStatsResult {
            note: note.clone(),
            word_count_mode,
            word_count,
            character_count: selected.chars().count(),
            line_count: selected.lines().count(),
            backlink_count,
            source,
        })
    }

    pub fn parse_note(&self, note: &str) -> anyhow::Result<ParsedNote> {
        let path = self.resolve_note_path(note)?;
        Ok(self
            .parse_file_cached(&path, self.vault.relative_path(&path))?
            .as_ref()
            .clone())
    }

    pub fn get_note_structure(&self, note: &str) -> anyhow::Result<NoteStructureResult> {
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

    fn resolve_reference_note_path(
        &self,
        reference: &ObsidianRef,
    ) -> anyhow::Result<camino::Utf8PathBuf> {
        if let Ok((path, _)) = self.vault.read_note(&reference.target) {
            return Ok(path);
        }
        let notes = self.index_notes()?;
        let indexed = find_indexed_note(&reference.raw, &notes)?;
        Ok(indexed.file.path.clone())
    }
}

pub(super) fn note_title(note: &ParsedNote, relative_path: &str) -> String {
    note.headings
        .iter()
        .find(|heading| heading.level == 1)
        .map(|heading| heading.text.trim())
        .filter(|title| !title.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| frontmatter_title(note))
        .unwrap_or_else(|| pathname_title(relative_path))
}

fn frontmatter_title(note: &ParsedNote) -> Option<String> {
    note.frontmatter
        .as_ref()?
        .get("title")?
        .as_str()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(ToOwned::to_owned)
}

fn pathname_title(relative_path: &str) -> String {
    let file_name = relative_path
        .rsplit_once('/')
        .map(|(_, name)| name)
        .unwrap_or(relative_path);
    file_name
        .strip_suffix(".md")
        .unwrap_or(file_name)
        .to_string()
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

fn visible_markdown_text(content: &str) -> String {
    let content = strip_frontmatter(content);
    let without_comments = strip_markdown_comments(content);
    visible_link_text(&without_comments)
}

fn strip_frontmatter(content: &str) -> &str {
    let Some(rest) = content.strip_prefix("---\n") else {
        return content;
    };
    let Some(end) = rest.find("\n---") else {
        return content;
    };
    let after_marker = &rest[end + "\n---".len()..];
    after_marker.strip_prefix('\n').unwrap_or(after_marker)
}

fn strip_markdown_comments(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut index = 0usize;
    while index < content.len() {
        let rest = &content[index..];
        if rest.starts_with("%%") {
            if let Some(end) = rest[2..].find("%%") {
                index += 2 + end + 2;
            } else {
                break;
            }
        } else if rest.starts_with("<!--") {
            if let Some(end) = rest[4..].find("-->") {
                index += 4 + end + 3;
            } else {
                break;
            }
        } else if let Some(ch) = rest.chars().next() {
            out.push(ch);
            index += ch.len_utf8();
        } else {
            break;
        }
    }
    out
}

fn visible_link_text(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut index = 0usize;
    while index < content.len() {
        let rest = &content[index..];
        if rest.starts_with("![[") || rest.starts_with("[[") {
            let offset = if rest.starts_with("![[") { 3 } else { 2 };
            if let Some(end) = rest[offset..].find("]]") {
                let body = &rest[offset..offset + end];
                out.push_str(obsidian_link_label(body));
                out.push(' ');
                index += offset + end + 2;
                continue;
            }
        }
        if rest.starts_with("![") || rest.starts_with('[') {
            let offset = if rest.starts_with("![") { 2 } else { 1 };
            if let Some(label_end) = rest[offset..].find(']') {
                let after_label = offset + label_end + 1;
                if rest[after_label..].starts_with('(') {
                    if let Some(url_end) = rest[after_label + 1..].find(')') {
                        out.push_str(&rest[offset..offset + label_end]);
                        out.push(' ');
                        index += after_label + 1 + url_end + 1;
                        continue;
                    }
                }
            }
        }
        if let Some(ch) = rest.chars().next() {
            out.push(ch);
            index += ch.len_utf8();
        } else {
            break;
        }
    }
    out
}

fn obsidian_link_label(body: &str) -> &str {
    body.rsplit_once('|')
        .map(|(_, alias)| alias)
        .unwrap_or_else(|| {
            body.split_once('#')
                .map(|(target, _)| target)
                .unwrap_or(body)
        })
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
