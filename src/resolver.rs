use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use crate::parser::{ParsedNote, ReferenceInfo};
use crate::vault::NoteFile;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ObsidianRef {
    pub raw: String,
    pub target: String,
    pub reference: Option<ReferenceInfo>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ResolveResult {
    Resolved {
        reference: ObsidianRef,
        path: String,
        heading: Option<String>,
        block_id: Option<String>,
    },
    Ambiguous {
        reference: ObsidianRef,
        candidates: Vec<ResolveCandidate>,
    },
    Unresolved {
        reference: ObsidianRef,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ResolveCandidate {
    pub path: String,
    pub match_kind: String,
}

#[derive(Clone, Debug)]
pub struct IndexedNote {
    pub file: NoteFile,
    pub parsed: Arc<ParsedNote>,
}

#[derive(Clone, Debug, Default)]
pub struct RefResolver {}

impl RefResolver {
    #[tracing::instrument(name = "vault.parse_ref")]
    pub fn parse_ref(input: &str) -> ObsidianRef {
        let mut raw = input.trim().to_string();
        if raw.starts_with("![[") && raw.ends_with("]]") {
            raw = raw[3..raw.len() - 2].to_string();
        } else if raw.starts_with("[[") && raw.ends_with("]]") {
            raw = raw[2..raw.len() - 2].to_string();
        }

        let target_part = raw.split('|').next().unwrap_or(&raw).trim();
        let (target, reference) = if let Some((target, fragment)) = target_part.split_once("#^") {
            (
                target.trim().to_string(),
                Some(ReferenceInfo::BlockId {
                    value: fragment.trim().to_string(),
                }),
            )
        } else if let Some((target, fragment)) = target_part.split_once('#') {
            let headings: Vec<String> = fragment
                .split('#')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(ToOwned::to_owned)
                .collect();
            let reference = match headings.as_slice() {
                [] => None,
                [heading] => Some(ReferenceInfo::Heading {
                    value: heading.clone(),
                }),
                _ => Some(ReferenceInfo::MultiHeading { value: headings }),
            };
            (target.trim().to_string(), reference)
        } else {
            (target_part.to_string(), None)
        };

        ObsidianRef {
            raw: input.to_string(),
            target,
            reference,
        }
    }

    pub fn resolve(reference: &str, notes: &[IndexedNote]) -> ResolveResult {
        let mut parsed_ref = Self::parse_ref(reference);
        let mut candidates = find_candidates(&parsed_ref.target, notes);
        candidates.sort_by(|a, b| natord::compare(&a.path, &b.path));
        candidates.dedup_by(|a, b| a.path == b.path);
        match candidates.as_slice() {
            [] => ResolveResult::Unresolved {
                reference: parsed_ref,
            },
            [candidate] => {
                let Some(note) = notes
                    .iter()
                    .find(|note| note.file.relative_path == candidate.path)
                else {
                    return ResolveResult::Unresolved {
                        reference: parsed_ref,
                    };
                };
                let Some(canonical_reference) =
                    Self::canonical_reference(note, &parsed_ref.reference)
                else {
                    return ResolveResult::Unresolved {
                        reference: parsed_ref,
                    };
                };
                parsed_ref.reference = canonical_reference;
                ResolveResult::Resolved {
                    heading: reference_heading(&parsed_ref.reference),
                    block_id: reference_block_id(&parsed_ref.reference),
                    path: candidate.path.clone(),
                    reference: parsed_ref,
                }
            }
            _ => ResolveResult::Ambiguous {
                reference: parsed_ref,
                candidates,
            },
        }
    }

    pub(crate) fn reference_exists(note: &IndexedNote, reference: &Option<ReferenceInfo>) -> bool {
        Self::canonical_reference(note, reference).is_some()
    }

    fn canonical_reference(
        note: &IndexedNote,
        reference: &Option<ReferenceInfo>,
    ) -> Option<Option<ReferenceInfo>> {
        match reference {
            None => Some(None),
            Some(ReferenceInfo::BlockId { value }) => note
                .parsed
                .blocks
                .iter()
                .any(|block| block.id == *value)
                .then(|| {
                    Some(ReferenceInfo::BlockId {
                        value: value.clone(),
                    })
                }),
            Some(ReferenceInfo::Heading { value }) => {
                note.parsed.headings.iter().find_map(|heading| {
                    (heading.level != 1 && heading_matches(value, heading))
                        .then(|| canonical_heading_reference(&heading.path))
                })
            }
            Some(ReferenceInfo::MultiHeading { value }) => {
                note.parsed.headings.iter().find_map(|heading| {
                    (heading.level != 1 && heading_path_matches(value, &heading.path))
                        .then(|| canonical_heading_reference(&heading.path))
                })
            }
        }
    }
}

fn canonical_heading_reference(path: &[String]) -> Option<ReferenceInfo> {
    match path {
        [] => None,
        [heading] => Some(ReferenceInfo::Heading {
            value: heading.clone(),
        }),
        _ => Some(ReferenceInfo::MultiHeading {
            value: path.to_vec(),
        }),
    }
}

fn heading_matches(requested: &str, heading: &crate::parser::HeadingInfo) -> bool {
    comparable_heading_text(&heading.text) == comparable_heading_text(requested)
        || comparable_heading_text(&heading.anchor) == comparable_heading_text(requested)
        || comparable_heading_text(&heading.path.join("/")) == comparable_heading_text(requested)
        || comparable_heading_text(&heading.path.join(" / ")) == comparable_heading_text(requested)
}

fn heading_path_matches(requested: &[String], candidate: &[String]) -> bool {
    requested.len() == candidate.len()
        && requested
            .iter()
            .zip(candidate)
            .all(|(left, right)| comparable_heading_text(left) == comparable_heading_text(right))
}

fn find_candidates(target: &str, notes: &[IndexedNote]) -> Vec<ResolveCandidate> {
    let mut out = Vec::new();
    if target.contains('/') || target.ends_with(".md") {
        let wanted_path = normalize_key(target);
        for note in notes {
            if normalize_key(&note.file.relative_path) == wanted_path {
                out.push(ResolveCandidate {
                    path: note.file.relative_path.clone(),
                    match_kind: "path".to_string(),
                });
            }
        }
        return out;
    }

    let clean_target = target.trim_end_matches(".md");
    let wanted = normalize_key(clean_target);
    let mut exact = Vec::new();
    for note in notes {
        let rel = note.file.relative_path.trim_end_matches(".md");
        if normalize_key(rel) == wanted || normalize_key(&note.file.relative_path) == wanted {
            exact.push(ResolveCandidate {
                path: note.file.relative_path.clone(),
                match_kind: "path".to_string(),
            });
            continue;
        }
        if note
            .file
            .path
            .file_stem()
            .is_some_and(|stem| normalize_key(stem) == wanted)
        {
            exact.push(ResolveCandidate {
                path: note.file.relative_path.clone(),
                match_kind: "stem".to_string(),
            });
        }
    }
    if !exact.is_empty() {
        return exact;
    }

    let mut numbered = Vec::new();
    for note in notes {
        if note
            .file
            .path
            .file_stem()
            .and_then(strip_numeric_sort_prefix)
            .is_some_and(|stem| normalize_key(stem) == wanted)
        {
            numbered.push(ResolveCandidate {
                path: note.file.relative_path.clone(),
                match_kind: "numbered_stem".to_string(),
            });
        }
    }
    if !numbered.is_empty() {
        return numbered;
    }

    for note in notes {
        if aliases(&note.parsed)
            .iter()
            .any(|alias| normalize_key(alias) == wanted)
        {
            out.push(ResolveCandidate {
                path: note.file.relative_path.clone(),
                match_kind: "alias".to_string(),
            });
        }
    }
    out
}

fn strip_numeric_sort_prefix(stem: &str) -> Option<&str> {
    let (prefix, rest) = stem.split_once('-')?;
    if is_sort_prefix(prefix) && !rest.is_empty() {
        Some(rest)
    } else {
        None
    }
}

fn is_sort_prefix(prefix: &str) -> bool {
    if prefix.is_empty() {
        return false;
    }
    let digit_start = prefix
        .find(|ch: char| ch.is_ascii_digit())
        .unwrap_or(prefix.len());
    let (label, number) = prefix.split_at(digit_start);
    !number.is_empty()
        && number.chars().all(|ch| ch.is_ascii_digit())
        && label.chars().all(|ch| ch.is_ascii_alphabetic())
}

fn aliases(parsed: &ParsedNote) -> Vec<String> {
    let Some(frontmatter) = &parsed.frontmatter else {
        return Vec::new();
    };
    let Some(value) = frontmatter
        .get("aliases")
        .or_else(|| frontmatter.get("alias"))
    else {
        return Vec::new();
    };
    match value {
        serde_json::Value::String(value) => vec![value.clone()],
        serde_json::Value::Array(values) => values
            .iter()
            .filter_map(|value| value.as_str().map(ToOwned::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

fn reference_heading(reference: &Option<ReferenceInfo>) -> Option<String> {
    match reference {
        Some(ReferenceInfo::Heading { value }) => Some(value.clone()),
        Some(ReferenceInfo::MultiHeading { value }) => value.last().cloned(),
        _ => None,
    }
}

fn reference_block_id(reference: &Option<ReferenceInfo>) -> Option<String> {
    match reference {
        Some(ReferenceInfo::BlockId { value }) => Some(value.clone()),
        _ => None,
    }
}

fn normalize_key(input: &str) -> String {
    input
        .trim()
        .trim_end_matches(".md")
        .nfc()
        .collect::<String>()
        .to_lowercase()
}

fn comparable_heading_text(value: &str) -> &str {
    let trimmed = value.trim();
    let without_colon = trimmed
        .strip_suffix(':')
        .or_else(|| trimmed.strip_suffix('：'))
        .unwrap_or(trimmed);
    without_colon.trim_end()
}

#[cfg(test)]
mod tests;
