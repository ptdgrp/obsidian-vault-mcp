#![allow(dead_code)]

use crate::blueprint::{
    document::{DocumentSchema, ParsedDocument},
    model::{CheckItem, EvidenceItem, RevisionEntry, TodoDetail},
    source::markdown_entries,
};

const TODO_REQUIRED_SECTIONS: [&str; 8] = [
    "Intent",
    "Completion Criteria",
    "Plan",
    "Handoff",
    "Results",
    "Evidence",
    "Revision History",
    "Notes",
];

const TODO_SCHEMA: DocumentSchema = DocumentSchema {
    name: "blueprint/todo/v2",
    required_sections: &TODO_REQUIRED_SECTIONS,
};

impl TodoDetail {
    pub fn parse(path: &str, source: &str) -> anyhow::Result<Self> {
        parse_todo_source(path, source)
    }
}

pub fn parse_todo_source(path: &str, source: &str) -> anyhow::Result<TodoDetail> {
    let parsed = ParsedDocument::parse(path, source, TODO_SCHEMA)?;
    Ok(TodoDetail {
        id: required_frontmatter(&parsed, "id")?,
        blueprint_id: required_frontmatter(&parsed, "blueprint")?,
        title: parsed.h1().to_string(),
        intent: section_text(&parsed, source, "Intent")?,
        completion_criteria: check_items(parsed.section("Completion Criteria")?.body(source)),
        plan: section_text(&parsed, source, "Plan")?,
        handoff: section_text(&parsed, source, "Handoff")?,
        results: section_text(&parsed, source, "Results")?,
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
        notes: section_text(&parsed, source, "Notes")?,
    })
}

fn required_frontmatter(document: &ParsedDocument, field: &str) -> anyhow::Result<String> {
    document
        .frontmatter_string(field)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow::anyhow!("frontmatter {field} is missing"))
}

fn section_text(document: &ParsedDocument, source: &str, title: &str) -> anyhow::Result<String> {
    Ok(document.section(title)?.body(source).trim().to_string())
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
