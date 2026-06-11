use markdown::{
    Document, MarkdownNode, Parser, ParserOptions,
    code::Code,
    heading::{Heading, HeadingLevel},
    link::Link,
    reference::Reference,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ParsedNote {
    pub path: String,
    pub frontmatter: Option<Value>,
    pub headings: Vec<HeadingInfo>,
    pub links: Vec<LinkInfo>,
    pub embeds: Vec<EmbedInfo>,
    pub tags: Vec<TagInfo>,
    pub blocks: Vec<BlockInfo>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct HeadingInfo {
    pub text: String,
    pub level: u8,
    pub anchor: String,
    pub path: Vec<String>,
    pub source: SourceSpan,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct LinkInfo {
    pub raw: String,
    pub target: String,
    pub alias: Option<String>,
    pub reference: Option<ReferenceInfo>,
    #[serde(default)]
    pub kind: LinkKind,
    pub source: SourceSpan,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    #[default]
    Wikilink,
    Markdown,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct EmbedInfo {
    pub raw: String,
    pub target: String,
    pub reference: Option<ReferenceInfo>,
    pub source: SourceSpan,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct TagInfo {
    pub tag: String,
    pub source: SourceSpan,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct BlockInfo {
    pub id: String,
    pub source: SourceSpan,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReferenceInfo {
    Heading { value: String },
    MultiHeading { value: Vec<String> },
    BlockId { value: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct SectionInfo {
    pub heading: String,
    pub heading_level: u8,
    pub heading_path: Vec<String>,
    #[serde(skip)]
    pub heading_anchor: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct SourceSpan {
    pub path: String,
    pub line_start: u64,
    pub line_end: u64,
    #[serde(skip)]
    pub byte_start: usize,
    #[serde(skip)]
    pub byte_end: usize,
    pub section: Option<SectionInfo>,
}

#[derive(Clone, Debug, Default)]
pub struct NoteParser {}

impl NoteParser {
    pub fn parse(path: String, text: &str, max_input_bytes: usize) -> anyhow::Result<ParsedNote> {
        let options = ParserOptions::default()
            .enabled_gfm()
            .enabled_ofm()
            .enabled_cjk_autocorrect()
            .with_max_input_bytes(max_input_bytes);
        let document = Parser::new_with_options(text, options)
            .parse_checked()
            .map_err(|err| anyhow::anyhow!("{err:?}"))?;
        Ok(extract(path, text, &document))
    }
}

pub fn extract(path: String, text: &str, document: &Document) -> ParsedNote {
    let mut parsed = ParsedNote {
        path: path.clone(),
        frontmatter: None,
        headings: Vec::new(),
        links: Vec::new(),
        embeds: Vec::new(),
        tags: Vec::new(),
        blocks: Vec::new(),
    };

    let mut heading_stack: Vec<HeadingInfo> = Vec::new();
    for index in active_node_indices(document) {
        let node = &document.tree[index];
        match &node.body {
            MarkdownNode::FrontMatter(value) => {
                parsed.frontmatter = serde_json::to_value(value.as_ref()).ok();
            }
            MarkdownNode::Heading(heading) => {
                let level = heading_level(heading);
                while heading_stack.last().is_some_and(|item| item.level >= level) {
                    heading_stack.pop();
                }
                let text_value = collect_text(document, index).trim().to_string();
                let mut path_parts: Vec<String> =
                    heading_stack.iter().map(|item| item.text.clone()).collect();
                path_parts.push(text_value.clone());
                let source = source_for_node(&path, text, document, index, &heading_stack);
                let info = HeadingInfo {
                    text: text_value.clone(),
                    level,
                    anchor: heading_anchor(&text_value),
                    path: path_parts,
                    source,
                };
                heading_stack.push(info.clone());
                parsed.headings.push(info);
            }
            MarkdownNode::Link(link) => {
                let source = source_for_node(&path, text, document, index, &heading_stack);
                match link.as_ref() {
                    Link::Wikilink(wikilink) => {
                        parsed.links.push(LinkInfo {
                            raw: slice_text(text, source.byte_start, source.byte_end),
                            target: wikilink.path.clone(),
                            alias: wikilink.text.clone(),
                            reference: wikilink.reference.as_ref().map(reference_info),
                            kind: LinkKind::Wikilink,
                            source,
                        });
                    }
                    Link::Default(default_link) => {
                        if let Some((target, reference)) =
                            local_markdown_link_target(&path, &default_link.url)
                        {
                            let alias = collect_text(document, index);
                            parsed.links.push(LinkInfo {
                                raw: slice_text(text, source.byte_start, source.byte_end),
                                target,
                                alias: (!alias.trim().is_empty()).then(|| alias.trim().to_string()),
                                reference,
                                kind: LinkKind::Markdown,
                                source,
                            });
                        }
                    }
                    Link::Footnote(_) | Link::FootnoteBackref(_) => {}
                }
            }
            MarkdownNode::Embed(embed) => {
                let source = source_for_node(&path, text, document, index, &heading_stack);
                parsed.embeds.push(EmbedInfo {
                    raw: slice_text(text, source.byte_start, source.byte_end),
                    target: embed.path.clone(),
                    reference: embed.reference.as_ref().map(reference_info),
                    source,
                });
            }
            MarkdownNode::Tag(tag) => {
                let source = source_for_node(&path, text, document, index, &heading_stack);
                parsed.tags.push(TagInfo {
                    tag: tag.clone(),
                    source,
                });
            }
            MarkdownNode::Code(code) => {
                if matches!(code.as_ref(), Code::Fenced(_) | Code::Indented(_)) {
                    continue;
                }
            }
            _ => {}
        }
        if let Some(id) = node.id.as_ref() {
            let source = source_for_node(&path, text, document, index, &heading_stack);
            parsed.blocks.push(BlockInfo {
                id: id.to_string(),
                source,
            });
        }
    }

    parsed
}

fn active_node_indices(document: &Document) -> Vec<usize> {
    let mut out = Vec::new();
    let mut current = document.tree.get_first_child(0);
    while let Some(index) = current {
        collect_active_node_indices(document, index, &mut out);
        current = document.tree.get_next(index);
    }
    out
}

fn collect_active_node_indices(document: &Document, index: usize, out: &mut Vec<usize>) {
    if document.tree.is_free_node(&index) {
        return;
    }
    out.push(index);
    let mut child = document.tree.get_first_child(index);
    while let Some(child_index) = child {
        collect_active_node_indices(document, child_index, out);
        child = document.tree.get_next(child_index);
    }
}

pub fn section_at(parsed: &ParsedNote, line: u64) -> Option<SectionInfo> {
    parsed
        .headings
        .iter()
        .filter(|heading| heading.source.line_start <= line)
        .max_by_key(|heading| heading.source.line_start)
        .map(|heading| SectionInfo {
            heading: heading.text.clone(),
            heading_level: heading.level,
            heading_path: heading.path.clone(),
            heading_anchor: heading.anchor.clone(),
        })
}

pub fn source_for_line(
    path: &str,
    text: &str,
    parsed: &ParsedNote,
    line_start: u64,
    line_end: u64,
) -> SourceSpan {
    let byte_start = byte_offset_for_line(text, line_start);
    let byte_end = byte_offset_for_line(text, line_end + 1).min(text.len());
    SourceSpan {
        path: path.to_string(),
        line_start,
        line_end,
        byte_start,
        byte_end,
        section: section_at(parsed, line_start),
    }
}

pub fn slice_text(text: &str, start: usize, end: usize) -> String {
    let start = start.min(text.len());
    let end = end.min(text.len()).max(start);
    text.get(start..end).unwrap_or_default().to_string()
}

pub fn heading_anchor(text: &str) -> String {
    text.trim().replace('\n', " ")
}

fn source_for_node(
    path: &str,
    text: &str,
    document: &Document,
    index: usize,
    heading_stack: &[HeadingInfo],
) -> SourceSpan {
    let node = &document.tree[index];
    let line_start = node.start.line;
    let line_end = node.end.line.max(line_start);
    SourceSpan {
        path: path.to_string(),
        line_start,
        line_end,
        byte_start: byte_offset_for_location(text, node.start.line, node.start.column),
        byte_end: byte_offset_for_location(text, node.end.line, node.end.column).max(
            byte_offset_for_location(text, node.start.line, node.start.column),
        ),
        section: heading_stack.last().map(|heading| SectionInfo {
            heading: heading.text.clone(),
            heading_level: heading.level,
            heading_path: heading.path.clone(),
            heading_anchor: heading.anchor.clone(),
        }),
    }
}

fn collect_text(document: &Document, index: usize) -> String {
    let mut out = String::new();
    collect_text_into(document, index, &mut out);
    out
}

fn collect_text_into(document: &Document, index: usize, out: &mut String) {
    if document.tree.is_free_node(&index) {
        return;
    }
    match &document.tree[index].body {
        MarkdownNode::Text(text) => out.push_str(text),
        MarkdownNode::SoftBreak | MarkdownNode::HardBreak => out.push('\n'),
        _ => {}
    }
    let mut child = document.tree.get_first_child(index);
    while let Some(child_index) = child {
        collect_text_into(document, child_index, out);
        child = document.tree.get_next(child_index);
    }
}

fn heading_level(heading: &Heading) -> u8 {
    match heading.level() {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn reference_info(reference: &Reference) -> ReferenceInfo {
    match reference {
        Reference::Heading(value) => ReferenceInfo::Heading {
            value: value.clone(),
        },
        Reference::MultiHeading(value) => ReferenceInfo::MultiHeading {
            value: value.clone(),
        },
        Reference::BlockId(value) => ReferenceInfo::BlockId {
            value: value.clone(),
        },
    }
}

fn local_markdown_link_target(
    current_path: &str,
    url: &str,
) -> Option<(String, Option<ReferenceInfo>)> {
    if is_external_or_absolute_url(url) {
        return None;
    }
    let decoded = percent_decode(url)?;
    let (path_part, fragment) = decoded.split_once('#').unwrap_or((&decoded, ""));
    if path_part.contains('?') {
        return None;
    }

    let target = if path_part.is_empty() {
        current_path.to_string()
    } else {
        normalize_relative_markdown_path(current_path, path_part)?
    };
    let reference = if fragment.is_empty() {
        None
    } else if let Some(block_id) = fragment.strip_prefix('^') {
        Some(ReferenceInfo::BlockId {
            value: block_id.to_string(),
        })
    } else {
        Some(ReferenceInfo::Heading {
            value: fragment.to_string(),
        })
    };
    Some((target, reference))
}

fn is_external_or_absolute_url(url: &str) -> bool {
    url.starts_with('/')
        || url.starts_with('\\')
        || url.starts_with("//")
        || url.starts_with('#').then_some(false).unwrap_or_else(|| {
            url.find(':').is_some_and(|colon| {
                let before_colon = &url[..colon];
                !before_colon.is_empty()
                    && before_colon
                        .chars()
                        .next()
                        .is_some_and(|char| char.is_ascii_alphabetic())
                    && before_colon
                        .chars()
                        .all(|char| char.is_ascii_alphanumeric() || matches!(char, '+' | '-' | '.'))
            })
        })
}

fn normalize_relative_markdown_path(current_path: &str, link_path: &str) -> Option<String> {
    let mut parts = current_path
        .rsplit_once('/')
        .map(|(parent, _)| {
            parent
                .split('/')
                .filter(|segment| !segment.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    for segment in link_path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(segment.to_string()),
        }
    }

    (!parts.is_empty()).then(|| parts.join("/"))
}

fn percent_decode(input: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(input.len());
    let input_bytes = input.as_bytes();
    let mut index = 0;
    while index < input_bytes.len() {
        if input_bytes[index] == b'%' {
            let high = input_bytes.get(index + 1).copied()?;
            let low = input_bytes.get(index + 2).copied()?;
            bytes.push(hex_value(high)? * 16 + hex_value(low)?);
            index += 3;
        } else {
            bytes.push(input_bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(bytes).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn byte_offset_for_location(text: &str, line: u64, column: u64) -> usize {
    let line_offset = byte_offset_for_line(text, line);
    let line_text = text[line_offset..].lines().next().unwrap_or_default();
    line_offset
        + line_text
            .char_indices()
            .nth(column.saturating_sub(1) as usize)
            .map(|(offset, _)| offset)
            .unwrap_or(line_text.len())
}

fn byte_offset_for_line(text: &str, line: u64) -> usize {
    if line <= 1 {
        return 0;
    }
    let mut current_line = 1_u64;
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            current_line += 1;
            if current_line == line {
                return index + 1;
            }
        }
    }
    text.len()
}
