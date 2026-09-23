use super::history::FileChange;
use super::{EditSectionResult, VaultMutations};

use crate::{
    parser::{NoteParser, byte_offset_for_line},
    query::{SectionSelector, public::Locator, section::section_source},
    resolver::RefResolver,
};

use super::rename::link_targets_note;

impl VaultMutations {
    pub fn append_section(
        &self,
        note: &str,
        selector: SectionSelector,
        content: &str,
    ) -> anyhow::Result<EditSectionResult> {
        let (path, relative_path, document, source) = self.section_document(note, &selector)?;
        let mut updated = document.clone();
        updated.insert_str(source.byte_end, content);
        self.write_section_edit(
            "append_section",
            &path,
            &relative_path,
            &document,
            &updated,
            (
                source.line_end + 1,
                source.line_end + content.lines().count() as u64,
            ),
        )
    }

    pub fn replace_section(
        &self,
        note: &str,
        selector: SectionSelector,
        content: &str,
    ) -> anyhow::Result<EditSectionResult> {
        let (path, relative_path, document, source) = self.section_document(note, &selector)?;
        let mut updated = document.clone();
        let replace_start = if matches!(selector, SectionSelector::Heading { .. }) {
            byte_offset_for_line(&document, source.line_start + 1)
        } else {
            source.byte_start
        };
        let mut replacement = content.to_string();
        if replace_start == document.len()
            && replace_start > 0
            && !document.ends_with('\n')
            && !replacement.is_empty()
        {
            replacement.insert(0, '\n');
        }
        if source.byte_end < document.len()
            && !replacement.is_empty()
            && !replacement.ends_with('\n')
        {
            replacement.push('\n');
        }
        updated.replace_range(replace_start..source.byte_end, &replacement);
        self.check_broken_references(
            &relative_path,
            &updated,
            replace_start,
            source.byte_end,
            "replacement",
        )?;
        self.write_section_edit(
            "replace_section",
            &path,
            &relative_path,
            &document,
            &updated,
            (
                source.line_start + u64::from(matches!(selector, SectionSelector::Heading { .. })),
                source.line_start
                    + u64::from(matches!(selector, SectionSelector::Heading { .. }))
                    + content.lines().count().saturating_sub(1) as u64,
            ),
        )
    }

    fn check_broken_references(
        &self,
        relative_path: &str,
        updated: &str,
        replaced_start: usize,
        replaced_end: usize,
        operation: &str,
    ) -> anyhow::Result<()> {
        let notes = self.queries.index_notes()?;
        let original = notes
            .iter()
            .find(|note| note.file.relative_path == relative_path)
            .ok_or_else(|| anyhow::anyhow!("note not found: {relative_path}"))?;
        let updated_parsed = NoteParser::parse(
            relative_path,
            updated,
            self.queries.vault.config().max_note_bytes,
        )?;
        let mut changed = original.clone();
        changed.parsed = std::sync::Arc::new(updated_parsed);
        let mut broken = Vec::new();
        for source_note in &notes {
            let references =
                source_note
                    .parsed
                    .links
                    .iter()
                    .map(|link| (&link.target, &link.reference, &link.source, &link.raw))
                    .chain(
                        source_note.parsed.embeds.iter().map(|embed| {
                            (&embed.target, &embed.reference, &embed.source, &embed.raw)
                        }),
                    );
            for (target, reference, source, raw) in references {
                let Some(reference) = reference else {
                    continue;
                };
                if source_note.file.relative_path == relative_path
                    && source.byte_start >= replaced_start
                    && source.byte_start < replaced_end
                {
                    continue;
                }
                if link_targets_note(
                    target,
                    &source_note.file.relative_path,
                    relative_path,
                    &notes,
                ) && RefResolver::reference_exists(original, &Some(reference.clone()))
                    && !RefResolver::reference_exists(&changed, &Some(reference.clone()))
                {
                    broken.push(format!("- {}: {}", source.path_with_line_ref(), raw));
                }
            }
        }
        if !broken.is_empty() {
            anyhow::bail!("{operation} would break references:\n{}", broken.join("\n"));
        }
        Ok(())
    }

    pub fn delete_section(
        &self,
        note: &str,
        selector: SectionSelector,
    ) -> anyhow::Result<EditSectionResult> {
        let (path, relative_path, document, source) = self.section_document(note, &selector)?;
        let mut updated = document.clone();
        updated.replace_range(source.byte_start..source.byte_end, "");
        self.check_broken_references(
            &relative_path,
            &updated,
            source.byte_start,
            source.byte_end,
            "deletion",
        )?;
        self.write_section_edit(
            "delete_section",
            &path,
            &relative_path,
            &document,
            &updated,
            (source.line_start, source.line_start),
        )
    }

    fn section_document(
        &self,
        note: &str,
        selector: &SectionSelector,
    ) -> anyhow::Result<(
        camino::Utf8PathBuf,
        String,
        String,
        crate::parser::SourceSpan,
    )> {
        let path = self.queries.resolve_note_path(note)?;
        let relative_path = self.queries.vault.relative_path(&path);
        let (parsed, content) = self.queries.parse_note_from_path(&path, &relative_path)?;
        let source = section_source(&relative_path, &content, &parsed, selector)?;
        Ok((path, relative_path, content, source))
    }

    fn write_section_edit(
        &self,
        operation: &str,
        path: &camino::Utf8Path,
        relative_path: &str,
        original_content: &str,
        content: &str,
        lines: (u64, u64),
    ) -> anyhow::Result<EditSectionResult> {
        if original_content == content {
            anyhow::bail!("section edit produced no changes for `{relative_path}`");
        }
        let current_content = std::fs::read(path)?;
        if current_content != original_content.as_bytes() {
            anyhow::bail!("note changed on disk before section edit: `{relative_path}`");
        }
        self.commit_changes(
            operation,
            vec![FileChange::replace(
                relative_path.to_string(),
                original_content.to_string(),
                content.to_string(),
            )],
        )?;
        let written_content = std::fs::read(path)?;
        if written_content != content.as_bytes() {
            anyhow::bail!("section edit could not be verified on disk: `{relative_path}`");
        }
        let max_line = content.lines().count().max(1) as u64;
        let line_start = lines.0.clamp(1, max_line);
        let line_end = lines.1.clamp(line_start, max_line);
        Ok(EditSectionResult {
            changed: Locator::lines(relative_path, line_start, line_end),
        })
    }
}
