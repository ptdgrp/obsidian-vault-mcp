mod cache;
mod categories;
mod context;
mod edit_distance;
mod files;
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

use std::{fs, sync::Arc};

use camino::Utf8Path;
use clap::ValueEnum;
use rayon::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, Serializer, ser::SerializeStruct};

use crate::parser::{
    BlockInfo, EmbedInfo, HeadingInfo, LinkInfo, ParsedNote, ReferenceInfo, SectionInfo,
    SourceSpan, TagInfo, path_with_line_ref, slice_text,
};
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
    pub path: String,
    pub source: SourceSpan,
    pub content: String,
    pub truncated: bool,
    /// How to retrieve omitted content when `truncated` is true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_step: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct NoteStatsResult {
    /// Vault-relative resolved note path.
    pub note: String,
    /// Counting strategy used for `word_count`.
    pub word_count_mode: WordCountMode,
    /// Word count computed with `word_count_mode`.
    pub word_count: usize,
    /// Character count computed from the note's Markdown source text.
    pub character_count: usize,
    /// Line count computed from the note's Markdown source text.
    pub line_count: usize,
    /// Total number of inbound links to this note across the visible vault.
    pub backlink_count: usize,
    /// Selected source span when `note` includes a heading or block reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceSpan>,
}

#[derive(
    Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
pub enum WordCountMode {
    /// Count words from the raw Markdown source text.
    #[default]
    Source,
    /// Count words from visible Markdown/Obsidian note text.
    Visible,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
/// Compact note structure for tool and CLI output.
///
/// The full parser keeps repeated source metadata for internal use, but this
/// shape avoids repeating the note path and full section object on every item.
pub struct NoteStructureResult {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frontmatter: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headings: Vec<CompactHeadingInfo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<CompactLinkInfo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub embeds: Vec<CompactEmbedInfo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<CompactTagInfo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<CompactBlockInfo>,
}

impl From<ParsedNote> for NoteStructureResult {
    fn from(note: ParsedNote) -> Self {
        Self {
            path: note.path,
            frontmatter: note.frontmatter,
            headings: note
                .headings
                .into_iter()
                .map(CompactHeadingInfo::from)
                .collect(),
            links: note.links.into_iter().map(CompactLinkInfo::from).collect(),
            embeds: note
                .embeds
                .into_iter()
                .map(CompactEmbedInfo::from)
                .collect(),
            tags: note.tags.into_iter().map(CompactTagInfo::from).collect(),
            blocks: note
                .blocks
                .into_iter()
                .map(CompactBlockInfo::from)
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct CompactHeadingInfo {
    pub text: String,
    pub level: u8,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<String>,
    pub line: u64,
}

impl From<HeadingInfo> for CompactHeadingInfo {
    fn from(heading: HeadingInfo) -> Self {
        Self {
            text: heading.text,
            level: heading.level,
            path: heading.path,
            line: heading.source.line_start,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct CompactLinkInfo {
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<crate::parser::ReferenceInfo>,
    #[serde(default)]
    pub kind: crate::parser::LinkKind,
    pub line: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
}

impl From<LinkInfo> for CompactLinkInfo {
    fn from(link: LinkInfo) -> Self {
        let (line, section) = compact_source(link.source);
        Self {
            target: link.target,
            alias: link.alias,
            reference: link.reference,
            kind: link.kind,
            line,
            section,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct CompactEmbedInfo {
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<crate::parser::ReferenceInfo>,
    pub line: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
}

impl From<EmbedInfo> for CompactEmbedInfo {
    fn from(embed: EmbedInfo) -> Self {
        let (line, section) = compact_source(embed.source);
        Self {
            target: embed.target,
            reference: embed.reference,
            line,
            section,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct CompactTagInfo {
    pub tag: String,
    pub line: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
}

impl From<TagInfo> for CompactTagInfo {
    fn from(tag: TagInfo) -> Self {
        let (line, section) = compact_source(tag.source);
        Self {
            tag: tag.tag,
            line,
            section,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct CompactBlockInfo {
    pub id: String,
    pub line: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
}

impl From<BlockInfo> for CompactBlockInfo {
    fn from(block: BlockInfo) -> Self {
        let (line, section) = compact_source(block.source);
        Self {
            id: block.id,
            line,
            section,
        }
    }
}

fn compact_source(source: SourceSpan) -> (u64, Option<String>) {
    (source.line_start, source.section.map(compact_section))
}

fn compact_section(section: SectionInfo) -> String {
    if section.heading_path.is_empty() {
        section.heading
    } else {
        section.heading_path.join(" > ")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct NoteOutlineResult {
    pub note: String,
    pub outline: Vec<OutlineNode>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct OutlineNode {
    pub heading: String,
    pub level: u8,
    pub heading_path: Vec<String>,
    pub source: SearchSource,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<OutlineNode>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SearchTextResult {
    pub query: String,
    pub matches: Vec<TextMatch>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SearchRegexResult {
    pub pattern: String,
    pub matches: Vec<TextMatch>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct TextMatch {
    /// Lightweight source for LLM navigation. Byte offsets are intentionally omitted.
    pub source: SearchSource,
    /// Short preview text. Use read_note for full evidence.
    pub snippet: String,
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
pub struct TagsResult {
    pub tags: Vec<TagBucket>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ListTagsResult {
    /// Unique tag names available in the requested scope.
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct CompactTagMatch {
    /// Vault-relative path, with #L line reference for body tags.
    pub note: String,
    pub source_kind: TagSourceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DetailedTagOccurrence {
    /// Vault-relative path, with #L line reference when line data exists.
    pub location: String,
    pub source_kind: TagSourceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<DetailedSection>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DetailedSection {
    pub heading: String,
    pub heading_level: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heading_path: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetTagsResult {
    pub tags: Vec<TagOutputBucket>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ListCategoriesResult {
    /// Unique folder-derived category names.
    pub categories: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetCategoriesResult {
    pub categories: Vec<CategoryOutputBucket>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct CategoryOutputBucket {
    pub category: String,
    /// Vault-relative Markdown note paths in this folder-derived category.
    pub files: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct TagOutputBucket {
    pub tag: String,
    pub notes: Vec<CompactTagMatch>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub occurrences: Vec<DetailedTagOccurrence>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct TagBucket {
    pub tag: String,
    pub notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub occurrences: Vec<TagOccurrence>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct TagOccurrence {
    pub note: String,
    pub source_kind: TagSourceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SearchSource>,
    #[serde(skip)]
    #[schemars(skip)]
    pub compact_section: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TagSourceKind {
    Note,
    Body,
    Section,
    Line,
    Frontmatter,
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
    pub field: String,
    pub mode: FrontmatterMatchMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub matches: Vec<FrontmatterMatch>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct FrontmatterMatch {
    pub note: String,
    pub value: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ContextResult {
    pub reference: String,
    pub groups: Vec<ContextGroup>,
    pub truncated: bool,
    pub omitted_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ContextGroup {
    pub kind: String,
    pub items: Vec<ContextItem>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ContextItem {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Human-readable file size using binary units.
    pub size: String,
}

#[derive(Clone, Debug)]
pub(crate) enum SectionSelector {
    Heading { heading: String },
    Block { block_id: String },
    Lines { line_start: u64, line_end: u64 },
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct LinkEvidence {
    pub source: SearchSource,
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub resolved: LegacyResolveSummary,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub snippet: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LegacyResolveSummary {
    Resolved {
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        heading: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        block_id: Option<String>,
    },
    Ambiguous {
        candidates: Vec<crate::resolver::ResolveCandidate>,
    },
    Unresolved,
}

impl From<ResolveResult> for LegacyResolveSummary {
    fn from(result: ResolveResult) -> Self {
        match result {
            ResolveResult::Resolved {
                path,
                heading,
                block_id,
                ..
            } => Self::Resolved {
                path,
                heading,
                block_id,
            },
            ResolveResult::Ambiguous { candidates, .. } => Self::Ambiguous { candidates },
            ResolveResult::Unresolved { .. } => Self::Unresolved,
        }
    }
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
pub struct UnresolvedLinksResult {
    pub links: Vec<LinkEvidence>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AmbiguousLinksResult {
    pub links: Vec<LinkEvidence>,
    pub truncated: bool,
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

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct VaultGraphResult {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct GraphNeighborhoodOptions {
    /// Note path, stem, alias, or Obsidian reference used as the graph center.
    pub target: String,
    /// Number of resolved local link hops to traverse from the target.
    #[serde(default = "default_graph_neighborhood_depth")]
    pub depth: usize,
    /// Edge direction to traverse.
    #[serde(default)]
    pub direction: GraphNeighborhoodDirection,
    /// Include unresolved and ambiguous edges touching returned nodes.
    #[serde(default)]
    pub include_unresolved: bool,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GraphNeighborhoodDirection {
    Out,
    In,
    #[default]
    Both,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct GraphNode {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct GraphEdge {
    pub source: SearchSource,
    pub from: String,
    pub to: String,
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
/// A flat, path-oriented view of visible vault files.
///
/// Directory nesting is intentionally omitted because each file path already
/// carries the useful location context for LLM agents.
pub struct VaultFilesResult {
    /// Whole-vault counts after ignore/exclude filtering.
    pub summary: VaultFilesSummary,
    /// Visible files, sorted by natural vault-relative path order.
    pub files: Vec<VaultFile>,
    /// Number of files omitted because max_files was reached.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub truncated_files: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
/// Aggregate counts for the returned file list.
pub struct VaultFilesSummary {
    /// Markdown files.
    pub notes: usize,
    /// Unique parent directories containing returned files, including root when applicable.
    pub directories: usize,
    /// Non-Markdown files.
    pub attachments: usize,
    /// Always zero for the flat file list.
    pub empty_directories: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
/// Options controlling how much of the flat vault file list is returned.
pub struct VaultFilesOptions {
    /// Include Markdown notes.
    #[serde(default = "default_true")]
    pub include_files: bool,
    /// Include non-Markdown files such as images.
    #[serde(default)]
    pub include_attachments: bool,
    /// Include selectable non-H1 heading titles for README.md files.
    #[serde(default)]
    pub include_readme_outline: bool,
    /// Maximum number of visible file entries returned.
    #[serde(default = "default_file_limit", alias = "max_children_per_dir")]
    pub max_files: usize,
}

impl Default for VaultFilesOptions {
    fn default() -> Self {
        Self {
            include_files: true,
            include_attachments: false,
            include_readme_outline: false,
            max_files: default_file_limit(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
/// One visible file in the vault.
pub struct VaultFile {
    /// Vault-relative file path.
    pub path: String,
    /// Markdown note or attachment.
    pub kind: VaultFileKind,
    /// Display title for note files. Priority: first level-one heading, frontmatter title, then pathname.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Flat list of README selectable non-H1 heading titles when include_readme_outline is true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outline: Option<Vec<String>>,
    /// Human-readable file size using binary units.
    pub size: String,
    /// Last modified time in the current system timezone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// Kind of file represented by the flat vault file list.
pub enum VaultFileKind {
    /// A Markdown note.
    Note,
    /// A non-Markdown file, such as an image.
    Attachment,
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
    suggested_note_path(&reference.target, notes)
        .map(|path| format!("{path}{}", reference_suffix(&reference.reference)))
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

fn read_snippet(file: &NoteFile, source: &SourceSpan) -> String {
    fs::read_to_string(&file.path)
        .ok()
        .map(|content| slice_text(&content, source.byte_start, source.byte_end))
        .unwrap_or_default()
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect()
}

fn default_file_limit() -> usize {
    100
}

fn default_graph_neighborhood_depth() -> usize {
    1
}

fn default_true() -> bool {
    true
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}
