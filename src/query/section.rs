use std::fs;

use crate::parser::{HeadingInfo, ParsedNote, SourceSpan, slice_text, source_for_line};

use super::{
    ReadSectionResult, SectionSelector, VaultQueries, edit_distance::levenshtein_distance,
    truncate_utf8,
};

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
        let source = section_source(&relative_path, &content, &parsed, &selector)?;
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

    pub(crate) fn read_section_selector_hint(&self, note: &str) -> anyhow::Result<String> {
        let path = self.resolve_note_path(note)?;
        let relative_path = self.vault.relative_path(&path);
        let parsed = self.parse_file_cached(&path, relative_path.clone())?;

        Ok(section_selector_hint_message(&relative_path, &parsed))
    }
}

pub(crate) fn section_source(
    relative_path: &str,
    content: &str,
    parsed: &ParsedNote,
    selector: &SectionSelector,
) -> anyhow::Result<SourceSpan> {
    let total_lines = content.lines().count().max(1) as u64;
    let (line_start, line_end) = match selector {
        SectionSelector::Heading { heading } => {
            let requested_heading = ParsedHeadingSelector::parse(heading);
            let Some(current) = find_selectable_heading(parsed, &requested_heading) else {
                return Err(anyhow::anyhow!(
                    "{}",
                    heading_not_found_message(heading, parsed)
                ));
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

    Ok(source_for_line(
        relative_path,
        content,
        parsed,
        line_start,
        line_end,
    ))
}

pub(crate) struct ParsedHeadingSelector<'a> {
    text: &'a str,
    comparable_text: &'a str,
    level: Option<u8>,
}

impl<'a> ParsedHeadingSelector<'a> {
    pub(crate) fn parse(heading: &'a str) -> Self {
        let trimmed = heading.trim();
        let marker_len = trimmed.bytes().take_while(|byte| *byte == b'#').count();
        let Some(level) = markdown_heading_level(marker_len) else {
            return Self {
                text: trimmed,
                comparable_text: comparable_heading_text(trimmed),
                level: None,
            };
        };

        let rest = &trimmed[marker_len..];
        if rest.chars().next().is_some_and(char::is_whitespace) {
            let text = rest.trim_start();
            Self {
                text,
                comparable_text: comparable_heading_text(text),
                level: Some(level),
            }
        } else {
            Self {
                text: trimmed,
                comparable_text: comparable_heading_text(trimmed),
                level: None,
            }
        }
    }

    fn matches(&self, candidate: &HeadingInfo) -> bool {
        if self.level.is_some_and(|level| candidate.level != level) {
            return false;
        }

        candidate_text_matches(candidate.text.as_str(), self)
            || candidate_text_matches(candidate.anchor.as_str(), self)
            || candidate_text_matches(&candidate.path.join("/"), self)
            || candidate_text_matches(&candidate.path.join(" / "), self)
    }
}

pub(crate) fn find_selectable_heading<'a>(
    parsed: &'a ParsedNote,
    requested_heading: &ParsedHeadingSelector<'_>,
) -> Option<&'a HeadingInfo> {
    parsed
        .headings
        .iter()
        .filter(|heading| heading.level != 1)
        .find(|candidate| requested_heading.matches(candidate))
}

fn candidate_text_matches(candidate: &str, requested: &ParsedHeadingSelector<'_>) -> bool {
    candidate == requested.text || comparable_heading_text(candidate) == requested.comparable_text
}

fn comparable_heading_text(value: &str) -> &str {
    let trimmed = value.trim();
    let without_colon = trimmed
        .strip_suffix(':')
        .or_else(|| trimmed.strip_suffix('：'))
        .unwrap_or(trimmed);
    without_colon.trim_end()
}

fn markdown_heading_level(marker_len: usize) -> Option<u8> {
    match marker_len {
        1 => Some(1),
        2 => Some(2),
        3 => Some(3),
        4 => Some(4),
        5 => Some(5),
        6 => Some(6),
        _ => None,
    }
}

pub(crate) fn heading_not_found_message(heading: &str, parsed: &ParsedNote) -> String {
    let suggestions = closest_heading_suggestions(heading, parsed);

    if suggestions.is_empty() {
        if parsed.headings.iter().any(|it| it.level == 1) {
            format!(
                "heading not found: {heading:?}. Note has no selectable headings; level-one headings are note titles. Use a lower-level heading, block id, or line selector."
            )
        } else {
            format!("heading not found: {heading:?}. Note has no headings.")
        }
    } else {
        format!(
            "heading not found: {heading:?}. Did you mean: {}?",
            suggestions
                .iter()
                .map(|it| wrapping_char(it, '"'))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn wrapping_char(input: &str, ch: char) -> String {
    format!("{ch}{input}{ch}")
}

fn closest_heading_suggestions(heading: &str, parsed: &ParsedNote) -> Vec<String> {
    let requested_heading = ParsedHeadingSelector::parse(heading);
    let mut best_distance = usize::MAX;
    let mut suggestions = Vec::new();

    for candidate in parsed.headings.iter().filter(|it| it.level != 1) {
        let distance = levenshtein_distance(
            requested_heading.comparable_text,
            comparable_heading_text(candidate.text.as_str()),
        );
        match distance.cmp(&best_distance) {
            std::cmp::Ordering::Less => {
                best_distance = distance;
                suggestions.clear();
                suggestions.push(candidate.text.clone());
            }
            std::cmp::Ordering::Equal => {
                if !suggestions.contains(&candidate.text) {
                    suggestions.push(candidate.text.clone());
                }
            }
            std::cmp::Ordering::Greater => {}
        }
    }

    suggestions
}

fn section_selector_hint_message(relative_path: &str, parsed: &ParsedNote) -> String {
    let headings = parsed
        .headings
        .iter()
        .filter(|it| it.level != 1)
        .take(10)
        .map(|heading| {
            format!(
                "{} {}",
                "#".repeat(usize::from(heading.level)),
                heading.text
            )
        })
        .collect::<Vec<_>>();
    let block_ids = parsed
        .blocks
        .iter()
        .take(10)
        .map(|block| format!("^{}", block.id))
        .collect::<Vec<_>>();

    let mut parts = Vec::new();
    if headings.is_empty() && block_ids.is_empty() {
        parts.push("no headings or block ids found; use a line selector like #L1-L20".to_string());
    } else {
        if !headings.is_empty() {
            parts.push(format!(
                "headings: {}",
                headings
                    .iter()
                    .map(|it| wrapping_char(it.trim_start_matches("#").trim_start(), '"'))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !block_ids.is_empty() {
            parts.push(format!(
                "block_ids: {}",
                block_ids
                    .iter()
                    .map(|it| wrapping_char(it, '"'))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }

    format!(
        "provide exactly one selector: heading, block_id, or line. Available selectors in {relative_path:?}: {}. retry read_section with heading, block_id, or line",
        parts.join("; ")
    )
}
