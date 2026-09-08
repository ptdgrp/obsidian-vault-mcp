use crate::parser::{
    HeadingInfo, ParsedNote, ReferenceInfo, SectionInfo, SourceSpan, byte_offset_for_line,
    source_for_line,
};

use super::{SectionSelector, levenshtein_distance::levenshtein_distance};

#[tracing::instrument(name = "vault.select_section", skip_all, err)]
pub(crate) fn section_source(
    relative_path: &str,
    content: &str,
    parsed: &ParsedNote,
    selector: &SectionSelector,
) -> anyhow::Result<SourceSpan> {
    let total_lines = content.lines().count().max(1) as u64;
    let (line_start, line_end) = match selector {
        SectionSelector::Heading { heading } => {
            return heading_source(relative_path, content, &parsed.headings, heading);
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

/// Select a heading section using only heading metadata, shared with full-note selection.
pub(crate) fn heading_source(
    relative_path: &str,
    content: &str,
    headings: &[HeadingInfo],
    heading: &str,
) -> anyhow::Result<SourceSpan> {
    let requested = ParsedHeadingSelector::parse(heading);
    let current = find_heading(headings, &requested)
        .ok_or_else(|| anyhow::anyhow!("{}", heading_not_found_message(heading, headings)))?;
    let line_start = current.source.line_start;
    let line_end = headings
        .iter()
        .filter(|candidate| {
            candidate.source.line_start > line_start && candidate.level <= current.level
        })
        .map(|candidate| candidate.source.line_start.saturating_sub(1))
        .min()
        .unwrap_or(content.lines().count().max(1) as u64)
        .max(line_start);
    Ok(SourceSpan {
        path: relative_path.to_string(),
        line_start,
        line_end,
        byte_start: byte_offset_for_line(content, line_start),
        byte_end: byte_offset_for_line(content, line_end.saturating_add(1)),
        section: Some(SectionInfo {
            heading: current.text.clone(),
            heading_level: current.level,
            heading_path: current.path.clone(),
            heading_anchor: current.anchor.clone(),
        }),
    })
}

pub(crate) fn selector_from_reference(
    reference: &Option<ReferenceInfo>,
) -> anyhow::Result<Option<SectionSelector>> {
    match reference {
        None => Ok(None),
        Some(ReferenceInfo::BlockId { value }) => Ok(Some(SectionSelector::Block {
            block_id: value.clone(),
        })),
        Some(ReferenceInfo::Heading { value }) => selector_from_fragment(value),
        Some(ReferenceInfo::MultiHeading { value }) => Ok(Some(SectionSelector::Heading {
            heading: value.join("/"),
        })),
    }
}

fn selector_from_fragment(fragment: &str) -> anyhow::Result<Option<SectionSelector>> {
    let Some(line_fragment) = fragment.strip_prefix('L') else {
        return Ok(Some(SectionSelector::Heading {
            heading: fragment.to_string(),
        }));
    };

    let Some((start, end)) = line_fragment.split_once('-') else {
        return parse_line_selector(line_fragment, line_fragment);
    };
    let end = if end.is_empty() {
        u64::MAX.to_string()
    } else {
        end.strip_prefix('L').unwrap_or(end).to_string()
    };
    if start.is_empty() || !end.chars().all(char::is_numeric) {
        return Ok(Some(SectionSelector::Heading {
            heading: fragment.to_string(),
        }));
    }
    parse_line_selector(start, &end)
}

fn parse_line_selector(start: &str, end: &str) -> anyhow::Result<Option<SectionSelector>> {
    if !start.chars().all(char::is_numeric) || !end.chars().all(char::is_numeric) {
        return Ok(Some(SectionSelector::Heading {
            heading: format!("L{start}"),
        }));
    }
    let line_start = start
        .parse::<u64>()
        .map_err(|_| anyhow::anyhow!("invalid line range"))?;
    let line_end = end
        .parse::<u64>()
        .map_err(|_| anyhow::anyhow!("invalid line range"))?;
    if line_start == 0 || line_end < line_start {
        anyhow::bail!("invalid line range");
    }
    Ok(Some(SectionSelector::Lines {
        line_start,
        line_end,
    }))
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

pub(crate) fn find_heading<'a>(
    headings: &'a [HeadingInfo],
    requested_heading: &ParsedHeadingSelector<'_>,
) -> Option<&'a HeadingInfo> {
    headings
        .iter()
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

pub(crate) fn heading_not_found_message(heading: &str, headings: &[HeadingInfo]) -> String {
    let suggestions = closest_heading_suggestions(heading, headings);

    if suggestions.is_empty() {
        format!("heading not found: {heading:?}. Note has no headings.")
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

fn closest_heading_suggestions(heading: &str, headings: &[HeadingInfo]) -> Vec<String> {
    let requested_heading = ParsedHeadingSelector::parse(heading);
    let mut best_distance = usize::MAX;
    let mut suggestions = Vec::new();

    for candidate in headings {
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
