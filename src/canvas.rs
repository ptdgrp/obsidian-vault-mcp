//! JSON Canvas semantic conversion. Geometry only determines group membership.
use crate::{
    query::{McpNonNegativeInteger, VaultQueries},
    rendered::{RenderPagination, default_page, paginate, read_source},
};
use anyhow::{Context, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct ReadCanvasRequest {
    /// Exact workspace-relative .canvas path; no node or source-line fragment.
    pub path: String,
    #[serde(default = "default_page")]
    #[schemars(with = "McpNonNegativeInteger", range(min = 1))]
    pub page: usize,
    /// Unicode-character page budget. Complete nodes and their edge context may exceed it.
    #[schemars(with = "Option<McpNonNegativeInteger>", range(min = 0))]
    pub max_chars: Option<usize>,
}
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct ReadCanvasResult {
    pub path: String,
    /// Always mermaid. Every page is a complete flowchart, without Markdown fences.
    pub output_format: String,
    pub content: String,
    pub pagination: RenderPagination,
}

#[derive(Debug, Deserialize)]
struct Canvas {
    #[serde(default)]
    nodes: Vec<Node>,
    #[serde(default)]
    edges: Vec<Edge>,
}
#[derive(Debug, Deserialize)]
struct Node {
    id: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    #[serde(flatten)]
    kind: NodeKind,
}
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum NodeKind {
    Text {
        text: String,
    },
    File {
        file: String,
        subpath: Option<String>,
    },
    Link {
        url: String,
    },
    Group {
        label: Option<String>,
        background: Option<String>,
        #[serde(rename = "backgroundStyle")]
        background_style: Option<BackgroundStyle>,
    },
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum BackgroundStyle {
    Cover,
    Ratio,
    Repeat,
}
#[derive(Debug, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
enum End {
    #[default]
    None,
    Arrow,
}
fn default_arrow() -> End {
    End::Arrow
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Side {
    Top,
    Right,
    Bottom,
    Left,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Edge {
    id: String,
    from_node: String,
    to_node: String,
    #[serde(default)]
    from_end: End,
    #[serde(default = "default_arrow")]
    to_end: End,
    #[allow(dead_code)]
    from_side: Option<Side>,
    #[allow(dead_code)]
    to_side: Option<Side>,
    label: Option<String>,
}
enum Unit {
    Node(usize),
    Edge(usize),
    Member(usize, usize),
}

impl VaultQueries {
    pub fn read_canvas(&self, request: ReadCanvasRequest) -> anyhow::Result<ReadCanvasResult> {
        let (path, source) = read_source(&self.vault, &request.path, "canvas")?;
        let canvas: Canvas = serde_json::from_str(&source)
            .with_context(|| format!("invalid JSON Canvas: {path}"))?;
        let (units, warnings, ids) =
            validate(&canvas).with_context(|| format!("invalid JSON Canvas: {path}"))?;
        let budget = request
            .max_chars
            .unwrap_or(self.vault.config().max_read_note_chars);
        // Keep group-inference diagnostics in the semantic content, within the
        // same budget as nodes and connections, without a second metadata list.
        let comments: String = warnings
            .iter()
            .map(|warning| format!("%% {}\n", escape_label(warning)))
            .collect();
        let (content, pagination, _) = paginate(&units, request.page, budget, |units| {
            format!("{comments}{}", render(&canvas, &ids, units))
        })?;
        Ok(ReadCanvasResult {
            path,
            output_format: "mermaid".into(),
            content,
            pagination,
        })
    }
}

type Validated = (Vec<Unit>, Vec<String>, HashMap<String, usize>);
fn validate(canvas: &Canvas) -> anyhow::Result<Validated> {
    let mut ids = HashMap::new();
    let mut units = Vec::new();
    let mut warnings = Vec::new();
    for (i, node) in canvas.nodes.iter().enumerate() {
        if node.id.is_empty() || ids.insert(node.id.clone(), i).is_some() {
            bail!("duplicate or empty node id: {:?}", node.id);
        }
        if ![node.x, node.y, node.width, node.height]
            .iter()
            .all(|n| n.is_finite())
            || node.width < 0.0
            || node.height < 0.0
        {
            bail!("invalid rectangle for node {:?}", node.id);
        }
        if let NodeKind::File {
            subpath: Some(subpath),
            ..
        } = &node.kind
            && !subpath.starts_with('#')
        {
            bail!("file node {:?} subpath must begin with #", node.id);
        }
        units.push(Unit::Node(i));
    }
    let mut edges = HashSet::new();
    for (i, edge) in canvas.edges.iter().enumerate() {
        if edge.id.is_empty() || !edges.insert(&edge.id) {
            bail!("duplicate or empty edge id: {:?}", edge.id);
        }
        for endpoint in [&edge.from_node, &edge.to_node] {
            if !ids.contains_key(endpoint) {
                bail!("edge {:?} has missing endpoint {:?}", edge.id, endpoint);
            }
        }
        units.push(Unit::Edge(i));
    }
    for (g, group) in canvas
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| matches!(n.kind, NodeKind::Group { .. }))
    {
        for (i, node) in canvas.nodes.iter().enumerate() {
            if i == g {
                continue;
            }
            let contains = group.x <= node.x
                && group.y <= node.y
                && group.x + group.width >= node.x + node.width
                && group.y + group.height >= node.y + node.height;
            let coincident = group.x == node.x
                && group.y == node.y
                && group.width == node.width
                && group.height == node.height;
            if contains && !(coincident && matches!(node.kind, NodeKind::Group { .. })) {
                units.push(Unit::Member(g, i));
            }
            if coincident && matches!(node.kind, NodeKind::Group { .. }) && g < i {
                warnings.push(format!("Groups {:?} and {:?} have identical bounds; no parent relationship is inferred.", group.id, node.id));
            }
        }
    }
    Ok((units, warnings, ids))
}

fn render(canvas: &Canvas, ids: &HashMap<String, usize>, units: &[Unit]) -> String {
    // Disable HTML labels: source text containing HTML is literal content.
    let mut out =
        "---\nconfig:\n  flowchart:\n    htmlLabels: false\n---\nflowchart LR\n".to_string();
    let mut needed = BTreeSet::new();
    for unit in units {
        match unit {
            Unit::Node(i) => {
                needed.insert(*i);
            }
            Unit::Edge(i) => {
                let edge = &canvas.edges[*i];
                needed.insert(ids[&edge.from_node]);
                needed.insert(ids[&edge.to_node]);
            }
            Unit::Member(g, n) => {
                needed.insert(*g);
                needed.insert(*n);
            }
        }
    }
    for i in needed {
        out.push_str(&format!(
            "  n{i}[\"{}\"]\n",
            escape_label(&node_text(&canvas.nodes[i]))
        ));
    }
    for unit in units {
        match unit {
            Unit::Node(_) => {}
            Unit::Member(g, n) => out.push_str(&format!("  n{g} -. contains .-> n{n}\n")),
            Unit::Edge(i) => {
                let edge = &canvas.edges[*i];
                let connector = match (&edge.from_end, &edge.to_end) {
                    (End::None, End::None) => "---",
                    (End::None, End::Arrow) => "-->",
                    (End::Arrow, End::None) => "-->",
                    (End::Arrow, End::Arrow) => "<-->",
                };
                let label = edge
                    .label
                    .as_ref()
                    .map(|s| format!("|\"{}\"|", escape_label(s)))
                    .unwrap_or_default();
                // Mermaid's reverse-link grammar rejects a pipe label after
                // <--. Swap endpoints to preserve the arrow's actual meaning.
                let (from, to) = if edge.from_end == End::Arrow && edge.to_end == End::None {
                    (&edge.to_node, &edge.from_node)
                } else {
                    (&edge.from_node, &edge.to_node)
                };
                out.push_str(&format!(
                    "  n{} {connector}{label} n{}\n",
                    ids[from], ids[to]
                ));
            }
        }
    }
    out
}
fn node_text(node: &Node) -> String {
    match &node.kind {
        NodeKind::Text { text } => format!("Text\n{text}"),
        NodeKind::File { file, subpath } => {
            let ext = camino::Utf8Path::new(file)
                .extension()
                .unwrap_or("")
                .to_lowercase();
            let kind = match ext.as_str() {
                "md" => "Markdown file",
                "base" => "Base file",
                "canvas" => "Canvas file",
                "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "bmp" | "avif" => "Image file",
                "mp4" | "webm" | "mov" | "mkv" => "Video attachment",
                "mp3" | "wav" | "ogg" | "m4a" | "flac" => "Audio attachment",
                _ => "File attachment",
            };
            format!(
                "{kind}\nPath: {file}{}",
                subpath
                    .as_ref()
                    .map(|s| format!("\nSubpath: {s}"))
                    .unwrap_or_default()
            )
        }
        NodeKind::Link { url } => format!("URL\n{url}"),
        NodeKind::Group {
            label,
            background,
            background_style,
        } => {
            let mut value = format!("Group\n{}", label.as_deref().unwrap_or("(unnamed)"));
            if let Some(path) = background {
                value.push_str(&format!("\nBackground image: {path}"));
            }
            if let Some(style) = background_style {
                value.push_str(&format!(
                    "\nBackground style: {}",
                    match style {
                        BackgroundStyle::Cover => "cover",
                        BackgroundStyle::Ratio => "ratio",
                        BackgroundStyle::Repeat => "repeat",
                    }
                ));
            }
            value
        }
    }
}
/// Mermaid numeric entities protect syntax, Markdown, and literal HTML alike.
fn escape_label(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '\n' => out.push_str("<br/>"),
            '"' | '&' | '<' | '>' | '#' | '`' | '|' | '[' | ']' | '{' | '}' | '\\' | '\r' => {
                out.push_str(&format!("#{};", ch as u32))
            }
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests;
