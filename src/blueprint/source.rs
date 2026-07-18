#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

use markdown::{
    Document, MarkdownNode, Parser, ParserOptions, ast::list::ListItem, parser::Location,
};

use crate::blueprint::{
    document::{DocumentSchema, ParsedDocument, Section},
    model::{
        BlueprintState, CheckItem, EvidenceItem, RevisionEntry, Todo, TodoGraphNode, TodoStatus,
    },
};

const REQUIRED_SECTIONS: [&str; 8] = [
    "Record",
    "Intent",
    "Constraints",
    "Definition of Done",
    "Plan",
    "Todos",
    "Results",
    "Notes",
];

const BLUEPRINT_SOURCE_SCHEMA: DocumentSchema = DocumentSchema {
    name: "",
    required_sections: &REQUIRED_SECTIONS,
};

const V2_BLUEPRINT_REQUIRED_SECTIONS: [&str; 11] = [
    "Record",
    "Intent",
    "Constraints",
    "Definition of Done",
    "Plan",
    "Rubric",
    "Todos",
    "Results",
    "Evidence",
    "Revision History",
    "Notes",
];

const V2_BLUEPRINT_SCHEMA: DocumentSchema = DocumentSchema {
    name: "blueprint/v2",
    required_sections: &V2_BLUEPRINT_REQUIRED_SECTIONS,
};

#[derive(Clone, Debug, PartialEq, Eq)]
/// Typed, aggregate view of a `blueprint/v2` document.
pub struct BlueprintSource {
    pub id: String,
    pub state: BlueprintState,
    pub title: String,
    pub intent: String,
    pub constraints: String,
    pub definition_of_done: Vec<CheckItem>,
    pub plan: String,
    pub rubric: String,
    pub todos: Vec<TodoGraphNode>,
    pub results: String,
    pub evidence: Vec<EvidenceItem>,
    pub revisions: Vec<RevisionEntry>,
    pub notes: String,
}

impl BlueprintSource {
    /// Parses one `blueprint/v2` aggregate document.
    pub fn parse(path: &str, source: &str) -> anyhow::Result<Self> {
        parse_blueprint_source(path, source)
    }
}

/// Parses one `blueprint/v2` aggregate document.
pub fn parse_blueprint_source(path: &str, source: &str) -> anyhow::Result<BlueprintSource> {
    let parsed = ParsedDocument::parse(path, source, V2_BLUEPRINT_SCHEMA)?;
    let id = required_frontmatter(&parsed, "id")?;
    let state = BlueprintState::try_from(required_frontmatter(&parsed, "state")?.as_str())?;
    let todos = parse_graph_nodes(source, parsed.section("Todos")?)?;
    let rubric = section_text(&parsed, source, "Rubric")?;
    if rubric.is_empty() {
        anyhow::bail!("Rubric must not be empty");
    }
    Ok(BlueprintSource {
        id,
        state,
        title: parsed.h1().to_string(),
        intent: section_text(&parsed, source, "Intent")?,
        constraints: section_text(&parsed, source, "Constraints")?,
        definition_of_done: parse_check_items(parsed.section("Definition of Done")?.body(source)),
        plan: section_text(&parsed, source, "Plan")?,
        rubric,
        todos,
        results: section_text(&parsed, source, "Results")?,
        evidence: evidence_items(parsed.section("Evidence")?.body(source)),
        revisions: revision_entries(parsed.section("Revision History")?.body(source)),
        notes: section_text(&parsed, source, "Notes")?,
    })
}

pub(crate) struct ParsedBlueprintSource {
    pub todos: Vec<Todo>,
}

impl ParsedBlueprintSource {
    pub(crate) fn parse(path: &str, source: &str) -> anyhow::Result<Self> {
        let parsed_document = ParsedDocument::parse(path, source, BLUEPRINT_SOURCE_SCHEMA)?;
        let document =
            Parser::new_with_options(source, ParserOptions::default().enabled_gfm().enabled_ofm())
                .parse_checked()
                .map_err(|error| anyhow::anyhow!("invalid Markdown: {error:?}"))?;

        let todos_section = parsed_document.section("Todos")?;
        let tasks = active_node_indices(&document)
            .into_iter()
            .filter_map(|index| task_node(&document, index))
            .filter(|task| todos_section.contains_line(source, task.line))
            .collect::<Vec<_>>();
        let mut todos = tasks
            .iter()
            .filter(|task| task.id.as_deref().is_some_and(|id| id.starts_with("todo-")))
            .map(|task| Todo {
                id: task.id.clone().expect("filtered to Todo ID"),
                title: task.title.clone(),
                status: task.status,
                created_by: field_value(source, task.line, "Created By"),
                owner: field_value(source, task.line, "Owner"),
                completed_by: field_value(source, task.line, "Completed By"),
                depends_on: field_value(source, task.line, "Depends On")
                    .map(|value| split_csv(&value))
                    .unwrap_or_default(),
                completion_criteria: Vec::new(),
                handoff: field_values(source, task.line, "Handoff"),
                result_summary: field_value(source, task.line, "Result Summary"),
                references: field_values(source, task.line, "Reference"),
                block_reason: field_value(source, task.line, "Block Reason"),
                cancel_reason: field_value(source, task.line, "Cancel Reason"),
                children: Vec::new(),
            })
            .collect::<Vec<_>>();
        let todo_indexes = todos
            .iter()
            .enumerate()
            .map(|(index, todo)| (todo.id.clone(), index))
            .collect::<HashMap<String, usize>>();

        for task in &tasks {
            let Some(parent_id) = nearest_todo_ancestor(&document, task.index) else {
                continue;
            };
            let Some(&parent_index) = todo_indexes.get(&parent_id) else {
                continue;
            };
            if has_ancestor_label(&document, task.index, "Completion Criteria:") {
                todos[parent_index].completion_criteria.push(CheckItem {
                    text: task.title.clone(),
                    completed: task.status == TodoStatus::Completed,
                });
            }
        }

        let child_pairs = tasks
            .iter()
            .filter(|task| task.id.as_deref().is_some_and(|id| id.starts_with("todo-")))
            .filter_map(|task| {
                has_ancestor_label(&document, task.index, "Children:")
                    .then(|| {
                        nearest_todo_ancestor(&document, task.index)
                            .map(|parent| (parent, task.id.clone().expect("filtered to Todo ID")))
                    })
                    .flatten()
            })
            .collect::<Vec<_>>();
        for (parent_id, child_id) in child_pairs {
            let Some(&parent_index) = todo_indexes.get(&parent_id) else {
                continue;
            };
            let Some(&child_index) = todo_indexes.get(&child_id) else {
                continue;
            };
            if parent_index != child_index {
                let child = todos[child_index].clone();
                todos[parent_index].children.push(child);
            }
        }
        let child_ids = todos
            .iter()
            .flat_map(|todo| todo.children.iter().map(|child| child.id.clone()))
            .collect::<HashSet<_>>();
        todos.retain(|todo| !child_ids.contains(&todo.id));
        Ok(Self { todos })
    }
}

struct TaskNode {
    index: usize,
    id: Option<String>,
    title: String,
    status: TodoStatus,
    line: u64,
}

fn task_node(document: &Document, index: usize) -> Option<TaskNode> {
    let node = &document.tree[index];
    let MarkdownNode::ListItem(item) = &node.body else {
        return None;
    };
    let ListItem::Task(item) = item.as_ref() else {
        return None;
    };
    let status = TodoStatus::from_marker(item.task?)?;
    Some(TaskNode {
        index,
        id: list_item_block_id(document, index),
        title: direct_text(document, index).trim().to_string(),
        status,
        line: node.start.line,
    })
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
            if !matches!(document.tree[child_index].body, MarkdownNode::List(_)) {
                collect(document, child_index, output);
            }
            child = document.tree.get_next(child_index);
        }
    }

    let mut output = String::new();
    collect(document, index, &mut output);
    output
}

fn nearest_todo_ancestor(document: &Document, index: usize) -> Option<String> {
    let mut current = document.tree.get_parent(index);
    while current != 0 {
        if matches!(document.tree[current].body, MarkdownNode::ListItem(_))
            && let Some(id) = list_item_block_id(document, current)
            && id.starts_with("todo-")
        {
            return Some(id);
        }
        let parent = document.tree.get_parent(current);
        if parent == current {
            break;
        }
        current = parent;
    }
    None
}

fn list_item_block_id(document: &Document, index: usize) -> Option<String> {
    let mut child = document.tree.get_first_child(index);
    while let Some(child_index) = child {
        if let Some(id) = document.tree[child_index].id.as_deref() {
            return Some(id.to_string());
        }
        child = document.tree.get_next(child_index);
    }
    None
}

fn has_ancestor_label(document: &Document, index: usize, label: &str) -> bool {
    let mut current = document.tree.get_parent(index);
    while current != 0 {
        if direct_text(document, current).trim() == label {
            return true;
        }
        let parent = document.tree.get_parent(current);
        if parent == current {
            break;
        }
        current = parent;
    }
    false
}

fn field_value(source: &str, task_start_line: u64, field: &str) -> Option<String> {
    source
        .lines()
        .skip(task_start_line as usize)
        .take_while(|line| !line.trim_start().starts_with("- [") && !line.starts_with("## "))
        .find_map(|line| line.trim().strip_prefix(&format!("- {field}:")))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn field_values(source: &str, task_start_line: u64, field: &str) -> Vec<String> {
    source
        .lines()
        .skip(task_start_line as usize)
        .take_while(|line| !line.trim_start().starts_with("- [") && !line.starts_with("## "))
        .filter_map(|line| line.trim().strip_prefix(&format!("- {field}:")))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn split_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
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

fn parse_check_items(source: &str) -> Vec<CheckItem> {
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

fn parse_graph_nodes(source: &str, todos_section: &Section) -> anyhow::Result<Vec<TodoGraphNode>> {
    let document =
        Parser::new_with_options(source, ParserOptions::default().enabled_gfm().enabled_ofm())
            .parse_checked()
            .map_err(|error| anyhow::anyhow!("invalid Markdown: {error:?}"))?;
    let tasks = active_node_indices(&document)
        .into_iter()
        .filter_map(|index| task_node(&document, index))
        .filter(|task| todos_section.contains_line(source, task.line))
        .filter(|task| task.id.as_deref().is_some_and(|id| id.starts_with("todo-")))
        .collect::<Vec<_>>();

    let todos = tasks
        .iter()
        .map(|task| {
            let line = source
                .lines()
                .nth(task.line.saturating_sub(1) as usize)
                .unwrap_or_default();
            let (title, document) = standard_markdown_link(line).ok_or_else(|| {
                anyhow::anyhow!(
                    "Todo {} must use a standard Markdown link to its document",
                    task.id.as_deref().unwrap_or_default()
                )
            })?;
            Ok(TodoGraphNode {
                id: task.id.clone().expect("filtered to Todo ID"),
                title,
                document,
                status: task.status,
                created_by: field_value(source, task.line, "Created By"),
                owner: field_value(source, task.line, "Owner"),
                completed_by: field_value(source, task.line, "Completed By"),
                depends_on: field_value(source, task.line, "Depends On")
                    .map(|value| split_csv(&value))
                    .unwrap_or_default(),
                block_reason: field_value(source, task.line, "Block Reason"),
                cancel_reason: field_value(source, task.line, "Cancel Reason"),
                children: Vec::new(),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let todo_indexes = todos
        .iter()
        .enumerate()
        .map(|(index, todo)| (todo.id.clone(), index))
        .collect::<HashMap<String, usize>>();
    let child_pairs = tasks
        .iter()
        .filter(|task| has_ancestor_label(&document, task.index, "Children:"))
        .filter_map(|task| {
            nearest_todo_ancestor(&document, task.index)
                .map(|parent| (parent, task.id.clone().expect("filtered to Todo ID")))
        })
        .collect::<Vec<_>>();
    let mut children_by_parent = HashMap::<usize, Vec<usize>>::new();
    let mut child_indexes = HashSet::new();
    for (parent_id, child_id) in child_pairs {
        let Some(&parent_index) = todo_indexes.get(&parent_id) else {
            continue;
        };
        let Some(&child_index) = todo_indexes.get(&child_id) else {
            continue;
        };
        if parent_index != child_index {
            children_by_parent
                .entry(parent_index)
                .or_default()
                .push(child_index);
            child_indexes.insert(child_index);
        }
    }
    Ok((0..todos.len())
        .filter(|index| !child_indexes.contains(index))
        .map(|index| build_graph_node(index, &todos, &children_by_parent))
        .collect())
}

fn build_graph_node(
    index: usize,
    nodes: &[TodoGraphNode],
    children_by_parent: &HashMap<usize, Vec<usize>>,
) -> TodoGraphNode {
    let mut node = nodes[index].clone();
    node.children = children_by_parent
        .get(&index)
        .into_iter()
        .flatten()
        .map(|child| build_graph_node(*child, nodes, children_by_parent))
        .collect();
    node
}

fn standard_markdown_link(line: &str) -> Option<(String, String)> {
    let task = line.trim_start().strip_prefix("- [")?;
    let task = task.get(3..)?.trim_start();
    let open = task.find('[')?;
    let close = task[open..].find("](")? + open;
    let end = task[close + 2..].find(')')? + close + 2;
    let title = task[open + 1..close].trim();
    let document = task[close + 2..end].trim();
    (!title.is_empty() && !document.is_empty()).then(|| (title.to_string(), document.to_string()))
}

fn evidence_items(source: &str) -> Vec<EvidenceItem> {
    markdown_entries(source, "evidence-")
        .into_iter()
        .map(|(id, markdown)| EvidenceItem { id, markdown })
        .collect()
}

fn revision_entries(source: &str) -> Vec<RevisionEntry> {
    markdown_entries(source, "revision-")
        .into_iter()
        .map(|(id, markdown)| RevisionEntry { id, markdown })
        .collect()
}

pub(crate) fn markdown_entries(source: &str, prefix: &str) -> Vec<(String, String)> {
    let Ok(document) =
        Parser::new_with_options(source, ParserOptions::default().enabled_gfm().enabled_ofm())
            .parse_checked()
    else {
        return Vec::new();
    };
    let starts = active_node_indices(&document)
        .into_iter()
        .filter_map(|index| {
            let node = &document.tree[index];
            let id = node.id.as_deref()?.strip_prefix(prefix)?.to_string();
            Some((
                location_to_byte(source, node.start),
                format!("{prefix}{id}"),
            ))
        })
        .collect::<Vec<_>>();
    starts
        .iter()
        .enumerate()
        .map(|(index, (start, id))| {
            let end = starts
                .get(index + 1)
                .map(|(next, _)| *next)
                .unwrap_or(source.len());
            (id.clone(), source[*start..end].to_string())
        })
        .collect()
}

fn location_to_byte(source: &str, location: Location) -> usize {
    let mut line = 1;
    let mut start = 0;
    for (index, character) in source.char_indices() {
        if line == location.line {
            break;
        }
        if character == '\n' {
            line += 1;
            start = index + 1;
        }
    }
    source[start..]
        .char_indices()
        .nth(location.column.saturating_sub(1) as usize)
        .map(|(offset, _)| start + offset)
        .unwrap_or(source.len())
}
