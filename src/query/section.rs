use std::fs;

use crate::parser::{slice_text, source_for_line};

use super::{ReadSectionResult, SectionSelector, VaultQueries, truncate_utf8};

impl VaultQueries {
    pub fn read_section(
        &self,
        note: &str,
        selector: SectionSelector,
    ) -> anyhow::Result<ReadSectionResult> {
        let path = self.resolve_note_path(note)?;
        let content = fs::read_to_string(&path)?;
        let relative_path = self.vault.relative_path(&path);
        let parsed = self.parse_file_cached(&path, relative_path.clone())?;
        let total_lines = content.lines().count().max(1) as u64;

        let (line_start, line_end) = match &selector {
            SectionSelector::Heading { heading } => {
                let Some(current) = parsed.headings.iter().find(|candidate| {
                    candidate.text == *heading
                        || candidate.anchor == *heading
                        || candidate.path.join(" / ") == *heading
                }) else {
                    return Err(anyhow::anyhow!("heading not found: {heading}"));
                };
                let next = parsed
                    .headings
                    .iter()
                    .filter(|candidate| {
                        candidate.source.line_start > current.source.line_start
                            && candidate.level <= current.level
                    })
                    .min_by_key(|candidate| candidate.source.line_start)
                    .map(|candidate| candidate.source.line_start.saturating_sub(1))
                    .unwrap_or(total_lines);
                (
                    current.source.line_start,
                    next.max(current.source.line_start),
                )
            }
            SectionSelector::Block { block_id } => {
                let Some(block) = parsed
                    .blocks
                    .iter()
                    .find(|candidate| candidate.id == *block_id)
                else {
                    return Err(anyhow::anyhow!("block id not found: {block_id}"));
                };
                (block.source.line_start, block.source.line_end)
            }
            SectionSelector::Lines {
                line_start,
                line_end,
            } => {
                if line_start == &0 || line_end < line_start {
                    return Err(anyhow::anyhow!("invalid line range"));
                }
                (*line_start, (*line_end).min(total_lines))
            }
        };

        let source = source_for_line(&relative_path, &content, &parsed, line_start, line_end);
        let mut section_content = slice_text(&content, source.byte_start, source.byte_end);
        let mut truncated = false;
        if section_content.len() > self.vault.config.max_output_bytes {
            section_content =
                truncate_utf8(&section_content, self.vault.config.max_output_bytes).to_string();
            truncated = true;
        }

        Ok(ReadSectionResult {
            note: relative_path,
            selector,
            source,
            content: section_content,
            truncated,
        })
    }
}
