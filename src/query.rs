use std::{fs, sync::Arc};

use camino::Utf8Path;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::parser::{
    BlockInfo, EmbedInfo, HeadingInfo, LinkInfo, ParsedNote, SectionInfo, SourceSpan, TagInfo,
    slice_text,
};
use crate::resolver::{IndexedNote, RefResolver, ResolveResult};
use crate::vault::{NoteFile, Vault};

use self::cache::ParseCache;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ListNotesResult {
    pub notes: Vec<NoteSummary>,
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
    pub content: String,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
/// Compact parse result for tool and CLI output.
///
/// The full parser keeps repeated source metadata for internal use, but this
/// shape avoids repeating the note path and full section object on every item.
pub struct ParseNoteResult {
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

impl From<ParsedNote> for ParseNoteResult {
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

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SearchSource {
    /// Vault-relative note path.
    pub path: String,
    /// 1-based start line.
    pub line_start: u64,
    /// 1-based end line.
    pub line_end: u64,
    /// Nearest containing heading, when available.
    pub section: Option<SectionInfo>,
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

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct TagsResult {
    pub tags: Vec<TagBucket>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct CompactTagsResult {
    pub tags: Vec<CompactTagBucket>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct CompactTagBucket {
    pub tag: String,
    pub notes: Vec<CompactTagMatch>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct CompactTagMatch {
    /// Vault-relative path, with :line suffix for body tags.
    pub note: String,
    pub source_kind: TagSourceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct DetailedTagsResult {
    pub tags: Vec<DetailedTagBucket>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct DetailedTagBucket {
    pub tag: String,
    pub occurrences: Vec<DetailedTagOccurrence>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct DetailedTagOccurrence {
    /// Vault-relative path, with :line or :start-end suffix when line data exists.
    pub location: String,
    pub source_kind: TagSourceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<DetailedSection>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct DetailedSection {
    pub heading: String,
    pub heading_level: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heading_path: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum TagsOutput {
    Compact(CompactTagsResult),
    Verbose(DetailedTagsResult),
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct TagBucket {
    pub tag: String,
    pub notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub occurrences: Vec<TagOccurrence>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
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
    pub source: SearchSource,
    pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ReadSectionResult {
    pub note: String,
    pub selector: SectionSelector,
    pub source: SourceSpan,
    pub content: String,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SectionSelector {
    Heading { heading: String },
    Block { block_id: String },
    Lines { line_start: u64, line_end: u64 },
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
pub struct NoteGraphResult {
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
    /// Include heading titles for README.md files.
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
    /// First Markdown heading for note files.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Flat list of README heading titles when include_readme_outline is true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outline: Option<Vec<String>>,
    /// Human-readable file size using binary units.
    pub size: String,
    /// Last modified time in the current system timezone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
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
    parse_cache: Arc<ParseCache>,
}

impl VaultQueries {
    pub fn new(vault: Vault) -> Self {
        let parse_cache = Arc::new(ParseCache::new(
            vault.config.parse_cache_ttl_secs,
            vault.config.parse_cache_max_entries,
        ));
        Self { vault, parse_cache }
    }

    fn parse_file_cached(
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

fn find_indexed_note<'a>(note: &str, notes: &'a [IndexedNote]) -> anyhow::Result<&'a IndexedNote> {
    match RefResolver::resolve(note, notes) {
        ResolveResult::Resolved { path, .. } => notes
            .iter()
            .find(|note| note.file.relative_path == path)
            .ok_or_else(|| anyhow::anyhow!("resolved note disappeared")),
        ResolveResult::Ambiguous { candidates, .. } => Err(anyhow::anyhow!(
            "ambiguous note reference: {:?}",
            candidates
        )),
        ResolveResult::Unresolved { .. } => Err(anyhow::anyhow!("unresolved note reference")),
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

mod cache;
mod context;
mod files;
mod graph;
mod links;
mod notes;
mod outline;
mod search;
mod section;

#[cfg(test)]
mod tests;
