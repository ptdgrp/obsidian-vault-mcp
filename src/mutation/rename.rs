use std::collections::BTreeMap;

use super::{RenameResult, VaultMutations};
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
                self.queries
                    .vault
                    .write_note_atomic(&indexed.file.path, &updated)?;
                self.queries
                    .parse_cache
                    .invalidate(&indexed.file.relative_path);
            }
            std::fs::rename(&target.file.path, &destination)?;
            self.queries
                .parse_cache
                .invalidate(&target.file.relative_path);
        }
        Ok(RenameResult {
            dry_run,
            updated_references,
            changed_notes,
        })
    }

    pub fn rename_block_id(
        &self,
        note: &str,
        old_block_id: &str,
        new_block_id: &str,
        dry_run: bool,
    ) -> anyhow::Result<RenameResult> {
        let notes = self.queries.index_notes()?;
        let target = find_indexed_note(note, &notes)?;
        let block = target
            .parsed
            .blocks
            .iter()
            .find(|block| block.id == old_block_id)
            .ok_or_else(|| anyhow::anyhow!("block id not found: {old_block_id}"))?;
        let mut edits: BTreeMap<String, Vec<TextEdit>> = BTreeMap::new();
        edits
            .entry(target.file.relative_path.clone())
            .or_default()
            .push(TextEdit {
                start: block.source.byte_start,
                end: block.source.byte_end,
                replacement: format!("^{new_block_id}"),
            });
        let mut updated_references = 0;
        for source_note in &notes {
            for link in &source_note.parsed.links {
                if matches!(&link.reference, Some(ReferenceInfo::BlockId { value }) if value == old_block_id)
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
                                &format!("#^{old_block_id}"),
                                &format!("#^{new_block_id}"),
                                1,
                            ),
                        });
                    updated_references += 1;
                }
            }
        }
        let changed_notes = edits.keys().cloned().collect();
        if !dry_run {
            for (path, note_edits) in edits {
                let indexed = find_indexed_note(&path, &notes)?;
                let updated =
                    apply_edits(std::fs::read_to_string(&indexed.file.path)?, note_edits)?;
                self.queries
                    .vault
                    .write_note_atomic(&indexed.file.path, &updated)?;
                self.queries
                    .parse_cache
                    .invalidate(&indexed.file.relative_path);
            }
        }
        Ok(RenameResult {
            dry_run,
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
                self.queries
                    .vault
                    .write_note_atomic(&indexed.file.path, &updated)?;
                self.queries
                    .parse_cache
                    .invalidate(&indexed.file.relative_path);
            }
        }
        Ok(RenameResult {
            dry_run,
            updated_references,
            changed_notes,
        })
    }
}

fn heading_reference_matches(reference: &Option<ReferenceInfo>, old: &str) -> bool {
    match reference {
        Some(ReferenceInfo::Heading { value }) => value == old,
        Some(ReferenceInfo::MultiHeading { value }) => value.last().is_some_and(|last| last == old),
        Some(ReferenceInfo::BlockId { .. }) | None => false,
    }
}

fn link_targets_note(
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
