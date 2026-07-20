use std::fs;

use rayon::prelude::*;

use crate::parser::{ParsedNote, slice_text, source_for_line};
use crate::resolver::{IndexedNote, ObsidianRef, RefResolver};

use super::path_filter::PathFilter;
use super::public::{Locator, PageSlice, ResolvedReference};
use super::section::{section_source, selector_from_reference};
use super::{
    CompactStructureHeading, ListNotesPagination, ListNotesResult, NoteStatsResult,
    NoteStructureResult, NoteSummary, ReadNoteResult, ResolveRefResult, SectionSelector,
    VaultQueries, compact_resolve_result, find_indexed_note, read_and_parse, truncate_chars,
};

impl VaultQueries {
    #[tracing::instrument(
        name = "vault.query.list_notes",
        skip_all,
        fields(operation.kind = "query", operation.name = "list_notes"),
        err
    )]
    pub fn list_notes(
        &self,
        include: &[String],
        exclude: &[String],
        page: usize,
    ) -> anyhow::Result<ListNotesResult> {
        let filter = PathFilter::new(include, exclude)?;
        let files = self
            .vault
            .list_notes()?
            .into_iter()
            .filter(|file| filter.is_match(&file.relative_path))
            .collect();
        let page = PageSlice::new(files, page, 100)?;
        let total_notes = page.total_items();
        let pagination = page.pagination();
        let notes = page
            .into_items()
            .into_iter()
            .map(|file| {
                let title = read_and_parse(self, &file).ok().map(|note| {
                    truncate_display_title(note_title(&note.parsed, &file.relative_path))
                });
                NoteSummary {
                    path: file.relative_path,
                    title,
                }
            })
            .collect();
        Ok(ListNotesResult {
            notes,
            pagination: ListNotesPagination {
                page: pagination.page,
                total_pages: pagination.total_pages,
                total_notes,
            },
        })
    }

    #[tracing::instrument(
        name = "vault.query.read_note",
        fields(operation.kind = "query", operation.name = "read_note"),
        err
    )]
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
        let relative_path = self.vault.relative_path(&path);
        let (parsed, content) = self.parse_note_from_path(&path, &relative_path)?;
        let total_lines = content.lines().count().max(1) as u64;
        let source = match selector.as_ref() {
            Some(selector) => section_source(&relative_path, &content, &parsed, selector)?,
            None => source_for_line(&relative_path, &content, &parsed, 1, total_lines),
        };
        let selected = slice_text(&content, source.byte_start, source.byte_end);
        let budget = max_chars.unwrap_or(self.vault.config().max_read_note_chars);
        let selected_chars = selected.chars().count();
        let truncated = selected_chars > budget;
        let content = if truncated {
            truncate_chars(&selected, budget)
        } else {
            selected
        };
        let returned_chars = selected_chars.min(budget);
        tracing::info!(
            selector.kind = selector_kind(selector.as_ref()),
            result.truncated = truncated,
            result.selected_chars = selected_chars,
            result.returned_chars = returned_chars,
            result.selected_lines = source.line_end - source.line_start + 1,
            "vault.query.read_note.result"
        );
        Ok(ReadNoteResult {
            source: Locator::lines(&relative_path, source.line_start, source.line_end),
            content,
            truncated,
        })
    }

    #[tracing::instrument(
        name = "vault.query.get_note_stats",
        fields(operation.kind = "query", operation.name = "get_note_stats"),
        err
    )]
    pub fn get_note_stats(&self, note: &str) -> anyhow::Result<NoteStatsResult> {
        let reference = RefResolver::parse_ref(note);
        let selector = selector_from_reference(&reference.reference)?;
        if matches!(selector, Some(SectionSelector::Lines { .. })) {
            anyhow::bail!("get_note_stats supports heading and block references only");
        }
        let path = self.resolve_reference_note_path(&reference)?;
        let relative_path = self.vault.relative_path(&path);
        if selector.is_none() {
            let maximum = self.vault.config().max_note_bytes;
            let size = fs::metadata(&path)?.len();
            if size > maximum as u64 {
                anyhow::bail!("note exceeds configured maximum size: {size} > {maximum} bytes");
            }
            let selected = fs::read_to_string(&path)?;
            return Ok(NoteStatsResult {
                scope: relative_path,
                word_count: count_words(&selected),
                character_count: selected.chars().count(),
                line_count: selected.lines().count(),
            });
        }
        let (parsed, content) = self.parse_note_from_path(&path, &relative_path)?;
        let source = selector
            .as_ref()
            .map(|selector| section_source(&relative_path, &content, &parsed, selector))
            .transpose()?;
        let selected = source
            .as_ref()
            .map(|source| slice_text(&content, source.byte_start, source.byte_end))
            .unwrap_or(content);
        let word_count = count_words(&selected);
        let scope = match (&selector, &source) {
            (Some(SectionSelector::Heading { .. }), Some(source)) => ResolvedReference::heading(
                relative_path.clone(),
                source
                    .section
                    .as_ref()
                    .map(|section| section.heading_path.clone())
                    .unwrap_or_default(),
            )
            .format(),
            (Some(SectionSelector::Block { block_id }), Some(_)) => {
                ResolvedReference::block(relative_path.clone(), block_id.clone()).format()
            }
            _ => relative_path.clone(),
        };
        Ok(NoteStatsResult {
            scope,
            word_count,
            character_count: selected.chars().count(),
            line_count: selected.lines().count(),
        })
    }

    #[tracing::instrument(
        name = "vault.query.get_note_structure",
        fields(operation.kind = "query", operation.name = "get_note_structure"),
        err
    )]
    pub fn get_note_structure(&self, note: &str) -> anyhow::Result<NoteStructureResult> {
        Ok(compact_note_structure(&self.parse_note(note)?.0))
    }

    pub(crate) fn resolve_ref(&self, reference: &str) -> anyhow::Result<ResolveRefResult> {
        let notes = self.index_notes()?;
        Ok(compact_resolve_result(
            RefResolver::resolve(reference, &notes),
            &notes,
        ))
    }
    #[tracing::instrument(
        name = "vault.query.index_notes",
        fields(operation.kind = "query", operation.name = "index_notes"),
        err
    )]
    pub fn index_notes(&self) -> anyhow::Result<Vec<IndexedNote>> {
        let mut notes: Vec<IndexedNote> = self
            .vault
            .list_notes()?
            .into_par_iter()
            .map(|file| read_and_parse(self, &file))
            .collect::<anyhow::Result<_>>()?;
        notes.sort_by(|a, b| natord::compare(&a.file.relative_path, &b.file.relative_path));
        Ok(notes)
    }

    pub(crate) fn resolve_note_path(&self, note: &str) -> anyhow::Result<camino::Utf8PathBuf> {
        if let Ok(path) = self.vault.resolve_path(note)
            && path.is_file()
        {
            return Ok(path);
        }
        let notes = self.index_notes()?;
        let indexed = find_indexed_note(note, &notes)?;
        Ok(indexed.file.path.clone())
    }

    #[tracing::instrument(name = "vault.resolve_reference")]
    fn resolve_reference_note_path(
        &self,
        reference: &ObsidianRef,
    ) -> anyhow::Result<camino::Utf8PathBuf> {
        if let Ok(path) = self.vault.resolve_path(&reference.target)
            && path.is_file()
        {
            return Ok(path);
        }
        let notes = self.index_notes()?;
        let indexed = find_indexed_note(&reference.raw, &notes)?;
        Ok(indexed.file.path.clone())
    }
}

fn selector_kind(selector: Option<&SectionSelector>) -> &'static str {
    match selector {
        None => "whole_note",
        Some(SectionSelector::Heading { .. }) => "heading",
        Some(SectionSelector::Block { .. }) => "block",
        Some(SectionSelector::Lines { .. }) => "lines",
    }
}

fn compact_note_structure(note: &ParsedNote) -> NoteStructureResult {
    const LIMIT: usize = 50;
    let frontmatter_fields = note.frontmatter.as_ref().and_then(frontmatter_fields);
    let mut headings = note
        .headings
        .iter()
        .filter(|heading| heading.level != 1)
        .map(|heading| CompactStructureHeading {
            heading: heading.path.join("/"),
            line: heading.source.line_start,
        })
        .collect::<Vec<_>>();
    headings.sort_by(|left, right| {
        natord::compare(&left.heading, &right.heading).then_with(|| left.line.cmp(&right.line))
    });
    let embeds = sorted_unique(
        note.embeds
            .iter()
            .map(|embed| reference_string(&embed.target, &embed.reference)),
    );
    let tags = sorted_unique(
        note.tags
            .iter()
            .map(|tag| normalize_tag(&tag.tag))
            .chain(frontmatter_tag_values(note.frontmatter.as_ref())),
    );
    let blocks = sorted_unique(note.blocks.iter().map(|block| block.id.clone()));

    let mut omitted = std::collections::BTreeMap::new();
    let headings = limit_group("headings", headings, &mut omitted, LIMIT);
    let embeds = limit_group("embeds", embeds, &mut omitted, LIMIT);
    let tags = limit_group("tags", tags, &mut omitted, LIMIT);
    let blocks = limit_group("blocks", blocks, &mut omitted, LIMIT);

    NoteStructureResult {
        note: note.path.to_owned(),
        link_count: note.links.len(),
        frontmatter_fields,
        headings,
        embeds,
        tags,
        blocks,
        omitted: (!omitted.is_empty()).then_some(omitted),
    }
}

fn frontmatter_fields(frontmatter: &serde_json::Value) -> Option<Vec<String>> {
    let fields = frontmatter.as_object()?;
    let mut fields = fields.keys().cloned().collect::<Vec<_>>();
    fields.sort_by(|left, right| natord::compare(left, right));
    (!fields.is_empty()).then_some(fields)
}

fn frontmatter_tag_values(frontmatter: Option<&serde_json::Value>) -> impl Iterator<Item = String> {
    let values = frontmatter
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flat_map(|object| {
            ["tags", "tag"]
                .into_iter()
                .filter_map(|field| object.get(field))
        })
        .flat_map(tag_values)
        .map(|tag| normalize_tag(&tag))
        .collect::<Vec<_>>();
    values.into_iter()
}

fn tag_values(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::String(value) => value
            .split(|ch: char| ch.is_whitespace() || ch == ',')
            .filter(|part| !part.trim().is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        serde_json::Value::Array(values) => values
            .iter()
            .filter_map(|value| value.as_str().map(ToOwned::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

fn normalize_tag(tag: &str) -> String {
    tag.trim().trim_start_matches('#').to_string()
}

fn reference_string(target: &str, reference: &Option<crate::parser::ReferenceInfo>) -> String {
    match reference {
        None => target.to_string(),
        Some(crate::parser::ReferenceInfo::Heading { value }) => format!("{target}#{value}"),
        Some(crate::parser::ReferenceInfo::MultiHeading { value }) => {
            format!("{target}#{}", value.join("#"))
        }
        Some(crate::parser::ReferenceInfo::BlockId { value }) => format!("{target}#^{value}"),
    }
}

fn sorted_unique(values: impl Iterator<Item = String>) -> Vec<String> {
    let mut values = values
        .filter(|value| !value.is_empty())
        .collect::<Vec<String>>();
    values.sort_by(|left, right| natord::compare(left, right));
    values.dedup();
    values
}

fn limit_group<T>(
    name: &str,
    mut values: Vec<T>,
    omitted: &mut std::collections::BTreeMap<String, usize>,
    limit: usize,
) -> Option<Vec<T>> {
    if values.len() > limit {
        omitted.insert(name.to_string(), values.len() - limit);
        values.truncate(limit);
    }
    (!values.is_empty()).then_some(values)
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

fn truncate_display_title(title: String) -> String {
    const MAX_TITLE_CHARS: usize = 200;
    if title.chars().count() <= MAX_TITLE_CHARS {
        return title;
    }
    let mut truncated = title.chars().take(MAX_TITLE_CHARS - 1).collect::<String>();
    truncated.push('…');
    truncated
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
