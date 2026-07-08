mod cache;
mod categories;
mod context;
mod files;
mod graph;
mod links;
mod notes;
mod outline;
mod search;
pub(crate) mod section;

#[cfg(test)]
mod tests;

use std::{borrow::Cow, fs, sync::Arc};

use camino::Utf8Path;
use clap::ValueEnum;
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize, Serializer, ser::SerializeStruct};

use crate::parser::{
    BlockInfo, EmbedInfo, HeadingInfo, LinkInfo, ParsedNote, SectionInfo, SourceSpan, TagInfo,
    path_with_line_ref, slice_text,
};
use crate::resolver::{IndexedNote, RefResolver, ResolveResult};
use crate::vault::{NoteFile, Vault};

use self::cache::ParseCache;

pub use crate::parser::TagScope;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ListNotesResult {
    pub notes: Vec<NoteSummary>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct NoteSummary {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Human-readable file size using binary units.
    pub size: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ReadNoteResult {
    pub path: String,
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
    /// Total number of inbound links to this note across the visible vault.
    pub backlink_count: usize,
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
    pub path_glob: Option<String>,
    pub matches: Vec<TextMatch>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct TextMatch {
    /// Lightweight source for LLM navigation. Byte offsets are intentionally omitted.
    pub source: SearchSource,
    /// Short preview text. Use read_section for full evidence.
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
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ResolveSummary {
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

impl From<ResolveResult> for ResolveSummary {
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

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct BacklinksOutput {
    pub target: String,
    pub resolution: ResolveSummary,
    pub backlinks: Vec<LinkEvidenceOutput>,
    pub truncated: bool,
}

impl BacklinksOutput {
    pub fn from_result(result: BacklinksResult, verbose: bool) -> Self {
        Self {
            target: result.target,
            resolution: result.resolution,
            backlinks: result
                .backlinks
                .into_iter()
                .map(|evidence| LinkEvidenceOutput::from_evidence(evidence, verbose))
                .collect(),
            truncated: result.truncated,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct OutlinksOutput {
    pub note: String,
    pub links: Vec<LinkEvidenceOutput>,
}

impl OutlinksOutput {
    pub fn from_result(result: OutlinksResult, verbose: bool) -> Self {
        Self {
            note: result.note,
            links: result
                .links
                .into_iter()
                .map(|evidence| LinkEvidenceOutput::from_evidence(evidence, verbose))
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct LinkEvidenceOutput {
    pub location: String,
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub resolved: ResolveSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SearchSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

impl LinkEvidenceOutput {
    pub fn from_evidence(evidence: LinkEvidence, verbose: bool) -> Self {
        let location = link_location(&evidence.source);
        let section = evidence
            .source
            .section
            .as_ref()
            .cloned()
            .map(compact_section);
        Self {
            location,
            target: evidence.target,
            alias: evidence.alias,
            resolved: evidence.resolved,
            section,
            source: verbose.then_some(evidence.source),
            snippet: verbose.then_some(evidence.snippet),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct BacklinksResult {
    pub target: String,
    pub resolution: ResolveSummary,
    pub backlinks: Vec<LinkEvidence>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct OutlinksResult {
    pub note: String,
    pub links: Vec<LinkEvidence>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct LinkEvidence {
    pub source: SearchSource,
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub resolved: ResolveSummary,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub snippet: String,
}

fn link_location(source: &SearchSource) -> String {
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
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TagSourceKind {
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

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ReadSectionResult {
    pub note: String,
    pub selector: SectionSelector,
    pub source: SourceSpan,
    pub content: String,
    pub truncated: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SectionSelector {
    Heading { heading: String },
    Block { block_id: String },
    Lines { line_start: u64, line_end: u64 },
}

impl Serialize for SectionSelector {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            SectionSelector::Heading { heading } => {
                let mut state = serializer.serialize_struct("SectionSelector", 2)?;
                state.serialize_field("kind", "heading")?;
                state.serialize_field("heading", heading)?;
                state.end()
            }
            SectionSelector::Block { block_id } => {
                let mut state = serializer.serialize_struct("SectionSelector", 2)?;
                state.serialize_field("kind", "block")?;
                state.serialize_field("block_id", block_id)?;
                state.end()
            }
            SectionSelector::Lines {
                line_start: _,
                line_end: _,
            } => {
                let mut state = serializer.serialize_struct("SectionSelector", 1)?;
                state.serialize_field("kind", "lines")?;
                state.end()
            }
        }
    }
}

impl JsonSchema for SectionSelector {
    fn schema_name() -> Cow<'static, str> {
        "SectionSelector".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        serde_json::json!({
            "oneOf": [
                {
                    "type": "object",
                    "properties": {
                        "kind": { "const": "heading" },
                        "heading": { "type": "string" }
                    },
                    "required": ["kind", "heading"]
                },
                {
                    "type": "object",
                    "properties": {
                        "kind": { "const": "block" },
                        "block_id": { "type": "string" }
                    },
                    "required": ["kind", "block_id"]
                },
                {
                    "type": "object",
                    "properties": {
                        "kind": { "const": "lines" }
                    },
                    "required": ["kind"]
                }
            ]
        })
        .try_into()
        .expect("section selector schema")
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
        ResolveResult::Unresolved { reference } => Err(anyhow::anyhow!(
            "unresolved note reference: {:?}",
            reference.raw
        )),
    }
}

fn read_snippet(file: &NoteFile, source: &SourceSpan) -> String {
    fs::read_to_string(&file.path)
        .ok()
        .map(|content| slice_text(&content, source.byte_start, source.byte_end))
        .unwrap_or_default()
}

fn truncate_utf8(input: &str, max_bytes: usize) -> &str {
    if input.len() <= max_bytes {
        return input;
    }
    let mut end = max_bytes;
    while !input.is_char_boundary(end) {
        end -= 1;
    }
    &input[..end]
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
