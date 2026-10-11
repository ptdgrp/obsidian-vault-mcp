use super::{dates, value::Value};
use crate::{parser::ParsedNote, query::VaultQueries, vault::NoteFile};
use anyhow::{Context, bail};
use chrono::{DateTime, Local};
use indexmap::IndexMap;
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    fs,
    sync::Arc,
};

pub struct Dataset<'a> {
    pub queries: &'a VaultQueries,
    files: RefCell<Vec<NoteFile>>,
    parsed: RefCell<HashMap<usize, Arc<ParsedNote>>>,
    properties: RefCell<HashMap<usize, IndexMap<String, Value>>>,
    link_cache: RefCell<HashMap<(usize, bool), Vec<Value>>>,
    property_types: BTreeMap<String, String>,
    pub candidates: usize,
}
impl<'a> Dataset<'a> {
    pub fn new(queries: &'a VaultQueries) -> anyhow::Result<Self> {
        let files = queries.vault.list_files()?;
        let mut property_types = BTreeMap::new();
        if queries.vault.root.join(".obsidian/types.json").is_file() {
            let (_, source) =
                crate::rendered::read_source(&queries.vault, ".obsidian/types.json", "json")?;
            let json: serde_json::Value =
                serde_json::from_str(&source).context("invalid .obsidian/types.json")?;
            if let Some(types) = json.get("types") {
                property_types = serde_json::from_value(types.clone())
                    .context("invalid property types in .obsidian/types.json")?;
            }
        }
        Ok(Self {
            candidates: files.len(),
            queries,
            files: RefCell::new(files),
            parsed: RefCell::new(HashMap::new()),
            properties: RefCell::new(HashMap::new()),
            link_cache: RefCell::new(HashMap::new()),
            property_types,
        })
    }
    pub fn file(&self, index: usize) -> NoteFile {
        self.files.borrow()[index].clone()
    }
    pub fn exact(&self, path: &str) -> anyhow::Result<usize> {
        let path = self.queries.vault.resolve_existing_file(path)?;
        let relative = self.queries.vault.relative_path(&path);
        if let Some(index) = self
            .files
            .borrow()
            .iter()
            .position(|file| file.relative_path == relative)
        {
            return Ok(index);
        }
        let metadata = fs::metadata(&path)?;
        let mut files = self.files.borrow_mut();
        let index = files.len();
        files.push(NoteFile {
            path,
            relative_path: relative,
            size_bytes: metadata.len(),
            modified_unix_ms: metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64),
        });
        Ok(index)
    }
    pub fn resolve(&self, target: &str, source: usize) -> anyhow::Result<Option<usize>> {
        let (target, _) = link_parts(target);
        let target = target.split('#').next().unwrap_or("");
        if target.contains("://") || target.starts_with("mailto:") {
            return Ok(None);
        }
        if target.is_empty() {
            return Ok(Some(source));
        }
        let source_file = self.file(source);
        let folder = source_file
            .path
            .parent()
            .unwrap()
            .strip_prefix(&self.queries.vault.root)
            .unwrap_or(camino::Utf8Path::new(""));
        let relative = folder.join(target);
        let exact_candidates = if target.starts_with("./") || target.starts_with("../") {
            vec![relative.to_string()]
        } else {
            vec![target.to_string(), relative.to_string()]
        };
        for path in exact_candidates {
            for candidate in [path.clone(), format!("{path}.md")] {
                if self.queries.vault.resolve_existing_file(&candidate).is_ok() {
                    return self.exact(&candidate).map(Some);
                }
            }
        }
        // Obsidian short paths: choose the closest directory, diagnose equal ties.
        let target = target.trim_start_matches('/');
        let files = self.files.borrow();
        let candidates: Vec<_> = files
            .iter()
            .enumerate()
            .filter(|(_, file)| {
                file.relative_path == target
                    || file.relative_path == format!("{target}.md")
                    || file.relative_path.ends_with(&format!("/{target}"))
                    || file.relative_path.ends_with(&format!("/{target}.md"))
            })
            .map(|(i, file)| {
                let common = file
                    .relative_path
                    .split('/')
                    .zip(source_file.relative_path.split('/'))
                    .take_while(|(a, b)| a == b)
                    .count();
                (common, i)
            })
            .collect();
        let Some(best) = candidates.iter().map(|(score, _)| *score).max() else {
            return Ok(None);
        };
        let best: Vec<_> = candidates
            .into_iter()
            .filter(|(score, _)| *score == best)
            .collect();
        if best.len() > 1 {
            bail!(
                "ambiguous link {target:?} from {}",
                source_file.relative_path
            );
        }
        Ok(Some(best[0].1))
    }
    pub fn link(&self, text: &str, display: Option<Value>, source: usize) -> anyhow::Result<Value> {
        let (path, alias) = link_parts(text);
        let resolved = self.resolve(&path, source)?;
        Ok(Value::Link {
            path,
            display: display.or_else(|| alias.map(Value::String)).map(Box::new),
            resolved,
        })
    }
    fn parse(&self, index: usize) -> anyhow::Result<Option<Arc<ParsedNote>>> {
        let file = self.file(index);
        if file.path.extension() != Some("md") {
            return Ok(None);
        }
        if let Some(parsed) = self.parsed.borrow().get(&index) {
            return Ok(Some(parsed.clone()));
        }
        let (parsed, _) = self
            .queries
            .parse_note_from_path(&file.path, &file.relative_path)?;
        self.parsed.borrow_mut().insert(index, parsed.clone());
        Ok(Some(parsed))
    }
    pub fn props(&self, index: usize) -> anyhow::Result<IndexMap<String, Value>> {
        if let Some(properties) = self.properties.borrow().get(&index) {
            return Ok(properties.clone());
        }
        let file = self.file(index);
        let mut properties = IndexMap::new();
        if file.path.extension() == Some("md") {
            let source = self.queries.read_note_content(&file.path)?;
            let normalized = source.strip_prefix('\u{feff}').unwrap_or(&source);
            let mut lines = normalized.split_inclusive('\n');
            if lines.next().is_some_and(|line| line.trim_end() == "---") {
                let mut yaml = String::new();
                let mut closed = false;
                for line in lines {
                    if ["---", "..."].contains(&line.trim_end()) {
                        closed = true;
                        break;
                    }
                    yaml.push_str(line);
                }
                if !closed {
                    bail!("unclosed YAML frontmatter in {}", file.relative_path);
                }
                if !yaml.trim().is_empty() {
                    let json: serde_json::Value =
                        serde_yaml_ng::from_str(&yaml).with_context(|| {
                            format!("invalid YAML frontmatter in {}", file.relative_path)
                        })?;
                    let object = json.as_object().with_context(|| {
                        format!("frontmatter must be an object in {}", file.relative_path)
                    })?;
                    for (key, value) in object {
                        properties.insert(
                            key.clone(),
                            self.convert(
                                value,
                                index,
                                self.property_types.get(key).map(String::as_str),
                            )
                            .with_context(|| {
                                format!("property {key:?} in {}", file.relative_path)
                            })?,
                        );
                    }
                }
            }
        }
        self.properties
            .borrow_mut()
            .insert(index, properties.clone());
        Ok(properties)
    }
    pub fn convert(
        &self,
        json: &serde_json::Value,
        index: usize,
        kind: Option<&str>,
    ) -> anyhow::Result<Value> {
        if !json.is_null() && matches!(kind, Some("date" | "datetime")) {
            let date = dates::parse_date(json.as_str().context("date property must be a string")?)?;
            return Ok(if kind == Some("date") {
                Value::DateOnly(date)
            } else {
                Value::Date(date)
            });
        }
        Ok(match json {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(v) => Value::Bool(*v),
            serde_json::Value::Number(v) => {
                Value::Number(v.as_f64().context("number out of range")?)
            }
            serde_json::Value::String(v) => {
                if v.starts_with("[[") && v.ends_with("]]") {
                    self.link(v, None, index)?
                } else {
                    Value::String(v.clone())
                }
            }
            serde_json::Value::Array(v) => Value::List(
                v.iter()
                    .map(|v| self.convert(v, index, None))
                    .collect::<anyhow::Result<_>>()?,
            ),
            serde_json::Value::Object(v) => Value::Object(
                v.iter()
                    .map(|(k, v)| Ok((k.clone(), self.convert(v, index, None)?)))
                    .collect::<anyhow::Result<_>>()?,
            ),
        })
    }
    pub fn tags(&self, index: usize) -> anyhow::Result<Vec<Value>> {
        let mut tags = Vec::new();
        if let Some(parsed) = self.parse(index)? {
            tags.extend(
                parsed
                    .tags
                    .iter()
                    .map(|tag| tag.tag.trim_start_matches('#').to_string()),
            );
        }
        let properties = self.props(index)?;
        fn add(value: &Value, tags: &mut Vec<String>) {
            match value {
                Value::String(v) => tags.extend(
                    v.split(|c: char| c.is_whitespace() || c == ',')
                        .filter(|s| !s.is_empty())
                        .map(|s| s.trim_start_matches('#').into()),
                ),
                Value::List(v) => {
                    for v in v {
                        add(v, tags)
                    }
                }
                _ => {}
            }
        }
        for name in ["tags", "tag"] {
            if let Some(value) = properties.get(name) {
                add(value, &mut tags);
            }
        }
        tags.sort();
        tags.dedup();
        Ok(tags.into_iter().map(Value::String).collect())
    }
    pub fn links(&self, index: usize, embeds: bool) -> anyhow::Result<Vec<Value>> {
        if let Some(links) = self.link_cache.borrow().get(&(index, embeds)) {
            return Ok(links.clone());
        }
        let mut links = Vec::new();
        if self.file(index).path.extension() == Some("md") {
            if !embeds {
                for value in self.props(index)?.values() {
                    self.property_links(value, index, &mut links)?;
                }
            }
            // Inspect the actual AST in source order, preserving repeated
            // references. Code blocks cannot create fictitious links/images.
            let file = self.file(index);
            let source = self.queries.read_note_content(&file.path)?;
            links.extend(self.markdown_references(&source, index, embeds)?);
        } else if self.file(index).path.extension() == Some("canvas") {
            let file = self.file(index);
            let source = self.queries.read_note_content(&file.path)?;
            let json: serde_json::Value = serde_json::from_str(&source)
                .with_context(|| format!("invalid canvas {}", file.relative_path))?;
            if let Some(nodes) = json.get("nodes").and_then(serde_json::Value::as_array) {
                for node in nodes {
                    if let Some(path) = node.get("file").and_then(serde_json::Value::as_str) {
                        links.push(self.link(path, None, index)?);
                    }
                }
            }
        }
        self.link_cache
            .borrow_mut()
            .insert((index, embeds), links.clone());
        Ok(links)
    }
    fn property_links(
        &self,
        value: &Value,
        index: usize,
        links: &mut Vec<Value>,
    ) -> anyhow::Result<()> {
        match value {
            Value::Link {
                path,
                display,
                resolved,
            } => links.push(Value::Link {
                path: path.clone(),
                display: display
                    .clone()
                    .or_else(|| Some(Box::new(Value::String(link_display(path))))),
                resolved: *resolved,
            }),
            Value::List(items) => {
                for value in items {
                    self.property_links(value, index, links)?;
                }
            }
            Value::Object(items) => {
                for value in items.values() {
                    self.property_links(value, index, links)?;
                }
            }
            Value::String(text) if text.contains('[') => {
                links.extend(self.markdown_references(text, index, false)?)
            }
            _ => {}
        }
        Ok(())
    }
    fn markdown_references(
        &self,
        source: &str,
        index: usize,
        embeds: bool,
    ) -> anyhow::Result<Vec<Value>> {
        use ptdgrp_markdown::{MarkdownNode, link::Link, reference::Reference};
        fn with_reference(path: &str, reference: &Option<Reference>) -> String {
            format!(
                "{path}{}",
                match reference {
                    None => String::new(),
                    Some(Reference::Heading(v)) => format!("#{v}"),
                    Some(Reference::MultiHeading(v)) => format!("#{}", v.join("#")),
                    Some(Reference::BlockId(v)) => format!("#^{v}"),
                }
            )
        }
        let options = ptdgrp_markdown::ParserOptions::default()
            .enabled_gfm()
            .enabled_ofm()
            .with_max_input_bytes(self.queries.vault.config().max_note_bytes);
        let document = ptdgrp_markdown::Parser::new_with_options(source, options)
            .parse()
            .map_err(|error| anyhow::anyhow!("{error:?}"))?;
        let file = self.file(index);
        let mut links = Vec::new();
        for node in crate::parser::active_node_indices(&document) {
            let mut markdown = false;
            let pair = match &document.tree[node].body {
                MarkdownNode::Link(link) if !embeds => match link.as_ref() {
                    Link::Wikilink(link) => Some((
                        with_reference(&link.path, &link.reference),
                        link.text.clone(),
                    )),
                    Link::Default(link) => {
                        markdown = true;
                        Some((
                            document.text(&link.url).into(),
                            Some(crate::parser::collect_text(&document, node)),
                        ))
                    }
                    _ => None,
                },
                MarkdownNode::Embed(embed) => {
                    let raw = source
                        .get(
                            document.tree[node].span.start as usize
                                ..document.tree[node].span.end as usize,
                        )
                        .unwrap_or("");
                    let (_, alias) = link_parts(raw.strip_prefix('!').unwrap_or(raw));
                    Some((with_reference(&embed.path, &embed.reference), alias))
                }
                MarkdownNode::Image(image) => {
                    markdown = true;
                    Some((
                        document.text(&image.url).into(),
                        Some(crate::parser::collect_text(&document, node)),
                    ))
                }
                _ => None,
            };
            if let Some((path, display)) = pair {
                if markdown {
                    if let Some((target, reference)) =
                        crate::parser::local_markdown_link_target(&file.relative_path, &path)
                    {
                        let resolved = self.resolve(
                            &crate::query::reference_display(&target, &reference),
                            index,
                        )?;
                        links.push(Value::Link {
                            path: path.clone(),
                            display: Some(Box::new(Value::String(
                                display
                                    .filter(|s| !s.is_empty())
                                    .unwrap_or_else(|| link_display(&path)),
                            ))),
                            resolved,
                        });
                    }
                } else if !path.contains("://") && !path.starts_with("mailto:") {
                    let display = display
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| link_display(&path));
                    links.push(self.link(&path, Some(Value::String(display)), index)?);
                }
            }
        }
        Ok(links)
    }
    pub fn has_link(&self, index: usize, other: &Value) -> anyhow::Result<bool> {
        let other = match other {
            Value::String(path) => self.link(path, None, index)?,
            Value::File(_) | Value::This(_) | Value::Link { .. } => other.clone(),
            _ => bail!("hasLink expects a file, link, or string"),
        };
        Ok(self
            .links(index, false)?
            .iter()
            .any(|link| link.equal(&other)))
    }
    pub fn date(&self, index: usize, created: bool) -> anyhow::Result<Value> {
        let file = self.file(index);
        let metadata = fs::metadata(&file.path)?;
        let time = if created {
            metadata.created().with_context(||format!("creation time is unavailable for {}; file.ctime cannot be reproduced on this filesystem",file.relative_path))?
        } else {
            metadata.modified()?
        };
        Ok(Value::Date(DateTime::<Local>::from(time)))
    }
    pub fn display(&self, value: &Value) -> String {
        match value {
            Value::File(i) | Value::This(i) => self.file(*i).relative_path,
            Value::List(v) => v
                .iter()
                .map(|v| self.display(v))
                .collect::<Vec<_>>()
                .join(", "),
            _ => value.text(),
        }
    }
}
pub fn link_parts(text: &str) -> (String, Option<String>) {
    let text = text
        .strip_prefix("[[")
        .and_then(|s| s.strip_suffix("]]"))
        .unwrap_or(text);
    match text.split_once('|') {
        Some((path, alias)) => (path.into(), Some(alias.into())),
        None => (text.into(), None),
    }
}
fn link_display(path: &str) -> String {
    path.replace('#', " > ")
}
