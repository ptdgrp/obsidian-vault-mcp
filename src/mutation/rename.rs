use std::collections::BTreeMap;

use super::{RenameResult, SetBlockIdResult, VaultMutations};
use crate::query::find_indexed_note;
use crate::{
    parser::{LinkKind, ReferenceInfo, source_for_line},
    resolver::{RefResolver, ResolveResult},
};

struct TextEdit {
    start: usize,
    end: usize,
    replacement: String,
}

impl VaultMutations {
    pub fn rename_note(
        &self,
        path: &str,
        new_path: &str,
        dry_run: bool,
    ) -> anyhow::Result<RenameResult> {
        let notes = self.queries.index_notes()?;
        let target_path = self.queries.vault.resolve_exact_note_path(path)?;
        let target_relative_path = self.queries.vault.relative_path(&target_path);
        let target = notes
            .iter()
            .find(|note| note.file.relative_path == target_relative_path)
            .ok_or_else(|| anyhow::anyhow!("note not found: {path}"))?;
        let destination = self.queries.vault.resolve_exact_note_path(new_path)?;
        if destination.exists() {
            return Err(anyhow::anyhow!(
                "destination note already exists: {new_path}"
            ));
        }
        let new_relative_path = self.queries.vault.relative_path(&destination);
        let mut edits: BTreeMap<String, Vec<TextEdit>> = BTreeMap::new();
        let mut updated_references = 0;
        for source_note in &notes {
            for link in &source_note.parsed.links {
                if !matches!(link.kind, LinkKind::Wikilink)
                    || !link_targets_note(
                        link.target.as_str(),
                        source_note.file.relative_path.as_str(),
                        target.file.relative_path.as_str(),
                        &notes,
                    )
                {
                    continue;
                }
                edits
                    .entry(source_note.file.relative_path.clone())
                    .or_default()
                    .push(TextEdit {
                        start: link.source.byte_start,
                        end: link.source.byte_end,
                        replacement: replace_link_target(&link.raw, &new_relative_path),
                    });
                if source_note.file.relative_path != target.file.relative_path {
                    updated_references += 1;
                }
            }
        }
        let mut changed_notes: Vec<String> = edits
            .keys()
            .map(|path| {
                if path == &target.file.relative_path {
                    new_relative_path.clone()
                } else {
                    path.clone()
                }
            })
            .collect();
        changed_notes.push(new_relative_path.clone());
        changed_notes.sort_by(|left, right| natord::compare(left, right));
        changed_notes.dedup();
        if !dry_run {
            let parent = destination
                .parent()
                .ok_or_else(|| anyhow::anyhow!("destination has no parent"))?;
            std::fs::create_dir_all(parent)?;
            for (path, note_edits) in edits {
                let indexed = find_indexed_note(&path, &notes)?;
                let updated =
                    apply_edits(std::fs::read_to_string(&indexed.file.path)?, note_edits)?;
                self.write_note_atomic(&indexed.file.path, &indexed.file.relative_path, &updated)?;
            }
            self.rename_note_path(&target.file.path, &destination, &target.file.relative_path)?;
        }
        Ok(RenameResult {
            dry_run,
            updated_references,
            changed_notes,
        })
    }

    pub fn set_block_id(
        &self,
        note: &str,
        old_block_id: Option<&str>,
        content: Option<&str>,
        block_id: Option<&str>,
        dry_run: bool,
    ) -> anyhow::Result<SetBlockIdResult> {
        if old_block_id.is_some() == content.is_some() {
            anyhow::bail!("provide exactly one block selector: old_block_id or content");
        }
        if content.is_some_and(|content| content.trim().is_empty()) {
            anyhow::bail!("content selector must not be empty");
        }
        let new_block_id = block_id
            .map(str::to_string)
            .unwrap_or_else(crate::timx8::generate);
        if !new_block_id.is_empty()
            && !new_block_id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            anyhow::bail!(
                "block_id must be lowercase and contain only ASCII letters, digits, or hyphens"
            );
        }
        let notes = self.queries.index_notes()?;
        let target = find_indexed_note(note, &notes)?;
        let candidates = target
            .parsed
            .block_candidates
            .iter()
            .filter(|candidate| match (old_block_id, content) {
                (Some(old_block_id), None) => candidate.id.as_deref() == Some(old_block_id),
                (None, Some(content)) => candidate.text.contains(content.trim()),
                _ => false,
            })
            .collect::<Vec<_>>();
        let block = match candidates.as_slice() {
            [] => {
                let selector = old_block_id.or(content).unwrap_or_default();
                anyhow::bail!("block not found: {selector}");
            }
            [block] => *block,
            _ => {
                let previews = candidates
                    .iter()
                    .map(|candidate| {
                        let mut preview = candidate.text.chars().take(120).collect::<String>();
                        if candidate.text.chars().count() > 120 {
                            preview.push('…');
                        }
                        format!("- {}: {preview}", candidate.source.path_with_line_ref())
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                anyhow::bail!(
                    "block content is ambiguous: {} matches\n{previews}",
                    candidates.len()
                );
            }
        };
        let previous_block_id = block.id.as_deref();
        if new_block_id.is_empty() && previous_block_id.is_none() {
            anyhow::bail!("target block does not have a block id to delete");
        }
        if new_block_id.is_empty() {
            let mut inbound_references = Vec::new();
            if let Some(previous_block_id) = previous_block_id {
                for source_note in &notes {
                    for link in &source_note.parsed.links {
                        if matches!(
                            &link.reference,
                            Some(ReferenceInfo::BlockId { value }) if value == previous_block_id
                        ) && link_targets_note(
                            link.target.as_str(),
                            source_note.file.relative_path.as_str(),
                            target.file.relative_path.as_str(),
                            &notes,
                        ) {
                            inbound_references.push(format!(
                                "- {}: {}",
                                link.source.path_with_line_ref(),
                                link.raw
                            ));
                        }
                    }
                }
            }
            if !inbound_references.is_empty() {
                anyhow::bail!(
                    "cannot delete referenced block id `{}`:\n{}",
                    previous_block_id.unwrap_or_default(),
                    inbound_references.join("\n")
                );
            }
        }
        if previous_block_id != Some(new_block_id.as_str())
            && target
                .parsed
                .blocks
                .iter()
                .any(|block| block.id == new_block_id)
        {
            anyhow::bail!("block id already exists in note: {new_block_id}");
        }
        let target_content = std::fs::read_to_string(&target.file.path)?;
        let mut edits: BTreeMap<String, Vec<TextEdit>> = BTreeMap::new();
        let (start, end, replacement) = match previous_block_id {
            Some(previous_block_id) => {
                let start = block_id_byte_start(
                    &target_content,
                    block.source.byte_start,
                    block.source.byte_end,
                    previous_block_id,
                )?;
                if new_block_id.is_empty() {
                    (
                        block_id_deletion_start(&target_content, block.source.byte_start, start)?,
                        start + previous_block_id.len() + 1,
                        String::new(),
                    )
                } else {
                    (
                        start,
                        start + previous_block_id.len() + 1,
                        format!("^{new_block_id}"),
                    )
                }
            }
            None => {
                let start = block_id_insertion_point(
                    &target_content,
                    block.source.byte_start,
                    block.source.byte_end,
                )?;
                (start, start, format!(" ^{new_block_id}"))
            }
        };
        edits
            .entry(target.file.relative_path.clone())
            .or_default()
            .push(TextEdit {
                start,
                end,
                replacement,
            });
        let mut updated_references = 0;
        if let Some(previous_block_id) = previous_block_id {
            for source_note in &notes {
                for link in &source_note.parsed.links {
                    if matches!(&link.reference, Some(ReferenceInfo::BlockId { value }) if value == previous_block_id)
                        && link_targets_note(
                            link.target.as_str(),
                            source_note.file.relative_path.as_str(),
                            target.file.relative_path.as_str(),
                            &notes,
                        )
                    {
                        edits
                            .entry(source_note.file.relative_path.clone())
                            .or_default()
                            .push(TextEdit {
                                start: link.source.byte_start,
                                end: link.source.byte_end,
                                replacement: link.raw.replacen(
                                    &format!("#^{previous_block_id}"),
                                    &format!("#^{new_block_id}"),
                                    1,
                                ),
                            });
                        updated_references += 1;
                    }
                }
            }
        }
        let changed_notes = edits.keys().cloned().collect();
        if !dry_run {
            for (path, note_edits) in edits {
                let indexed = find_indexed_note(&path, &notes)?;
                let updated =
                    apply_edits(std::fs::read_to_string(&indexed.file.path)?, note_edits)?;
                self.write_note_atomic(&indexed.file.path, &indexed.file.relative_path, &updated)?;
            }
        }
        Ok(SetBlockIdResult {
            dry_run,
            previous_block_id: previous_block_id.map(str::to_string),
            block_id: (!new_block_id.is_empty()).then_some(new_block_id),
            updated_references,
            changed_notes,
        })
    }

    pub fn rename_heading(
        &self,
        note: &str,
        old_heading: &str,
        new_heading: &str,
        dry_run: bool,
    ) -> anyhow::Result<RenameResult> {
        let notes = self.queries.index_notes()?;
        let target = find_indexed_note(note, &notes)?;
        let heading = target
            .parsed
            .headings
            .iter()
            .find(|heading| heading.text == old_heading || heading.anchor == old_heading)
            .ok_or_else(|| anyhow::anyhow!("heading not found: {old_heading}"))?;
        let mut edits: BTreeMap<String, Vec<TextEdit>> = BTreeMap::new();
        let target_content = std::fs::read_to_string(&target.file.path)?;
        let heading_line = source_for_line(
            &target.file.relative_path,
            &target_content,
            &target.parsed,
            heading.source.line_start,
            heading.source.line_start,
        );
        edits
            .entry(target.file.relative_path.clone())
            .or_default()
            .push(TextEdit {
                start: heading_line.byte_start,
                end: heading_line.byte_end,
                replacement: format!(
                    "{} {}\n",
                    "#".repeat(usize::from(heading.level)),
                    new_heading
                ),
            });

        let mut updated_references = 0;
        for source_note in &notes {
            for link in &source_note.parsed.links {
                if !matches!(link.kind, LinkKind::Wikilink)
                    || !heading_reference_matches(&link.reference, old_heading)
                    || !link_targets_note(
                        link.target.as_str(),
                        source_note.file.relative_path.as_str(),
                        target.file.relative_path.as_str(),
                        &notes,
                    )
                {
                    continue;
                }
                let replacement = replace_heading_reference(&link.raw, old_heading, new_heading);
                if replacement == link.raw {
                    continue;
                }
                edits
                    .entry(source_note.file.relative_path.clone())
                    .or_default()
                    .push(TextEdit {
                        start: link.source.byte_start,
                        end: link.source.byte_end,
                        replacement,
                    });
                updated_references += 1;
            }
        }

        let changed_notes = edits.keys().cloned().collect();
        if !dry_run {
            for (path, note_edits) in edits {
                let indexed = find_indexed_note(&path, &notes)?;
                let content = std::fs::read_to_string(&indexed.file.path)?;
                let updated = apply_edits(content, note_edits)?;
                self.write_note_atomic(&indexed.file.path, &indexed.file.relative_path, &updated)?;
            }
        }
        Ok(RenameResult {
            dry_run,
            updated_references,
            changed_notes,
        })
    }
}

fn block_id_insertion_point(
    content: &str,
    block_start: usize,
    block_end: usize,
) -> anyhow::Result<usize> {
    let raw = content
        .get(block_start..block_end)
        .ok_or_else(|| anyhow::anyhow!("block source range is invalid"))?;
    let trailing_line_breaks = raw.len() - raw.trim_end_matches(['\r', '\n']).len();
    Ok(block_end - trailing_line_breaks)
}

fn block_id_deletion_start(
    content: &str,
    block_start: usize,
    marker_start: usize,
) -> anyhow::Result<usize> {
    let prefix = content
        .get(block_start..marker_start)
        .ok_or_else(|| anyhow::anyhow!("block id source range is invalid"))?;
    if prefix.ends_with("\r\n") {
        Ok(marker_start - 2)
    } else if prefix.ends_with(['\n', ' ']) {
        Ok(marker_start - 1)
    } else {
        Ok(marker_start)
    }
}

fn block_id_byte_start(
    content: &str,
    block_start: usize,
    block_end: usize,
    block_id: &str,
) -> anyhow::Result<usize> {
    let raw = content
        .get(block_start..block_end)
        .ok_or_else(|| anyhow::anyhow!("block source range is invalid"))?;
    let marker = format!("^{block_id}");
    raw.rfind(&marker)
        .map(|offset| block_start + offset)
        .ok_or_else(|| anyhow::anyhow!("block id source not found: {block_id}"))
}

fn heading_reference_matches(reference: &Option<ReferenceInfo>, old: &str) -> bool {
    match reference {
        Some(ReferenceInfo::Heading { value }) => value == old,
        Some(ReferenceInfo::MultiHeading { value }) => value.last().is_some_and(|last| last == old),
        Some(ReferenceInfo::BlockId { .. }) | None => false,
    }
}

pub(super) fn link_targets_note(
    target: &str,
    source_path: &str,
    wanted_path: &str,
    notes: &[crate::resolver::IndexedNote],
) -> bool {
    if target.is_empty() {
        return source_path == wanted_path;
    }
    matches!(RefResolver::resolve(target, notes), ResolveResult::Resolved { path, .. } if path == wanted_path)
}

fn replace_heading_reference(raw: &str, old: &str, new: &str) -> String {
    raw.replacen(&format!("#{old}"), &format!("#{new}"), 1)
}

fn replace_link_target(raw: &str, new_path: &str) -> String {
    let Some(inner) = raw
        .strip_prefix("[[")
        .and_then(|value| value.strip_suffix("]]"))
    else {
        return raw.to_string();
    };
    let suffix_start = inner.find(['#', '|']).unwrap_or(inner.len());
    format!("[[{new_path}{}]]", &inner[suffix_start..])
}

fn apply_edits(mut content: String, mut edits: Vec<TextEdit>) -> anyhow::Result<String> {
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.start));
    for edit in edits {
        if edit.end > content.len() || edit.start > edit.end {
            return Err(anyhow::anyhow!("rename edit range is invalid"));
        }
        content.replace_range(edit.start..edit.end, &edit.replacement);
    }
    Ok(content)
}
