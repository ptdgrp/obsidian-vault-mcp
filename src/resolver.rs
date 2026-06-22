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
    pub parsed: ParsedNote,
}

#[derive(Clone, Debug, Default)]
pub struct RefResolver {}

impl RefResolver {
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
        let parsed_ref = Self::parse_ref(reference);
        let mut candidates = find_candidates(&parsed_ref.target, notes);
        candidates.sort_by(|a, b| natord::compare(&a.path, &b.path));
        candidates.dedup_by(|a, b| a.path == b.path);
        match candidates.as_slice() {
            [] => ResolveResult::Unresolved {
                reference: parsed_ref,
            },
            [candidate] => ResolveResult::Resolved {
                heading: reference_heading(&parsed_ref.reference),
                block_id: reference_block_id(&parsed_ref.reference),
                path: candidate.path.clone(),
                reference: parsed_ref,
            },
            _ => ResolveResult::Ambiguous {
                reference: parsed_ref,
                candidates,
            },
        }
    }

    pub fn link_matches(link_target: &str, wanted_path: &str, notes: &[IndexedNote]) -> bool {
        match Self::resolve(link_target, notes) {
            ResolveResult::Resolved { path, .. } => path == wanted_path,
            _ => normalize_key(link_target) == normalize_key(wanted_path),
        }
    }
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
        if !out.is_empty() {
            return out;
        }
    }

    let clean_target = target.trim_end_matches(".md");
    let wanted = normalize_key(clean_target);
    for note in notes {
        let rel = note.file.relative_path.trim_end_matches(".md");
        if normalize_key(rel) == wanted || normalize_key(&note.file.relative_path) == wanted {
            out.push(ResolveCandidate {
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
            out.push(ResolveCandidate {
                path: note.file.relative_path.clone(),
                match_kind: "stem".to_string(),
            });
            continue;
        }
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

#[cfg(test)]
mod tests;
