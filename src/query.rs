mod cache;
mod categories;
mod graph;
mod levenshtein_distance;
mod links;
mod notes;
mod outline;
mod path_filter;
pub(crate) mod public;
mod search;
pub(crate) mod section;

#[cfg(test)]
mod tests;

use self::{
    cache::ParseCache, levenshtein_distance::levenshtein_distance, public::ResolvedReference,
};
pub use crate::parser::TagScope;
use crate::parser::{
    LinkInfo, LinkKind, ParsedNote, ReferenceInfo, SectionInfo, SourceSpan, path_with_line_ref,
};
use crate::resolver::{IndexedNote, ObsidianRef, RefResolver, ResolveCandidate, ResolveResult};
use crate::vault::{NoteFile, Vault};
use camino::Utf8Path;
use rayon::prelude::*;
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize, Serializer, ser::SerializeStruct};
use std::{borrow::Cow, collections::BTreeMap, sync::Arc};

/// Schema-only stand-in for non-negative Rust integer fields exposed over MCP.
///
/// `schemars` labels unsigned integers with its non-standard `uint` format,
/// which some MCP clients report as an unknown JSON Schema format.
pub(crate) struct McpNonNegativeInteger;

impl JsonSchema for McpNonNegativeInteger {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "McpNonNegativeInteger".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        serde_json::json!({"type": "integer", "minimum": 0})
            .try_into()
            .expect("valid non-negative integer schema")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ListNotesResult {
    pub notes: Vec<NoteSummary>,
    pub pagination: ListNotesPagination,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ListNotesPagination {
    #[schemars(with = "McpNonNegativeInteger")]
    pub page: usize,
    #[schemars(with = "McpNonNegativeInteger")]
    pub total_pages: usize,
    #[schemars(with = "McpNonNegativeInteger")]
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
    #[schemars(with = "Option<bool>")]
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
    #[schemars(with = "McpNonNegativeInteger")]
    pub word_count: usize,
    /// Character count computed from the note's Markdown source text.
    #[schemars(with = "McpNonNegativeInteger")]
    pub character_count: usize,
    /// Line count computed from the note's Markdown source text.
    #[schemars(with = "McpNonNegativeInteger")]
    pub line_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
/// Compact note structure for tool and CLI output.
///
/// The full parser keeps repeated source metadata for internal use, but this
/// shape avoids repeating the note path and full section object on every item.
pub struct NoteStructureResult {
    pub note: String,
    #[schemars(with = "McpNonNegativeInteger")]
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
    #[schemars(with = "Option<BTreeMap<String, McpNonNegativeInteger>>")]
    pub omitted: Option<BTreeMap<String, usize>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CompactStructureHeading {
    pub heading: String,
    #[schemars(with = "McpNonNegativeInteger")]
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
    #[schemars(with = "McpNonNegativeInteger")]
    pub line: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct NoteOutlinePagination {
    #[schemars(with = "McpNonNegativeInteger")]
    pub page: usize,
    #[schemars(with = "McpNonNegativeInteger")]
    pub total_pages: usize,
    #[schemars(with = "McpNonNegativeInteger")]
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
    #[schemars(with = "McpNonNegativeInteger")]
    pub page: usize,
    #[schemars(with = "McpNonNegativeInteger")]
    pub total_pages: usize,
    #[schemars(with = "McpNonNegativeInteger")]
    pub total_matches: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct TextMatch {
    /// Workspace-relative path with Obsidian-style line reference.
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
    #[schemars(with = "McpNonNegativeInteger")]
    pub page: usize,
    #[schemars(with = "McpNonNegativeInteger")]
    pub total_pages: usize,
    #[schemars(with = "McpNonNegativeInteger")]
    pub total_backlinks: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct OutlinksResult {
    pub note: String,
    pub targets: Vec<OutlinkTarget>,
    pub ambiguous_targets: Vec<AmbiguousOutlinkTarget>,
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
    #[schemars(with = "McpNonNegativeInteger")]
    pub page: usize,
    #[schemars(with = "McpNonNegativeInteger")]
    pub total_pages: usize,
    #[schemars(with = "McpNonNegativeInteger")]
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
    #[schemars(with = "McpNonNegativeInteger")]
    pub page: usize,
    #[schemars(with = "McpNonNegativeInteger")]
    pub total_pages: usize,
    #[schemars(with = "McpNonNegativeInteger")]
    pub total_tags: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetTagResult {
    pub matches: Vec<String>,
    pub pagination: GetTagPagination,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetTagPagination {
    #[schemars(with = "McpNonNegativeInteger")]
    pub page: usize,
    #[schemars(with = "McpNonNegativeInteger")]
    pub total_pages: usize,
    #[schemars(with = "McpNonNegativeInteger")]
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
    #[schemars(with = "McpNonNegativeInteger")]
    pub page: usize,
    #[schemars(with = "McpNonNegativeInteger")]
    pub total_pages: usize,
    #[schemars(with = "McpNonNegativeInteger")]
    pub total_categories: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetCategoryResult {
    pub notes: Vec<String>,
    pub pagination: GetCategoryPagination,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetCategoryPagination {
    #[schemars(with = "McpNonNegativeInteger")]
    pub page: usize,
    #[schemars(with = "McpNonNegativeInteger")]
    pub total_pages: usize,
    #[schemars(with = "McpNonNegativeInteger")]
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
    #[schemars(with = "McpNonNegativeInteger")]
    pub page: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FrontmatterMatchMode {
    Exists,
    Equals,
    Regex,
}

impl TryFrom<&str> for FrontmatterMatchMode {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "exists" => Ok(crate::query::FrontmatterMatchMode::Exists),
            "equals" => Ok(crate::query::FrontmatterMatchMode::Equals),
            "regex" => Ok(crate::query::FrontmatterMatchMode::Regex),
            _ => Err(anyhow::anyhow!(
                "invalid frontmatter query mode: '{value}'; expected one of: exists, equals, regex"
            )),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct FrontmatterQueryResult {
    pub notes: Vec<String>,
    pub pagination: FrontmatterQueryPagination,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct FrontmatterQueryPagination {
    #[schemars(with = "McpNonNegativeInteger")]
    pub page: usize,
    #[schemars(with = "McpNonNegativeInteger")]
    pub total_pages: usize,
    #[schemars(with = "McpNonNegativeInteger")]
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
    #[schemars(with = "Option<McpNonNegativeInteger>")]
    pub omitted_candidates: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AuditLinkTotals {
    #[schemars(with = "McpNonNegativeInteger")]
    pub unresolved: usize,
    #[schemars(with = "McpNonNegativeInteger")]
    pub ambiguous: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Pagination {
    #[schemars(with = "McpNonNegativeInteger")]
    pub page: usize,
    #[schemars(with = "McpNonNegativeInteger")]
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

impl TryFrom<&str> for NeighborhoodDirection {
    type Error = anyhow::Error;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "out" => Ok(Self::Out),
            "in" => Ok(Self::In),
            "both" => Ok(Self::Both),
            _ => Err(anyhow::anyhow!(
                "invalid neighborhood direction: '{value}'; expected one of: both, out, in"
            )),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct NeighborhoodResult {
    pub center: NeighborhoodCenter,
    pub notes: Vec<NeighborhoodNote>,
    pub links: Vec<NeighborhoodLink>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<McpNonNegativeInteger>")]
    pub omitted_notes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<McpNonNegativeInteger>")]
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
    #[schemars(with = "McpNonNegativeInteger")]
    pub distance: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
pub struct NeighborhoodLink {
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug)]
pub struct VaultQueries {
    pub vault: Arc<Vault>,
    pub(crate) parse_cache: Arc<ParseCache>,
}

impl VaultQueries {
    pub fn new(vault: Arc<Vault>) -> Self {
        let config = vault.config();
        let parse_cache = Arc::new(ParseCache::new(
            config.parse_cache_ttl_secs,
            config.parse_cache_max_entries,
        ));
        Self { vault, parse_cache }
    }

    #[tracing::instrument(
        name = "vault.parse_note_from_path",
        skip_all,
        fields(note.path = relative_path),
        err
    )]
    /// Parse a note and its source content from an already resolved path.
    pub(crate) fn parse_note_from_path(
        &self,
        path: &Utf8Path,
        relative_path: &str,
    ) -> anyhow::Result<(Arc<ParsedNote>, String)> {
        self.parse_cache
            .parse_note(path, relative_path, self.vault.config().max_note_bytes)
    }

    #[tracing::instrument(name = "vault.parse_note", skip_all, err)]
    pub(crate) fn parse_note(&self, note: &str) -> anyhow::Result<(Arc<ParsedNote>, String)> {
        let path = self.resolve_note_path(note)?;
        let relative_path = self.vault.relative_path(&path);
        self.parse_cache
            .parse_note(&path, &relative_path, self.vault.config().max_note_bytes)
    }

    /// Resolve a parsed link, allowing Markdown links to target notes excluded from discovery.
    pub(crate) fn resolve_link(
        &self,
        link: &LinkInfo,
        notes: &[IndexedNote],
    ) -> anyhow::Result<ResolveResult> {
        let reference = reference_display(&link.target, &link.reference);
        let parsed_reference = RefResolver::parse_ref(&reference);
        if matches!(link.kind, LinkKind::Markdown) {
            let heading = match &parsed_reference.reference {
                Some(ReferenceInfo::Heading { value }) => Some(value.clone()),
                Some(ReferenceInfo::MultiHeading { value }) => Some(value.join("#")),
                _ => None,
            };
            let block_id = match &parsed_reference.reference {
                Some(ReferenceInfo::BlockId { value }) => Some(value.clone()),
                _ => None,
            };
            return Ok(ResolveResult::Resolved {
                path: parsed_reference.target.clone(),
                reference: parsed_reference,
                heading,
                block_id,
            });
        }

        // A bare wikilink directly matches a root-level note with that name.
        if !parsed_reference.target.contains('/') && !parsed_reference.target.ends_with(".md") {
            let direct_target = format!("{}.md", parsed_reference.target);
            if notes
                .iter()
                .any(|note| note.file.relative_path == direct_target)
            {
                return Ok(resolve_exact_link(&direct_target, &parsed_reference, notes));
            }
        }

        if let Some(relative_target) =
            source_relative_target(&link.source.path, &parsed_reference.target)
            && notes
                .iter()
                .any(|note| note.file.relative_path == relative_target)
        {
            return Ok(resolve_exact_link(
                &relative_target,
                &parsed_reference,
                notes,
            ));
        }

        if !parsed_reference.target.is_empty() {
            let target = parsed_reference.target.trim_end_matches(".md");
            let suffix = format!("/{target}.md");
            let candidates = notes
                .iter()
                .filter(|note| {
                    note.file.relative_path == format!("{target}.md")
                        || note.file.relative_path.ends_with(&suffix)
                })
                .map(|note| ResolveCandidate {
                    path: note.file.relative_path.clone(),
                    match_kind: "path_suffix".to_string(),
                })
                .collect::<Vec<_>>();
            if candidates.len() == 1 {
                return Ok(resolve_exact_link(
                    &candidates[0].path,
                    &parsed_reference,
                    notes,
                ));
            }
            if candidates.len() > 1 {
                return Ok(ResolveResult::Ambiguous {
                    reference: RefResolver::parse_ref(&reference),
                    candidates,
                });
            }
        } else if let Some(note) = notes
            .iter()
            .find(|note| note.file.relative_path == link.source.path)
        {
            return Ok(resolve_exact_link(
                &note.file.relative_path,
                &parsed_reference,
                notes,
            ));
        }
        return Ok(RefResolver::resolve(&reference, notes));
    }

    #[tracing::instrument(
        name = "vault.query.index_filtered_notes",
        skip_all,
        fields(operation.kind = "query", operation.name = "index_filtered_notes"),
        err
    )]
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
    let (parsed, _) = queries.parse_note_from_path(&file.path, &file.relative_path)?;
    Ok(IndexedNote {
        file: file.clone(),
        parsed,
    })
}

fn resolve_exact_link(
    target: &str,
    original: &ObsidianRef,
    notes: &[IndexedNote],
) -> ResolveResult {
    let mut result = RefResolver::resolve(&reference_display(target, &original.reference), notes);
    if let ResolveResult::Unresolved { reference } = &mut result {
        reference.target = original.target.clone();
    }
    result
}

fn source_relative_target(source: &str, target: &str) -> Option<String> {
    if target.is_empty() {
        return None;
    }
    let mut parts = Utf8Path::new(source)
        .parent()
        .map(|parent| {
            parent
                .components()
                .map(|component| component.as_str().to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            segment => parts.push(segment.to_string()),
        }
    }
    if parts.is_empty() {
        return None;
    }
    let mut path = parts.join("/");
    if !path.ends_with(".md") {
        path.push_str(".md");
    }
    Some(path)
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
            let suggestions = suggested_note_references(&reference, notes);
            if let [closest] = suggestions.as_slice() {
                return Err(anyhow::anyhow!(
                    "unresolved note reference: {:?} (did you mean {:?})",
                    reference.raw,
                    closest
                ));
            }
            if !suggestions.is_empty() {
                return Err(anyhow::anyhow!(
                    "unresolved note reference: {:?} (did you mean one of: {})",
                    reference.raw,
                    suggestions
                        .iter()
                        .map(|suggestion| format!("{suggestion:?}"))
                        .collect::<Vec<_>>()
                        .join(", ")
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
    let suggestions = suggested_note_references(reference, notes);
    match suggestions.as_slice() {
        [suggestion] => Some(suggestion.clone()),
        _ => None,
    }
}

fn suggested_note_references(reference: &ObsidianRef, notes: &[IndexedNote]) -> Vec<String> {
    suggested_note_paths(&reference.target, notes)
        .into_iter()
        .filter_map(|path| {
            notes
                .iter()
                .find(|note| note.file.relative_path == path)
                .filter(|note| RefResolver::reference_exists(note, &reference.reference))
                .map(|_| format!("{path}{}", reference_suffix(&reference.reference)))
        })
        .collect()
}

fn suggested_note_paths(target: &str, notes: &[IndexedNote]) -> Vec<String> {
    let requested = Utf8Path::new(target);
    let requested_parent = requested.parent();
    let requested_extension = requested.extension();
    let requested_stem = requested.file_stem().unwrap_or(target);

    let exact_paths = notes
        .iter()
        .filter(|note| note.file.relative_path == target)
        .map(|note| note.file.relative_path.clone())
        .collect::<Vec<_>>();
    if !exact_paths.is_empty() {
        return sorted_paths(exact_paths);
    }

    let same_directory_extension = |note: &&IndexedNote| {
        let candidate = Utf8Path::new(&note.file.relative_path);
        candidate.parent() == requested_parent && candidate.extension() == requested_extension
    };
    let same_directory_exact_stems = notes
        .iter()
        .filter(same_directory_extension)
        .filter(|note| Utf8Path::new(&note.file.relative_path).file_stem() == Some(requested_stem))
        .map(|note| note.file.relative_path.clone())
        .collect::<Vec<_>>();
    if !same_directory_exact_stems.is_empty() {
        return sorted_paths(same_directory_exact_stems);
    }

    let stem_prefix = format!("{requested_stem}-");
    let same_directory_prefix_stems = notes
        .iter()
        .filter(same_directory_extension)
        .filter(|note| {
            Utf8Path::new(&note.file.relative_path)
                .file_stem()
                .is_some_and(|stem| stem.starts_with(&stem_prefix))
        })
        .map(|note| note.file.relative_path.clone())
        .collect::<Vec<_>>();
    if !same_directory_prefix_stems.is_empty() {
        return sorted_paths(same_directory_prefix_stems);
    }

    let exact_stems = notes
        .iter()
        .filter(|note| Utf8Path::new(&note.file.relative_path).file_stem() == Some(requested_stem))
        .map(|note| note.file.relative_path.clone())
        .collect::<Vec<_>>();
    if !exact_stems.is_empty() {
        return sorted_paths(exact_stems);
    }

    let target = target.trim_end_matches(".md");
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
        .map(|(_, path)| vec![path.to_string()])
        .unwrap_or_default()
}

fn sorted_paths(mut paths: Vec<String>) -> Vec<String> {
    paths.sort_by(|left, right| natord::compare(left, right));
    paths.dedup();
    paths
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
