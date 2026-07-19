use std::ops::Range;

use markdown::{
    Document, MarkdownNode, Parser, ParserOptions, ast::heading::HeadingLevel, parser::Location,
};

#[derive(Clone, Copy)]
pub(crate) struct DocumentSchema {
    pub name: &'static str,
    pub required_sections: &'static [&'static str],
}

#[derive(Clone, Debug)]
pub(crate) struct Section {
    pub title: String,
    body_start: usize,
    body_end: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct ParsedDocument {
    h1: String,
    frontmatter: serde_json::Map<String, serde_json::Value>,
    frontmatter_range: Range<usize>,
    sections: Vec<Section>,
}

impl ParsedDocument {
    pub(crate) fn parse(path: &str, source: &str, schema: DocumentSchema) -> anyhow::Result<Self> {
        if !path.ends_with(".md") {
            anyhow::bail!("document path must end with .md: {path}");
        }
        let document =
            Parser::new_with_options(source, ParserOptions::default().enabled_gfm().enabled_ofm())
                .parse_checked()
                .map_err(|error| anyhow::anyhow!("invalid Markdown: {error:?}"))?;

        let mut h1 = None;
        let mut frontmatter = None;
        let mut headings = Vec::new();
        for index in active_node_indices(&document) {
            let node = &document.tree[index];
            match &node.body {
                MarkdownNode::FrontMatter(value) => {
                    let value = serde_json::to_value(value.as_ref())?;
                    let frontmatter_map = value
                        .as_object()
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("frontmatter must be a mapping"))?;
                    frontmatter = Some((
                        frontmatter_map,
                        location_range(source, node.start, node.end),
                    ));
                }
                MarkdownNode::Heading(heading) if heading.level() == &HeadingLevel::H1 => {
                    if h1.is_none() {
                        h1 = Some(direct_text(&document, index).trim().to_string());
                    }
                }
                MarkdownNode::Heading(heading) if heading.level() == &HeadingLevel::H2 => {
                    headings.push((
                        direct_text(&document, index).trim().to_string(),
                        location_to_byte(source, node.start),
                        line_start_after(source, node.end.line),
                    ));
                }
                _ => {}
            }
        }

        let h1 = h1
            .filter(|title| !title.is_empty())
            .ok_or_else(|| anyhow::anyhow!("document must contain an H1"))?;
        let (frontmatter, _frontmatter_range) = match frontmatter {
            Some(frontmatter) => frontmatter,
            None if schema.name.is_empty() => (serde_json::Map::new(), 0..0),
            None => anyhow::bail!("document must contain frontmatter"),
        };
        if !schema.name.is_empty() {
            let actual = frontmatter
                .get("schema")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("frontmatter schema is missing"))?;
            if actual != schema.name {
                anyhow::bail!("unsupported schema: {actual}");
            }
        }

        let sections = headings
            .iter()
            .enumerate()
            .map(|(index, (title, _, body_start))| Section {
                title: title.clone(),
                body_start: *body_start,
                body_end: headings
                    .get(index + 1)
                    .map(|(_, heading_start, _)| *heading_start)
                    .unwrap_or(source.len()),
            })
            .collect::<Vec<_>>();
        for title in schema.required_sections {
            let count = sections
                .iter()
                .filter(|section| section.title == *title)
                .count();
            match count {
                0 => anyhow::bail!("missing required section: {title}"),
                1 => {}
                _ => anyhow::bail!("required section must occur exactly once: {title}"),
            }
        }
        if !schema.required_sections.is_empty() {
            let actual = sections
                .iter()
                .map(|section| section.title.as_str())
                .collect::<Vec<_>>();
            if actual != schema.required_sections {
                anyhow::bail!(
                    "document sections must be exactly: {}",
                    schema.required_sections.join(", ")
                );
            }
        }

        Ok(Self {
            h1,
            frontmatter,
            frontmatter_range: _frontmatter_range,
            sections,
        })
    }

    pub(crate) fn h1(&self) -> &str {
        &self.h1
    }

    pub(crate) fn frontmatter_string(&self, field: &str) -> Option<&str> {
        self.frontmatter.get(field)?.as_str()
    }

    pub(crate) fn section(&self, title: &str) -> anyhow::Result<&Section> {
        self.sections
            .iter()
            .find(|section| section.title == title)
            .ok_or_else(|| anyhow::anyhow!("missing required section: {title}"))
    }

    pub(crate) fn section_body_range(&self, title: &str) -> anyhow::Result<Range<usize>> {
        let section = self.section(title)?;
        Ok(section.body_start..section.body_end)
    }

    pub(crate) fn replace_section(
        &self,
        source: &str,
        title: &str,
        body: &str,
    ) -> anyhow::Result<String> {
        let section = self.section(title)?;
        let mut next = source.to_string();
        next.replace_range(
            section.body_start..section.body_end,
            &format!("\n{}\n\n", body.trim()),
        );
        Ok(next)
    }

    pub(crate) fn append_section(
        &self,
        source: &str,
        title: &str,
        body: &str,
    ) -> anyhow::Result<String> {
        let section = self.section(title)?;
        let current = section.body(source).trim();
        let combined = if current.is_empty() {
            body.trim().to_string()
        } else {
            format!("{}\n\n{}", current, body.trim())
        };
        self.replace_section(source, title, &combined)
    }

    pub(crate) fn replace_frontmatter_field(
        &self,
        source: &str,
        field: &str,
        value: &str,
    ) -> anyhow::Result<String> {
        if field.is_empty() || field.contains(['\n', ':']) {
            anyhow::bail!("invalid frontmatter field: {field}");
        }
        let range = self.frontmatter_range.clone();
        let frontmatter = source
            .get(range.clone())
            .ok_or_else(|| anyhow::anyhow!("frontmatter source range is invalid"))?;
        let mut next = source.to_string();
        if let Some((start, end, quote, comment_separator)) =
            frontmatter_field_value_range(frontmatter, field)
        {
            let mut replacement = quote
                .map(|quote| format!("{quote}{}{quote}", escape_yaml_string(value, quote)))
                .unwrap_or_else(|| value.to_string());
            replacement.push_str(comment_separator);
            next.replace_range(range.start + start..range.start + end, &replacement);
        } else {
            let closing = frontmatter
                .rfind("---")
                .ok_or_else(|| anyhow::anyhow!("frontmatter closing delimiter is missing"))?;
            next.insert_str(range.start + closing, &format!("{field}: {value}\n"));
        }
        Ok(next)
    }
}

impl Section {
    pub(crate) fn body<'a>(&self, source: &'a str) -> &'a str {
        &source[self.body_start..self.body_end]
    }

    pub(crate) fn contains_line(&self, source: &str, line: u64) -> bool {
        let offset = line_start(source, line);
        self.body_start <= offset && offset < self.body_end
    }
}

fn active_node_indices(document: &Document) -> Vec<usize> {
    fn collect(document: &Document, index: usize, output: &mut Vec<usize>) {
        if document.tree.is_free_node(&index) {
            return;
        }
        output.push(index);
        let mut child = document.tree.get_first_child(index);
        while let Some(child_index) = child {
            collect(document, child_index, output);
            child = document.tree.get_next(child_index);
        }
    }

    let mut output = Vec::new();
    let mut child = document.tree.get_first_child(0);
    while let Some(index) = child {
        collect(document, index, &mut output);
        child = document.tree.get_next(index);
    }
    output
}

fn direct_text(document: &Document, index: usize) -> String {
    fn collect(document: &Document, index: usize, output: &mut String) {
        match &document.tree[index].body {
            MarkdownNode::Text(text) => output.push_str(text),
            MarkdownNode::SoftBreak | MarkdownNode::HardBreak => output.push('\n'),
            _ => {}
        }
        let mut child = document.tree.get_first_child(index);
        while let Some(child_index) = child {
            collect(document, child_index, output);
            child = document.tree.get_next(child_index);
        }
    }

    let mut output = String::new();
    collect(document, index, &mut output);
    output
}

fn location_range(source: &str, start: Location, end: Location) -> Range<usize> {
    let start = location_to_byte(source, start);
    let end = line_start_after(source, end.line).max(start);
    start..end
}

fn location_to_byte(source: &str, location: Location) -> usize {
    let start = line_start(source, location.line);
    source[start..]
        .char_indices()
        .nth(location.column.saturating_sub(1) as usize)
        .map(|(offset, _)| start + offset)
        .unwrap_or_else(|| source.len())
}

fn line_start(source: &str, line: u64) -> usize {
    if line <= 1 {
        return 0;
    }
    let mut current = 1;
    for (index, character) in source.char_indices() {
        if character == '\n' {
            current += 1;
            if current == line {
                return index + 1;
            }
        }
    }
    source.len()
}

fn line_start_after(source: &str, line: u64) -> usize {
    line_start(source, line.saturating_add(1))
}

fn frontmatter_field_value_range<'a>(
    frontmatter: &'a str,
    field: &str,
) -> Option<(usize, usize, Option<char>, &'a str)> {
    let mut offset = 0;
    for line in frontmatter.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        let Some(colon) = yaml_mapping_colon(content) else {
            offset += line.len();
            continue;
        };
        if yaml_key(&content[..colon]) == Some(field) {
            let value = &content[colon + 1..];
            let leading_whitespace = value.len() - value.trim_start().len();
            let comment_separator =
                if leading_whitespace > 0 && value[leading_whitespace..].starts_with('#') {
                    &value[..leading_whitespace]
                } else {
                    ""
                };
            let start = offset + colon + 1 + leading_whitespace;
            let value = &value[leading_whitespace..];
            let (length, quote) = yaml_value_length(value);
            return Some((start, start + length, quote, comment_separator));
        }
        offset += line.len();
    }
    None
}

fn yaml_mapping_colon(line: &str) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if let Some(active_quote) = quote {
            if active_quote == '"' && character == '\\' && !escaped {
                escaped = true;
                continue;
            }
            if character == active_quote && !escaped {
                quote = None;
            }
            escaped = false;
        } else if matches!(character, '\'' | '"') {
            quote = Some(character);
        } else if character == ':' {
            return Some(index);
        }
    }
    None
}

fn yaml_key(key: &str) -> Option<&str> {
    let key = key.trim();
    key.strip_prefix('"')
        .and_then(|key| key.strip_suffix('"'))
        .or_else(|| {
            key.strip_prefix('\'')
                .and_then(|key| key.strip_suffix('\''))
        })
        .or((!key.is_empty()).then_some(key))
}

fn yaml_value_length(value: &str) -> (usize, Option<char>) {
    let Some(quote) = value
        .chars()
        .next()
        .filter(|quote| matches!(quote, '\'' | '"'))
    else {
        let comment = value
            .char_indices()
            .find(|(index, character)| {
                *character == '#'
                    && (*index == 0
                        || value[..*index]
                            .chars()
                            .last()
                            .is_some_and(char::is_whitespace))
            })
            .map(|(index, _)| index)
            .unwrap_or(value.len());
        return (value[..comment].trim_end().len(), None);
    };

    let mut escaped = false;
    for (index, character) in value.char_indices().skip(1) {
        if quote == '"' && character == '\\' && !escaped {
            escaped = true;
            continue;
        }
        if character == quote && !escaped {
            return (index + character.len_utf8(), Some(quote));
        }
        escaped = false;
    }
    (value.len(), Some(quote))
}

fn escape_yaml_string(value: &str, quote: char) -> String {
    match quote {
        '"' => value.replace('\\', "\\\\").replace('"', "\\\""),
        '\'' => value.replace('\'', "''"),
        _ => value.to_string(),
    }
}
