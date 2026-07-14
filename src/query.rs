mod cache;
mod categories;
mod edit_distance;
mod graph;
mod links;
mod notes;
mod outline;
mod path_filter;
pub(crate) mod public;
mod search;
pub(crate) mod section;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use camino::Utf8Path;
use rayon::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, Serializer, ser::SerializeStruct};

use crate::parser::{ParsedNote, ReferenceInfo, SectionInfo, SourceSpan, path_with_line_ref};
use crate::resolver::{IndexedNote, ObsidianRef, RefResolver, ResolveResult};
use crate::vault::{NoteFile, Vault};

use self::{cache::ParseCache, edit_distance::levenshtein_distance, public::ResolvedReference};

pub use crate::parser::TagScope;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ListNotesResult {
    pub notes: Vec<NoteSummary>,
    pub pagination: ListNotesPagination,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ListNotesPagination {
    pub page: usize,
    pub total_pages: usize,
    pub total_notes: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct NoteSummary {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ReadNoteResult {
    pub source: String,
    pub content: String,
    #[serde(skip_serializing_if = "is_false")]
    pub truncated: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn default_page() -> usize {
    1
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct NoteStatsResult {
    /// Resolved note, heading, or block scope.
    pub scope: String,
    /// Word count computed from the raw Markdown source text.
    pub word_count: usize,
    /// Character count computed from the note's Markdown source text.
    pub character_count: usize,
    /// Line count computed from the note's Markdown source text.
    pub line_count: usize,
    /// Total number of inbound links to this note across the visible vault.
    pub backlink_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
/// Compact note structure for tool and CLI output.
///
/// The full parser keeps repeated source metadata for internal use, but this
/// shape avoids repeating the note path and full section object on every item.
pub struct NoteStructureResult {
    pub note: String,
    pub link_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frontmatter_fields: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headings: Option<Vec<CompactStructureHeading>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embeds: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocks: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CompactStructureHeading {
    pub heading: String,
    pub line: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct NoteOutlineResult {
    pub note: String,
    pub headings: Vec<OutlineHeading>,
    pub pagination: NoteOutlinePagination,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct OutlineHeading {
    pub heading: String,
    pub level: u8,
    pub line: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct NoteOutlinePagination {
    pub page: usize,
    pub total_pages: usize,
    pub total_headings: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SearchTextResult {
    pub matches: Vec<TextMatch>,
    pub pagination: SearchPagination,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SearchRegexResult {
    pub matches: Vec<TextMatch>,
    pub pagination: SearchPagination,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SearchPagination {
    pub page: usize,
    pub total_pages: usize,
    pub total_matches: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct TextMatch {
    /// Vault-relative path with Obsidian-style line reference.
    pub source: String,
    /// Short centered preview text. Use read_note for full evidence.
    pub preview: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct SearchSource {
    /// Vault-relative note path with Obsidian-style line reference, e.g. note.md#L1-L99.
    pub path: String,
    /// 1-based start line.
    #[serde(skip)]
    #[schemars(skip)]
    pub line_start: u64,
    /// 1-based end line.
    #[serde(skip)]
    #[schemars(skip)]
    pub line_end: u64,
    /// Nearest containing heading, when available.
    pub section: Option<SectionInfo>,
}

impl Serialize for SearchSource {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("SearchSource", 2)?;
        state.serialize_field(
            "path",
            &path_with_line_ref(&self.path, self.line_start, self.line_end),
        )?;
        state.serialize_field("section", &self.section)?;
        state.end()
    }
}

impl From<SourceSpan> for SearchSource {
    fn from(source: SourceSpan) -> Self {
        Self {
            path: source.path,
            line_start: source.line_start,
            line_end: source.line_end,
            section: source.section,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ResolveRefResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ambiguous_targets: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unresolved_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_target: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct BacklinksResult {
    pub scope: String,
    pub references: Vec<BacklinkReference>,
    pub pagination: BacklinksPagination,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct BacklinkReference {
    pub target: String,
    pub sources: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct BacklinksPagination {
    pub page: usize,
    pub total_pages: usize,
    pub total_backlinks: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct OutlinksResult {
    pub note: String,
    pub targets: Vec<OutlinkTarget>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ambiguous_targets: Vec<AmbiguousOutlinkTarget>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unresolved_targets: Vec<UnresolvedOutlinkTarget>,
    pub pagination: OutlinksPagination,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct OutlinkTarget {
    pub source: String,
    pub target: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AmbiguousOutlinkTarget {
    pub source: String,
    pub reference: String,
    pub candidates: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct UnresolvedOutlinkTarget {
    pub source: String,
    pub reference: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct OutlinksPagination {
    pub page: usize,
    pub total_pages: usize,
    pub total_links: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ListTagsResult {
    /// Unique tag names available in the requested scope.
    pub tags: Vec<String>,
    pub pagination: ListTagsPagination,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ListTagsPagination {
    pub page: usize,
    pub total_pages: usize,
    pub total_tags: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetTagResult {
    pub matches: Vec<String>,
    pub pagination: GetTagPagination,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetTagPagination {
    pub page: usize,
    pub total_pages: usize,
    pub total_matches: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ListCategoriesResult {
    /// Unique folder-derived category names.
    pub categories: Vec<String>,
    pub pagination: ListCategoriesPagination,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ListCategoriesPagination {
    pub page: usize,
    pub total_pages: usize,
    pub total_categories: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetCategoryResult {
    pub notes: Vec<String>,
    pub pagination: GetCategoryPagination,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetCategoryPagination {
    pub page: usize,
    pub total_pages: usize,
    pub total_notes: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct FrontmatterQueryOptions {
    /// Top-level frontmatter field name.
    pub field: String,
    /// Match mode. Use exists to find notes that contain the field without matching a value.
    pub mode: FrontmatterMatchMode,
    /// Value used by equals or regex mode.
    #[serde(default)]
    pub value: Option<String>,
    /// Vault-relative glob patterns. A note must match at least one when non-empty.
    #[serde(default)]
    pub include: Vec<String>,
    /// Vault-relative glob patterns. Matching notes are excluded.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// One-based page number. Each page contains up to 100 matching notes.
    #[serde(default = "default_page")]
    pub page: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FrontmatterMatchMode {
    Exists,
    Equals,
    Regex,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct FrontmatterQueryResult {
    pub notes: Vec<String>,
    pub pagination: FrontmatterQueryPagination,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct FrontmatterQueryPagination {
    pub page: usize,
    pub total_pages: usize,
    pub total_notes: usize,
}

#[derive(Clone, Debug)]
pub(crate) enum SectionSelector {
    Heading { heading: String },
    Block { block_id: String },
    Lines { line_start: u64, line_end: u64 },
}

pub(crate) fn link_location(source: &SearchSource) -> String {
    if source.line_start == source.line_end {
        format!("{}#L{}", source.path, source.line_start)
    } else {
        format!(
            "{}#L{}-L{}",
            source.path, source.line_start, source.line_end
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AuditLinksResult {
    pub unresolved: Vec<AuditUnresolvedLink>,
    pub ambiguous: Vec<AuditAmbiguousLink>,
    pub totals: AuditLinkTotals,
    pub pagination: Pagination,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AuditUnresolvedLink {
    pub source: String,
    pub target: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AuditAmbiguousLink {
    pub source: String,
    pub target: String,
    pub candidates: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted_candidates: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AuditLinkTotals {
    pub unresolved: usize,
    pub ambiguous: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Pagination {
    pub page: usize,
    pub total_pages: usize,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NeighborhoodDirection {
    Out,
    In,
    #[default]
    Both,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct NeighborhoodResult {
    pub center: NeighborhoodCenter,
    pub notes: Vec<NeighborhoodNote>,
    pub links: Vec<NeighborhoodLink>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted_notes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted_links: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct NeighborhoodCenter {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heading: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct NeighborhoodNote {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub distance: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
pub struct NeighborhoodLink {
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug)]
pub struct VaultQueries {
    pub vault: Vault,
    pub(crate) parse_cache: Arc<ParseCache>,
}

impl VaultQueries {
    pub fn new(vault: Vault) -> Self {
        let parse_cache = Arc::new(ParseCache::new(
            vault.config.parse_cache_ttl_secs,
            vault.config.parse_cache_max_entries,
        ));
        Self { vault, parse_cache }
    }

    pub(crate) fn parse_file_cached(
        &self,
        path: &Utf8Path,
        relative_path: String,
    ) -> anyhow::Result<Arc<ParsedNote>> {
        self.parse_cache
            .parse_note(path, relative_path, self.vault.config.max_note_bytes)
    }

    fn index_filtered_notes(
        &self,
        filter: &path_filter::PathFilter,
    ) -> anyhow::Result<Vec<IndexedNote>> {
        let mut notes: Vec<IndexedNote> = self
            .vault
            .list_notes()?
            .into_par_iter()
            .filter(|file| filter.is_match(&file.relative_path))
            .map(|file| read_and_parse(self, &file))
            .collect::<anyhow::Result<_>>()?;
        notes.sort_by(|a, b| natord::compare(&a.file.relative_path, &b.file.relative_path));
        Ok(notes)
    }
}

fn read_and_parse(queries: &VaultQueries, file: &NoteFile) -> anyhow::Result<IndexedNote> {
    let parsed = queries
        .parse_file_cached(&file.path, file.relative_path.clone())?
        .as_ref()
        .clone();
    Ok(IndexedNote {
        file: file.clone(),
        parsed,
    })
}

pub(crate) fn find_indexed_note<'a>(
    note: &str,
    notes: &'a [IndexedNote],
) -> anyhow::Result<&'a IndexedNote> {
    match RefResolver::resolve(note, notes) {
        ResolveResult::Resolved { path, .. } => notes
            .iter()
            .find(|note| note.file.relative_path == path)
            .ok_or_else(|| anyhow::anyhow!("resolved note disappeared")),
        ResolveResult::Ambiguous { candidates, .. } => Err(anyhow::anyhow!(
            "ambiguous note reference: {:?}",
            candidates
        )),
        ResolveResult::Unresolved { reference } => {
            if let Some(closest) = suggested_note_reference(&reference, notes) {
                return Err(anyhow::anyhow!(
                    "unresolved note reference: {:?} (did you mean {:?})",
                    reference.raw,
                    closest
                ));
            }
            Err(anyhow::anyhow!(
                "unresolved note reference: {:?}",
                reference.raw
            ))
        }
    }
}

fn suggested_note_reference(reference: &ObsidianRef, notes: &[IndexedNote]) -> Option<String> {
    let path = suggested_note_path(&reference.target, notes)?;
    let note = notes.iter().find(|note| note.file.relative_path == path)?;
    RefResolver::reference_exists(note, &reference.reference)
        .then(|| format!("{path}{}", reference_suffix(&reference.reference)))
}

fn suggested_note_path(target: &str, notes: &[IndexedNote]) -> Option<String> {
    let target = target.trim_end_matches(".md");
    let target_stem = target.rsplit('/').next().unwrap_or(target);
    let exact_stems = notes
        .iter()
        .filter(|note| {
            note.file
                .relative_path
                .trim_end_matches(".md")
                .rsplit('/')
                .next()
                == Some(target_stem)
        })
        .collect::<Vec<_>>();
    if let [note] = exact_stems.as_slice() {
        return Some(note.file.relative_path.clone());
    }

    notes
        .iter()
        .map(|note| {
            let path = note.file.relative_path.as_str();
            (
                levenshtein_distance(target, path.trim_end_matches(".md")),
                path,
            )
        })
        .min_by_key(|(distance, _)| *distance)
        .filter(|(distance, path)| *distance <= 3 && !path.is_empty())
        .map(|(_, path)| path.to_string())
}

pub(crate) fn compact_resolve_result(
    result: ResolveResult,
    notes: &[IndexedNote],
) -> ResolveRefResult {
    match result {
        ResolveResult::Resolved {
            path, reference, ..
        } => ResolveRefResult {
            target: Some(format!("{path}{}", reference_suffix(&reference.reference))),
            ambiguous_targets: None,
            unresolved_target: None,
            suggested_target: None,
        },
        ResolveResult::Ambiguous {
            reference,
            candidates,
        } => {
            let suffix = reference_suffix(&reference.reference);
            let mut targets = candidates
                .into_iter()
                .map(|candidate| format!("{}{suffix}", candidate.path))
                .collect::<Vec<_>>();
            targets.sort_by(|a, b| natord::compare(a, b));
            ResolveRefResult {
                target: None,
                ambiguous_targets: Some(targets),
                unresolved_target: None,
                suggested_target: None,
            }
        }
        ResolveResult::Unresolved { reference } => ResolveRefResult {
            target: None,
            ambiguous_targets: None,
            unresolved_target: Some(reference_display(&reference.target, &reference.reference)),
            suggested_target: suggested_note_reference(&reference, notes),
        },
    }
}

pub(crate) fn reference_suffix(reference: &Option<ReferenceInfo>) -> String {
    match reference {
        None => String::new(),
        Some(ReferenceInfo::Heading { value }) => format!("#{value}"),
        Some(ReferenceInfo::MultiHeading { value }) => {
            value.iter().map(|heading| format!("#{heading}")).collect()
        }
        Some(ReferenceInfo::BlockId { value }) => format!("#^{value}"),
    }
}

pub(crate) fn reference_display(target: &str, reference: &Option<ReferenceInfo>) -> String {
    format!("{target}{}", reference_suffix(reference))
}

pub(crate) fn resolved_reference_from_result(result: &ResolveResult) -> Option<ResolvedReference> {
    match result {
        ResolveResult::Resolved {
            path,
            reference,
            block_id,
            ..
        } => {
            if let Some(block_id) = block_id {
                return Some(ResolvedReference::block(path.clone(), block_id.clone()));
            }
            Some(ResolvedReference::heading(
                path.clone(),
                heading_path(&reference.reference),
            ))
        }
        _ => None,
    }
}

fn heading_path(reference: &Option<ReferenceInfo>) -> Vec<String> {
    match reference {
        Some(ReferenceInfo::Heading { value }) => vec![value.clone()],
        Some(ReferenceInfo::MultiHeading { value }) => value.clone(),
        _ => Vec::new(),
    }
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect()
}
