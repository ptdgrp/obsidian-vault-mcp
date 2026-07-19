#![allow(dead_code)]

use crate::blueprint::{
    ExternalBody,
    document::{DocumentSchema, ParsedDocument},
    model::{CheckItem, EvidenceItem, RevisionEntry, TodoDetail},
    source::markdown_entries,
};

const TODO_REQUIRED_SECTIONS: [&str; 7] = [
    "Plan",
    "Completion Criteria",
    "Handoff",
    "Result",
    "Evidence",
    "Notes",
    "Revision History",
];

const TODO_SCHEMA: DocumentSchema = DocumentSchema {
    name: "blueprint/todo/v3",
    required_sections: &TODO_REQUIRED_SECTIONS,
};

impl TodoDetail {
    /// Parses one standalone `blueprint/todo/v3` detail document.
    pub fn parse(path: &str, source: &str) -> anyhow::Result<Self> {
        parse_todo_source(path, source)
    }
}

/// Parses one standalone `blueprint/todo/v3` detail document.
pub fn parse_todo_source(path: &str, source: &str) -> anyhow::Result<TodoDetail> {
    let parsed = ParsedDocument::parse(path, source, TODO_SCHEMA)?;
    Ok(TodoDetail {
        id: required_frontmatter(&parsed, "id")?,
        blueprint_id: required_frontmatter(&parsed, "blueprint")?,
        title: parsed.h1().to_string(),
        created_by: required_frontmatter(&parsed, "created_by")?,
        owner: required_frontmatter(&parsed, "owner")?,
        completed_by: optional_frontmatter(&parsed, "completed_by"),
        block_reason: optional_frontmatter(&parsed, "block_reason"),
        cancel_reason: optional_frontmatter(&parsed, "cancel_reason"),
        plan: external_section_text(&parsed, source, "Plan", true)?,
        completion_criteria: check_items(parsed.section("Completion Criteria")?.body(source)),
        handoff: external_section_text(&parsed, source, "Handoff", false)?,
        result: external_section_prefix_text(&parsed, source, "Result", false)?,
        evidence: markdown_entries(parsed.section("Evidence")?.body(source), "evidence-")
            .into_iter()
            .map(|(id, markdown)| EvidenceItem { id, markdown })
            .collect(),
        revisions: markdown_entries(
            parsed.section("Revision History")?.body(source),
            "revision-",
        )
        .into_iter()
        .map(|(id, markdown)| RevisionEntry { id, markdown })
        .collect(),
        notes: external_section_text(&parsed, source, "Notes", false)?,
    })
}

fn optional_frontmatter(document: &ParsedDocument, field: &str) -> Option<String> {
    document
        .frontmatter_string(field)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn required_frontmatter(document: &ParsedDocument, field: &str) -> anyhow::Result<String> {
    document
        .frontmatter_string(field)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow::anyhow!("frontmatter {field} is missing"))
}

fn external_section_text(
    document: &ParsedDocument,
    source: &str,
    title: &str,
    required: bool,
) -> anyhow::Result<String> {
    let body = document.section(title)?.body(source).trim();
    let parsed = if required {
        ExternalBody::parse(title, body)?
    } else {
        ExternalBody::parse_optional(title, body)?
    };
    Ok(parsed.text())
}

fn external_section_prefix_text(
    document: &ParsedDocument,
    source: &str,
    title: &str,
    required: bool,
) -> anyhow::Result<String> {
    let body = document.section(title)?.body(source).trim();
    let (parsed, _) = ExternalBody::parse_leading(title, body, required)?;
    Ok(parsed.text())
}

fn check_items(source: &str) -> Vec<CheckItem> {
    source
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let marker = line.strip_prefix("- [")?.chars().next()?;
            let completed = matches!(marker, 'x' | 'X');
            let text = line.get(5..)?.trim().to_string();
            (!text.is_empty()).then_some(CheckItem { text, completed })
        })
        .collect()
}
