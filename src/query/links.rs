use std::collections::{BTreeMap, BTreeSet};

use crate::resolver::{RefResolver, ResolveResult};

use super::{
    BacklinksOutput, BacklinksResult, CompactBacklinksResult, CompactOutlinksResult,
    CompactTagBucket, CompactTagMatch, CompactTagsResult, DetailedSection, DetailedTagBucket,
    DetailedTagOccurrence, DetailedTagsResult, FrontmatterMatch, FrontmatterMatchMode,
    FrontmatterQueryOptions, FrontmatterQueryResult, LinkEvidence, OutlinksOutput, OutlinksResult,
    TagBucket, TagOccurrence, TagSourceKind, TagsOutput, TagsResult, VaultQueries,
    find_indexed_note, read_snippet,
};

impl VaultQueries {
    pub fn get_outlinks(&self, note: &str) -> anyhow::Result<OutlinksResult> {
        let notes = self.index_notes()?;
        let indexed = find_indexed_note(note, &notes)?;
        let links = indexed
            .parsed
            .links
            .iter()
            .map(|link| LinkEvidence {
                source: link.source.clone().into(),
                target: link.target.clone(),
                alias: link.alias.clone(),
                resolved: RefResolver::resolve(&link.target, &notes).into(),
                snippet: read_snippet(&indexed.file, &link.source),
            })
            .collect();
        Ok(OutlinksResult {
            note: indexed.file.relative_path.clone(),
            links,
        })
    }

    pub fn get_outlinks_output(&self, note: &str, verbose: bool) -> anyhow::Result<OutlinksOutput> {
        let result = self.get_outlinks(note)?;
        if verbose {
            Ok(OutlinksOutput::Verbose(result))
        } else {
            Ok(OutlinksOutput::Compact(CompactOutlinksResult::from(result)))
        }
    }

    pub fn get_backlinks(&self, target: &str) -> anyhow::Result<BacklinksResult> {
        let notes = self.index_notes()?;
        let resolution = RefResolver::resolve(target, &notes);
        let wanted_path = match &resolution {
            ResolveResult::Resolved { path, .. } => Some(path.clone()),
            _ => None,
        };
        let mut backlinks = Vec::new();
        for note in &notes {
            for link in &note.parsed.links {
                let matches = wanted_path
                    .as_ref()
                    .is_some_and(|path| RefResolver::link_matches(&link.target, path, &notes))
                    || link.target == target;
                if matches {
                    backlinks.push(LinkEvidence {
                        source: link.source.clone().into(),
                        target: link.target.clone(),
                        alias: link.alias.clone(),
                        resolved: RefResolver::resolve(&link.target, &notes).into(),
                        snippet: read_snippet(&note.file, &link.source),
                    });
                }
            }
        }
        backlinks.sort_by(|a, b| {
            natord::compare(&a.source.path, &b.source.path)
                .then(a.source.line_start.cmp(&b.source.line_start))
        });
        let truncated = backlinks.len() > self.vault.config.max_results;
        backlinks.truncate(self.vault.config.max_results);
        Ok(BacklinksResult {
            target: target.to_string(),
            resolution: resolution.into(),
            backlinks,
            truncated,
        })
    }

    pub fn get_backlinks_output(
        &self,
        target: &str,
        verbose: bool,
    ) -> anyhow::Result<BacklinksOutput> {
        let result = self.get_backlinks(target)?;
        if verbose {
            Ok(BacklinksOutput::Verbose(result))
        } else {
            Ok(BacklinksOutput::Compact(CompactBacklinksResult::from(
                result,
            )))
        }
    }

    pub fn list_tags(&self, tag: Option<&str>) -> anyhow::Result<TagsResult> {
        let mut buckets: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut occurrences: BTreeMap<String, Vec<TagOccurrence>> = BTreeMap::new();
        for note in self.index_notes()? {
            for found in &note.parsed.tags {
                if tag_matches(tag, &found.tag) {
                    buckets
                        .entry(found.tag.clone())
                        .or_default()
                        .insert(note.file.relative_path.clone());
                    occurrences
                        .entry(found.tag.clone())
                        .or_default()
                        .push(TagOccurrence {
                            note: note.file.relative_path.clone(),
                            source_kind: TagSourceKind::Body,
                            source: Some(found.source.clone().into()),
                        });
                }
            }
            for found in frontmatter_tags(note.parsed.frontmatter.as_ref()) {
                if tag_matches(tag, &found) {
                    buckets
                        .entry(found.clone())
                        .or_default()
                        .insert(note.file.relative_path.clone());
                    occurrences.entry(found).or_default().push(TagOccurrence {
                        note: note.file.relative_path.clone(),
                        source_kind: TagSourceKind::Frontmatter,
                        source: None,
                    });
                }
            }
        }
        Ok(TagsResult {
            tags: buckets
                .into_iter()
                .map(|(tag, notes)| TagBucket {
                    occurrences: occurrences.remove(&tag).unwrap_or_default(),
                    tag,
                    notes: notes.into_iter().collect(),
                })
                .collect(),
        })
    }

    pub fn list_tags_output(&self, tag: Option<&str>, verbose: bool) -> anyhow::Result<TagsOutput> {
        let result = self.list_tags(tag)?;
        if verbose {
            Ok(TagsOutput::Verbose(detailed_tags(result)))
        } else {
            Ok(TagsOutput::Compact(compact_tags(result)))
        }
    }

    pub fn query_frontmatter(
        &self,
        options: FrontmatterQueryOptions,
    ) -> anyhow::Result<FrontmatterQueryResult> {
        let matcher = match options.mode {
            FrontmatterMatchMode::Exists => None,
            FrontmatterMatchMode::Equals => {
                Some(MetadataMatcher::Equals(options.value.clone().ok_or_else(
                    || anyhow::anyhow!("frontmatter equals query requires a value"),
                )?))
            }
            FrontmatterMatchMode::Regex => Some(MetadataMatcher::Regex(regex::Regex::new(
                options
                    .value
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("frontmatter regex query requires a value"))?,
            )?)),
        };

        let mut matches = Vec::new();
        for note in self.index_notes()? {
            let Some(frontmatter) = note.parsed.frontmatter.as_ref() else {
                continue;
            };
            let Some(value) = frontmatter.get(&options.field) else {
                continue;
            };
            if matcher
                .as_ref()
                .is_none_or(|matcher| matcher.matches(value))
            {
                matches.push(FrontmatterMatch {
                    note: note.file.relative_path.clone(),
                    value: value.clone(),
                });
            }
        }

        matches.sort_by(|a, b| natord::compare(&a.note, &b.note));
        let truncated = matches.len() > self.vault.config.max_results;
        matches.truncate(self.vault.config.max_results);
        Ok(FrontmatterQueryResult {
            field: options.field,
            mode: options.mode,
            value: options.value,
            matches,
            truncated,
        })
    }
}

enum MetadataMatcher {
    Equals(String),
    Regex(regex::Regex),
}

impl MetadataMatcher {
    fn matches(&self, value: &serde_json::Value) -> bool {
        match self {
            MetadataMatcher::Equals(wanted) => {
                value_strings(value).iter().any(|item| item == wanted)
            }
            MetadataMatcher::Regex(regex) => {
                value_strings(value).iter().any(|item| regex.is_match(item))
            }
        }
    }
}

fn tag_matches(wanted: Option<&str>, found: &str) -> bool {
    wanted.is_none_or(|wanted| normalize_tag(wanted) == normalize_tag(found))
}

fn frontmatter_tags(frontmatter: Option<&serde_json::Value>) -> Vec<String> {
    let Some(frontmatter) = frontmatter else {
        return Vec::new();
    };
    ["tags", "tag"]
        .iter()
        .filter_map(|field| frontmatter.get(field))
        .flat_map(tag_values)
        .map(|tag| normalize_tag(&tag))
        .filter(|tag| !tag.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn tag_values(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::String(value) => value
            .split(|char: char| char.is_whitespace() || char == ',')
            .filter(|part| !part.trim().is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        serde_json::Value::Array(values) => values
            .iter()
            .filter_map(|value| value.as_str().map(ToOwned::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

fn normalize_tag(tag: &str) -> String {
    tag.trim().trim_start_matches('#').to_string()
}

fn compact_tags(result: TagsResult) -> CompactTagsResult {
    CompactTagsResult {
        tags: result
            .tags
            .into_iter()
            .map(|bucket| {
                let tag = bucket.tag.clone();
                CompactTagBucket {
                    tag,
                    notes: compact_tag_matches(bucket),
                }
            })
            .collect(),
    }
}

fn compact_tag_matches(bucket: TagBucket) -> Vec<CompactTagMatch> {
    if bucket.occurrences.is_empty() {
        return bucket
            .notes
            .into_iter()
            .map(|note| CompactTagMatch {
                note,
                source_kind: TagSourceKind::Body,
                section: None,
            })
            .collect();
    }

    bucket
        .occurrences
        .into_iter()
        .map(|occurrence| {
            let section = occurrence
                .source
                .as_ref()
                .and_then(|source| source.section.as_ref())
                .map(|section| section.heading_path.join(" / "));
            let note = occurrence
                .source
                .as_ref()
                .map(|source| format!("{}:{}", source.path, source.line_start))
                .unwrap_or(occurrence.note);
            CompactTagMatch {
                note,
                source_kind: occurrence.source_kind,
                section,
            }
        })
        .collect()
}

fn detailed_tags(result: TagsResult) -> DetailedTagsResult {
    DetailedTagsResult {
        tags: result
            .tags
            .into_iter()
            .map(|bucket| DetailedTagBucket {
                tag: bucket.tag,
                occurrences: detailed_tag_occurrences(bucket.occurrences),
            })
            .collect(),
    }
}

fn detailed_tag_occurrences(occurrences: Vec<TagOccurrence>) -> Vec<DetailedTagOccurrence> {
    occurrences
        .into_iter()
        .map(|occurrence| {
            let location = occurrence
                .source
                .as_ref()
                .map(source_location)
                .unwrap_or_else(|| occurrence.note.clone());
            let section = occurrence
                .source
                .and_then(|source| source.section.map(detailed_section));
            DetailedTagOccurrence {
                location,
                source_kind: occurrence.source_kind,
                section,
            }
        })
        .collect()
}

fn detailed_section(section: crate::parser::SectionInfo) -> DetailedSection {
    let heading_path =
        (section.heading_path != vec![section.heading.clone()]).then_some(section.heading_path);
    DetailedSection {
        heading: section.heading,
        heading_level: section.heading_level,
        heading_path,
    }
}

fn source_location(source: &super::SearchSource) -> String {
    if source.line_start == source.line_end {
        format!("{}:{}", source.path, source.line_start)
    } else {
        format!("{}:{}-{}", source.path, source.line_start, source.line_end)
    }
}

fn value_strings(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::Null => vec!["null".to_string()],
        serde_json::Value::Bool(value) => vec![value.to_string()],
        serde_json::Value::Number(value) => vec![value.to_string()],
        serde_json::Value::String(value) => vec![value.clone()],
        serde_json::Value::Array(values) => values.iter().flat_map(value_strings).collect(),
        serde_json::Value::Object(_) => vec![value.to_string()],
    }
}
