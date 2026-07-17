use super::{EditSectionResult, VaultMutations};

use crate::query::{SectionSelector, public::Locator, section::section_source};

impl VaultMutations {
    pub fn append_section(
        &self,
        note: &str,
        selector: SectionSelector,
        content: &str,
    ) -> anyhow::Result<EditSectionResult> {
        let (path, relative_path, document, source) = self.section_document(note, &selector)?;
        let mut updated = document;
        updated.insert_str(source.byte_end, content);
        self.write_section_edit(
            &path,
            &relative_path,
            &updated,
            source.line_end + 1,
            source.line_end + content.lines().count() as u64,
        )
    }

    pub fn replace_section(
        &self,
        note: &str,
        selector: SectionSelector,
        content: &str,
    ) -> anyhow::Result<EditSectionResult> {
        let (path, relative_path, document, source) = self.section_document(note, &selector)?;
        let mut updated = document;
        updated.replace_range(source.byte_start..source.byte_end, content);
        self.write_section_edit(
            &path,
            &relative_path,
            &updated,
            source.line_start,
            source.line_start + content.lines().count().saturating_sub(1) as u64,
        )
    }

    pub fn delete_section(
        &self,
        note: &str,
        selector: SectionSelector,
    ) -> anyhow::Result<EditSectionResult> {
        let (path, relative_path, document, source) = self.section_document(note, &selector)?;
        let mut updated = document;
        updated.replace_range(source.byte_start..source.byte_end, "");
        self.write_section_edit(
            &path,
            &relative_path,
            &updated,
            source.line_start,
            source.line_start,
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
        let (parsed, content) = self.queries.parse_note_3(&path, &relative_path)?;
        let source = section_source(&relative_path, &content, &parsed, selector)?;
        Ok((path, relative_path, content, source))
    }

    fn write_section_edit(
        &self,
        path: &camino::Utf8Path,
        relative_path: &str,
        content: &str,
        line_start: u64,
        line_end: u64,
    ) -> anyhow::Result<EditSectionResult> {
        self.write_note_atomic(path, relative_path, content)?;
        let max_line = content.lines().count().max(1) as u64;
        let line_start = line_start.clamp(1, max_line);
        let line_end = line_end.clamp(line_start, max_line);
        Ok(EditSectionResult {
            changed: Locator::lines(relative_path, line_start, line_end),
        })
    }
}
