use std::collections::{BTreeMap, BTreeSet};

use crate::parser::{HeadingInfo, LinkInfo, ParsedNote, ReferenceInfo, SourceSpan};
use crate::resolver::{IndexedNote, RefResolver, ResolveResult};

use super::path_filter::PathFilter;
use super::public::{Locator, PageSlice, ResolvedReference};
use super::section::{ParsedHeadingSelector, find_selectable_heading, selector_from_reference};

use super::{
    AmbiguousOutlinkTarget, BacklinkReference, BacklinksPagination, BacklinksResult,
    CompactTagMatch, DetailedSection, DetailedTagOccurrence, FrontmatterMatch,
    FrontmatterMatchMode, FrontmatterQueryOptions, FrontmatterQueryResult, GetTagsResult,
    ListTagsResult, OutlinkTarget, OutlinksPagination, OutlinksResult, TagBucket, TagOccurrence,
    TagOutputBucket, TagScope, TagSourceKind, TagsResult, UnresolvedOutlinkTarget, VaultQueries,
    find_indexed_note, reference_display, reference_suffix, resolved_reference_from_result,
};

const LINK_PAGE_SIZE: usize = 50;

#[derive(Clone, Debug)]
struct OutlinkOccurrence {
    source: String,
    resolution: ResolveResult,
}

#[derive(Clone, Debug)]
struct BacklinkOccurrence {
    source: String,
    target: String,
}

fn backlink_scope_contains(wanted: &ResolvedReference, actual: &ResolvedReference) -> bool {
    wanted.path == actual.path && wanted.block_id.is_none() && wanted.heading_path.is_empty()
        || wanted.contains(actual)
}

impl VaultQueries {
    pub fn get_outlinks(&self, note: &str, page: usize) -> anyhow::Result<OutlinksResult> {
        let notes = self.index_notes()?;
        let indexed = find_indexed_note(note, &notes)?;
        let mut occurrences = indexed
            .parsed
            .links
            .iter()
            .map(|link| {
                let resolution =
                    RefResolver::resolve(&reference_display(&link.target, &link.reference), &notes);
                OutlinkOccurrence {
                    source: Locator::lines(
                        &link.source.path,
                        link.source.line_start,
                        link.source.line_end,
                    ),
                    resolution,
                }
            })
            .collect::<Vec<_>>();
        occurrences.sort_by(|a, b| natord::compare(&a.source, &b.source));
        let slice = PageSlice::new(occurrences, page, LINK_PAGE_SIZE)?;
        let mut targets = Vec::new();
        let mut ambiguous_targets = Vec::new();
        let mut unresolved_targets = Vec::new();
        for occurrence in slice.items() {
            match &occurrence.resolution {
                ResolveResult::Resolved {
                    path, reference, ..
                } => targets.push(OutlinkTarget {
                    source: occurrence.source.clone(),
                    target: format!("{path}{}", reference_suffix(&reference.reference)),
                }),
                ResolveResult::Ambiguous {
                    reference,
                    candidates,
                } => {
                    let suffix = reference_suffix(&reference.reference);
                    let mut candidates = candidates
                        .iter()
                        .map(|candidate| format!("{}{suffix}", candidate.path))
                        .collect::<Vec<_>>();
                    candidates.sort_by(|a, b| natord::compare(a, b));
                    ambiguous_targets.push(AmbiguousOutlinkTarget {
                        source: occurrence.source.clone(),
                        reference: reference_display(&reference.target, &reference.reference),
                        candidates,
                    });
                }
                ResolveResult::Unresolved { reference } => {
                    unresolved_targets.push(UnresolvedOutlinkTarget {
                        source: occurrence.source.clone(),
                        reference: reference_display(&reference.target, &reference.reference),
                    });
                }
            }
        }
        let pagination = slice.pagination();
        Ok(OutlinksResult {
            note: indexed.file.relative_path.clone(),
            targets,
            ambiguous_targets,
            unresolved_targets,
            pagination: OutlinksPagination {
                page: pagination.page,
                total_pages: pagination.total_pages,
                total_links: slice.total_items(),
            },
        })
    }

    pub fn get_backlinks(
        &self,
        target: &str,
        include: &[String],
        exclude: &[String],
        page: usize,
    ) -> anyhow::Result<BacklinksResult> {
        let filter = PathFilter::new(include, exclude)?;
        let notes = self.index_notes()?;
        let resolution = RefResolver::resolve(target, &notes);
        let wanted = match resolved_reference_from_result(&resolution) {
            Some(reference) => reference,
            None => {
                return Err(anyhow::anyhow!(
                    "target must resolve to exactly one note reference: {:?}",
                    target
                ));
            }
        };
        let mut backlinks = Vec::new();
        let mut seen = BTreeSet::new();
        for note in notes
            .iter()
            .filter(|note| filter.is_match(&note.file.relative_path))
        {
            for link in &note.parsed.links {
                let link_ref = reference_display(&link.target, &link.reference);
                let link_resolution = RefResolver::resolve(&link_ref, &notes);
                let Some(actual) = resolved_reference_from_result(&link_resolution) else {
                    continue;
                };
                if backlink_scope_contains(&wanted, &actual) {
                    let source = Locator::lines(
                        &link.source.path,
                        link.source.line_start,
                        link.source.line_end,
                    );
                    let target = actual.format();
                    if seen.insert((source.clone(), target.clone())) {
                        backlinks.push(BacklinkOccurrence { source, target });
                    }
                }
            }
        }
        backlinks.sort_by(|a, b| {
            natord::compare(&a.source, &b.source)
                .then_with(|| natord::compare(&a.target, &b.target))
        });
        let slice = PageSlice::new(backlinks, page, LINK_PAGE_SIZE)?;
        let mut references = Vec::<BacklinkReference>::new();
        for occurrence in slice.items() {
            if let Some(group) = references
                .iter_mut()
                .find(|group| group.target == occurrence.target)
            {
                group.sources.push(occurrence.source.clone());
            } else {
                references.push(BacklinkReference {
                    target: occurrence.target.clone(),
                    sources: vec![occurrence.source.clone()],
                });
            }
        }
        let pagination = slice.pagination();
        Ok(BacklinksResult {
            scope: wanted.format(),
            references,
            pagination: BacklinksPagination {
                page: pagination.page,
                total_pages: pagination.total_pages,
                total_backlinks: slice.total_items(),
            },
        })
    }

    pub(crate) fn backlink_count_for_path(&self, wanted_path: &str) -> anyhow::Result<usize> {
        let notes = self.index_notes()?;
        Ok(count_matching_backlinks(&notes, wanted_path))
    }

    pub(crate) fn backlink_count_for_scope(
        &self,
        wanted_path: &str,
        target: &ParsedNote,
        selector: &super::SectionSelector,
        source: &SourceSpan,
    ) -> anyhow::Result<usize> {
        let notes = self.index_notes()?;
        Ok(notes
            .iter()
            .flat_map(|note| note.parsed.links.iter())
            .filter(|link| RefResolver::link_matches(&link.target, wanted_path, &notes))
            .filter(|link| link_targets_selected_scope(link, target, selector, source))
            .count())
    }

    pub fn list_tags(
        &self,
        scope: TagScope,
        include: &[String],
        exclude: &[String],
    ) -> anyhow::Result<ListTagsResult> {
        let result = self.collect_tags(None, scope, include, exclude)?;
        Ok(ListTagsResult {
            tags: result.tags.into_iter().map(|bucket| bucket.tag).collect(),
        })
    }

    pub fn get_tags(
        &self,
        tags: &[String],
        scope: TagScope,
        verbose: bool,
        include: &[String],
        exclude: &[String],
    ) -> anyhow::Result<GetTagsResult> {
        let result = self.collect_tags(Some(tags), scope, include, exclude)?;
        Ok(tags_output(result, verbose))
    }

    fn collect_tags(
        &self,
        tags: Option<&[String]>,
        scope: TagScope,
        include: &[String],
        exclude: &[String],
    ) -> anyhow::Result<TagsResult> {
        let filter = PathFilter::new(include, exclude)?;
        let mut buckets: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut occurrences: BTreeMap<String, Vec<TagOccurrence>> = BTreeMap::new();
        for note in self.index_filtered_notes(&filter)? {
            for found in &note.parsed.tags {
                if scope.includes_body_tag(found.scope) && tag_matches(tags, &found.tag) {
                    buckets
                        .entry(found.tag.clone())
                        .or_default()
                        .insert(note.file.relative_path.clone());
                    occurrences
                        .entry(found.tag.clone())
                        .or_default()
                        .push(TagOccurrence {
                            note: note.file.relative_path.clone(),
                            source_kind: tag_source_kind(
                                found.scope,
                                found
                                    .source
                                    .section
                                    .as_ref()
                                    .map(|section| section.heading_level),
                            ),
                            source: Some(found.source.clone().into()),
                            compact_section: compact_tag_section(
                                &found.source,
                                &note.parsed.headings,
                            ),
                        });
                }
            }
            if scope.includes_frontmatter_tags() {
                for found in frontmatter_tags(note.parsed.frontmatter.as_ref()) {
                    if tag_matches(tags, &found) {
                        buckets
                            .entry(found.clone())
                            .or_default()
                            .insert(note.file.relative_path.clone());
                        occurrences.entry(found).or_default().push(TagOccurrence {
                            note: note.file.relative_path.clone(),
                            source_kind: TagSourceKind::Frontmatter,
                            source: None,
                            compact_section: None,
                        });
                    }
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

fn count_matching_backlinks(notes: &[IndexedNote], wanted_path: &str) -> usize {
    notes
        .iter()
        .flat_map(|note| note.parsed.links.iter())
        .filter(|link| RefResolver::link_matches(&link.target, wanted_path, notes))
        .count()
}

fn link_targets_selected_scope(
    link: &LinkInfo,
    target: &ParsedNote,
    selector: &super::SectionSelector,
    selected_source: &SourceSpan,
) -> bool {
    match (selector, &link.reference) {
        (super::SectionSelector::Block { block_id }, Some(ReferenceInfo::BlockId { value })) => {
            value == block_id
        }
        (super::SectionSelector::Heading { .. }, Some(reference)) => {
            let Ok(Some(super::SectionSelector::Heading { heading })) =
                selector_from_reference(&Some(reference.clone()))
            else {
                return false;
            };
            let requested = ParsedHeadingSelector::parse(&heading);
            find_selectable_heading(target, &requested)
                .is_some_and(|heading| heading.source.line_start == selected_source.line_start)
        }
        _ => false,
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

fn tag_matches(wanted: Option<&[String]>, found: &str) -> bool {
    let found = normalize_tag(found);
    wanted.is_none_or(|wanted| wanted.iter().any(|tag| normalize_tag(tag) == found))
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

fn tag_source_kind(scope: TagScope, heading_level: Option<u8>) -> TagSourceKind {
    match scope {
        TagScope::Note | TagScope::Frontmatter | TagScope::Body => TagSourceKind::Body,
        TagScope::Section if heading_level == Some(1) => TagSourceKind::Note,
        TagScope::Section => TagSourceKind::Section,
        TagScope::Line => TagSourceKind::Line,
    }
}

fn compact_tag_section(source: &SourceSpan, headings: &[HeadingInfo]) -> Option<String> {
    let section = source.section.as_ref()?;
    if section.heading_level == 1 {
        return None;
    }

    let matching_headings = headings
        .iter()
        .filter(|heading| heading.level != 1 && heading.text == section.heading)
        .collect::<Vec<_>>();
    for suffix_len in 1..=section.heading_path.len() {
        let suffix_start = section.heading_path.len() - suffix_len;
        let suffix = &section.heading_path[suffix_start..];
        let matching_suffixes = matching_headings
            .iter()
            .filter(|heading| {
                heading.path.len() >= suffix_len
                    && heading.path[heading.path.len() - suffix_len..] == *suffix
            })
            .count();
        if matching_suffixes == 1 {
            return Some(suffix.join("/"));
        }
    }

    Some(section.heading_path.join("/"))
}

fn tags_output(result: TagsResult, verbose: bool) -> GetTagsResult {
    GetTagsResult {
        tags: result
            .tags
            .into_iter()
            .map(|bucket| {
                let tag = bucket.tag.clone();
                let occurrences = if verbose {
                    detailed_tag_occurrences(bucket.occurrences.clone())
                } else {
                    Vec::new()
                };
                TagOutputBucket {
                    tag,
                    notes: compact_tag_matches(bucket),
                    occurrences,
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
            let section = occurrence.compact_section;
            let note = occurrence
                .source
                .as_ref()
                .map(source_location)
                .unwrap_or(occurrence.note);
            CompactTagMatch {
                note,
                source_kind: occurrence.source_kind,
                section,
            }
        })
        .collect()
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
    let heading_path = (!section.heading_path.is_empty()
        && section.heading_path != vec![section.heading.clone()])
    .then_some(section.heading_path);
    DetailedSection {
        heading: section.heading,
        heading_level: section.heading_level,
        heading_path,
    }
}

fn source_location(source: &super::SearchSource) -> String {
    if source.line_start == source.line_end {
        format!("{}#L{}", source.path, source.line_start)
    } else {
        format!(
            "{}#L{}-L{}",
            source.path, source.line_start, source.line_end
        )
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
