use std::collections::BTreeSet;

use crate::parser::{LinkInfo, ParsedNote, ReferenceInfo, SourceSpan};
use crate::resolver::{IndexedNote, RefResolver, ResolveResult};

use super::path_filter::PathFilter;
use super::public::{Locator, PageSlice, ResolvedReference};
use super::section::{ParsedHeadingSelector, find_selectable_heading, selector_from_reference};

use super::{
    AmbiguousOutlinkTarget, BacklinkReference, BacklinksPagination, BacklinksResult,
    FrontmatterMatchMode, FrontmatterQueryOptions, FrontmatterQueryPagination,
    FrontmatterQueryResult, GetTagPagination, GetTagResult, ListTagsPagination, ListTagsResult,
    OutlinkTarget, OutlinksPagination, OutlinksResult, TagScope, UnresolvedOutlinkTarget,
    VaultQueries, find_indexed_note, reference_display, reference_suffix,
    resolved_reference_from_result,
};

const LINK_PAGE_SIZE: usize = 50;
const DISCOVERY_PAGE_SIZE: usize = 100;

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
    #[tracing::instrument(
        name = "vault.query.get_outlinks",
        fields(operation.kind = "query", operation.name = "get_outlinks"),
        err
    )]
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

    #[tracing::instrument(
        name = "vault.query.get_backlinks",
        fields(operation.kind = "query", operation.name = "get_backlinks"),
        err
    )]
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

    #[tracing::instrument(
        name = "vault.query.list_tags",
        fields(operation.kind = "query", operation.name = "list_tags"),
        err
    )]
    pub fn list_tags(
        &self,
        scope: TagScope,
        include: &[String],
        exclude: &[String],
        page: usize,
    ) -> anyhow::Result<ListTagsResult> {
        let tags = self.collect_tag_names(scope, include, exclude)?;
        let slice = PageSlice::new(tags, page, DISCOVERY_PAGE_SIZE)?;
        let total_tags = slice.total_items();
        let pagination = slice.pagination();
        Ok(ListTagsResult {
            tags: slice.into_items(),
            pagination: ListTagsPagination {
                page: pagination.page,
                total_pages: pagination.total_pages,
                total_tags,
            },
        })
    }

    #[tracing::instrument(
        name = "vault.query.get_tag",
        fields(operation.kind = "query", operation.name = "get_tag"),
        err
    )]
    pub fn get_tag(
        &self,
        tag: &str,
        scope: TagScope,
        include: &[String],
        exclude: &[String],
        page: usize,
    ) -> anyhow::Result<GetTagResult> {
        let wanted = normalize_tag(tag);
        if wanted.is_empty() {
            anyhow::bail!("provide a non-empty tag");
        }
        let matches = self.collect_tag_locators(&wanted, scope, include, exclude)?;
        let slice = PageSlice::new(matches, page, DISCOVERY_PAGE_SIZE)?;
        let total_matches = slice.total_items();
        let pagination = slice.pagination();
        Ok(GetTagResult {
            matches: slice.into_items(),
            pagination: GetTagPagination {
                page: pagination.page,
                total_pages: pagination.total_pages,
                total_matches,
            },
        })
    }

    fn collect_tag_names(
        &self,
        scope: TagScope,
        include: &[String],
        exclude: &[String],
    ) -> anyhow::Result<Vec<String>> {
        let filter = PathFilter::new(include, exclude)?;
        let mut tags = BTreeSet::new();
        for note in self.index_filtered_notes(&filter)? {
            if matches!(
                scope,
                TagScope::Note | TagScope::Body | TagScope::Section | TagScope::Line
            ) {
                for found in &note.parsed.tags {
                    let tag = normalize_tag(&found.tag);
                    if !tag.is_empty() {
                        tags.insert(tag);
                    }
                }
            }
            if matches!(scope, TagScope::Note | TagScope::Frontmatter) {
                for found in frontmatter_tags(note.parsed.frontmatter.as_ref()) {
                    if !found.is_empty() {
                        tags.insert(found);
                    }
                }
            }
        }
        let mut tags = tags.into_iter().collect::<Vec<_>>();
        tags.sort_by(|a, b| natord::compare(a, b));
        Ok(tags)
    }

    fn collect_tag_locators(
        &self,
        wanted: &str,
        scope: TagScope,
        include: &[String],
        exclude: &[String],
    ) -> anyhow::Result<Vec<String>> {
        let filter = PathFilter::new(include, exclude)?;
        let mut locators = BTreeSet::new();
        for note in self.index_filtered_notes(&filter)? {
            let frontmatter_matches = matches!(scope, TagScope::Note | TagScope::Frontmatter)
                && frontmatter_tags(note.parsed.frontmatter.as_ref())
                    .into_iter()
                    .any(|tag| tag == wanted);
            if frontmatter_matches {
                locators.insert(note.file.relative_path.clone());
            }

            if matches!(scope, TagScope::Frontmatter) {
                continue;
            }

            for found in &note.parsed.tags {
                if normalize_tag(&found.tag) != wanted {
                    continue;
                }
                let locator = match scope {
                    TagScope::Note | TagScope::Body => note.file.relative_path.clone(),
                    TagScope::Section => {
                        section_tag_locator(&note.file.relative_path, &found.source)
                    }
                    TagScope::Line => Locator::lines(
                        &note.file.relative_path,
                        found.source.line_start,
                        found.source.line_end,
                    ),
                    TagScope::Frontmatter => unreachable!(),
                };
                locators.insert(locator);
            }
        }
        let mut locators = locators.into_iter().collect::<Vec<_>>();
        locators.sort_by(|a, b| natord::compare(a, b));
        Ok(locators)
    }

    #[tracing::instrument(
        name = "vault.query.query_frontmatter",
        fields(operation.kind = "query", operation.name = "query_frontmatter"),
        err
    )]
    pub fn query_frontmatter(
        &self,
        options: FrontmatterQueryOptions,
    ) -> anyhow::Result<FrontmatterQueryResult> {
        let matcher = match options.mode {
            FrontmatterMatchMode::Exists => {
                if options.value.is_some() {
                    anyhow::bail!("frontmatter exists query does not accept a value");
                }
                None
            }
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

        let filter = PathFilter::new(&options.include, &options.exclude)?;
        let mut notes = Vec::new();
        for note in self.index_filtered_notes(&filter)? {
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
                notes.push(note.file.relative_path.clone());
            }
        }

        notes.sort_by(|a, b| natord::compare(a, b));
        let slice = PageSlice::new(notes, options.page, DISCOVERY_PAGE_SIZE)?;
        let total_notes = slice.total_items();
        let pagination = slice.pagination();
        Ok(FrontmatterQueryResult {
            notes: slice.into_items(),
            pagination: FrontmatterQueryPagination {
                page: pagination.page,
                total_pages: pagination.total_pages,
                total_notes,
            },
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

fn section_tag_locator(path: &str, source: &SourceSpan) -> String {
    let Some(section) = source.section.as_ref() else {
        return path.to_string();
    };
    if section.heading_level == 1 {
        return path.to_string();
    }

    let mut heading_path = section.heading_path.clone();
    if heading_path.len() == section.heading_level as usize && !heading_path.is_empty() {
        heading_path.remove(0);
    }
    if heading_path.is_empty() {
        return path.to_string();
    }

    let mut locator = path.to_string();
    for heading in heading_path {
        locator.push('#');
        locator.push_str(&heading);
    }
    locator
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
