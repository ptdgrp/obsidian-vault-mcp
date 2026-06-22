use std::fs;

use super::{EditSectionResult, VaultMutations};

use crate::query::{SectionSelector, section::section_source};

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
        let document = fs::read_to_string(&path)?;
        let parsed = self
            .queries
            .parse_file_cached(&path, relative_path.clone())?;
        let source = section_source(&relative_path, &document, &parsed, selector)?;
        Ok((path, relative_path, document, source))
    }

    fn write_section_edit(
        &self,
        path: &camino::Utf8Path,
        relative_path: &str,
        content: &str,
        line_start: u64,
        line_end: u64,
    ) -> anyhow::Result<EditSectionResult> {
        self.queries.vault.write_note_atomic(path, content)?;
        self.queries.parse_cache.invalidate(relative_path);
        Ok(EditSectionResult {
            note: relative_path.to_string(),
            line_start,
            line_end: line_end.max(line_start),
        })
    }
}
