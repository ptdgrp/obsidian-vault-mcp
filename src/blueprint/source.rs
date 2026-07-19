#![allow(dead_code)]

use std::{
    collections::{HashMap, HashSet},
    ops::Range,
};

use markdown::{
    Document, MarkdownNode, Parser, ParserOptions, ast::list::ListItem, link::Link,
    parser::Location,
};

use crate::blueprint::{
    ExternalBody,
    document::{DocumentSchema, ParsedDocument, Section},
    model::{
        BlueprintState, CheckItem, DefinitionOfDoneItem, EvidenceItem, RevisionEntry, Todo,
        TodoGraphNode, TodoStatus,
    },
};

const REQUIRED_SECTIONS: [&str; 9] = [
    "Intent",
    "Constraints",
    "Definition of Done",
    "Plan",
    "Todo Graph",
    "Results",
    "Rubric",
    "Notes",
    "Revision History",
];

const BLUEPRINT_SOURCE_SCHEMA: DocumentSchema = DocumentSchema {
    name: "blueprint/v3",
    required_sections: &REQUIRED_SECTIONS,
};

const V3_BLUEPRINT_REQUIRED_SECTIONS: [&str; 9] = [
    "Intent",
    "Constraints",
    "Definition of Done",
    "Plan",
    "Todo Graph",
    "Results",
    "Rubric",
    "Notes",
    "Revision History",
];

const V3_BLUEPRINT_SCHEMA: DocumentSchema = DocumentSchema {
    name: "blueprint/v3",
    required_sections: &V3_BLUEPRINT_REQUIRED_SECTIONS,
};

#[derive(Clone, Debug, PartialEq, Eq)]
/// Typed, aggregate view of a `blueprint/v3` document.
pub struct BlueprintSource {
    pub id: String,
    pub state: BlueprintState,
    pub title: String,
    pub created_by: String,
    pub intent: String,
    pub constraints: String,
    pub definition_of_done: Vec<DefinitionOfDoneItem>,
    pub plan: String,
    pub rubric: String,
    pub todos: Vec<TodoGraphNode>,
    pub results: String,
    pub revisions: Vec<RevisionEntry>,
    pub notes: String,
}

impl BlueprintSource {
    /// Parses one `blueprint/v3` aggregate document.
    pub fn parse(path: &str, source: &str) -> anyhow::Result<Self> {
        parse_blueprint_source(path, source)
    }
}

/// Parses one `blueprint/v3` aggregate document.
pub fn parse_blueprint_source(path: &str, source: &str) -> anyhow::Result<BlueprintSource> {
    let parsed = ParsedDocument::parse(path, source, V3_BLUEPRINT_SCHEMA)?;
    let id = required_frontmatter(&parsed, "id")?;
    let state = BlueprintState::try_from(required_frontmatter(&parsed, "state")?.as_str())?;
    let todos = parse_graph_nodes(source, parsed.section("Todo Graph")?)?;
    Ok(BlueprintSource {
        id,
        state,
        title: parsed.h1().to_string(),
        created_by: required_frontmatter(&parsed, "created_by")?,
        intent: external_section_text(&parsed, source, "Intent", true)?,
        constraints: external_section_text(&parsed, source, "Constraints", false)?,
        definition_of_done: parse_definition_of_done(
            parsed.section("Definition of Done")?.body(source),
        )?,
        plan: external_section_text(&parsed, source, "Plan", true)?,
        rubric: external_section_text(&parsed, source, "Rubric", false)?,
        todos,
        results: external_section_prefix_text(&parsed, source, "Results", false)?,
        revisions: revision_entries(parsed.section("Revision History")?.body(source)),
        notes: external_section_text(&parsed, source, "Notes", false)?,
    })
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

        let todos_section = parsed_document.section("Todo Graph")?;
        parse_graph_nodes(source, todos_section)?;
        let tasks = active_node_indices(&document)
            .into_iter()
            .filter_map(|index| task_node(&document, index))
            .filter(|task| todos_section.contains_line(source, task.line))
            .filter(|task| is_protocol_task(&document, task.index, "Todo Graph"))
            .collect::<Vec<_>>();
        let mut todos = tasks
            .iter()
            .filter(|task| task.id.as_deref().is_some_and(|id| id.starts_with("todo-")))
            .map(|task| Todo {
                id: task.id.clone().expect("filtered to Todo ID"),
                title: task.title.clone(),
                status: task.status,
                created_by: None,
                owner: None,
                completed_by: None,
                depends_on: field_value(source, task.line, "Depends On")
                    .map(|value| split_csv(&value))
                    .unwrap_or_default(),
                completion_criteria: Vec::new(),
                handoff: Vec::new(),
                result_summary: None,
                references: Vec::new(),
                block_reason: None,
                cancel_reason: None,
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

fn parse_definition_of_done(source: &str) -> anyhow::Result<Vec<DefinitionOfDoneItem>> {
    source
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let marker = line.strip_prefix("- [")?.chars().next()?;
            Some((matches!(marker, 'x' | 'X'), line.get(5..)?.trim()))
        })
        .map(|(completed, value)| {
            let (text, id) = value
                .rsplit_once(" ^")
                .filter(|(_, id)| id.starts_with("dod-") && !id[4..].is_empty())
                .ok_or_else(|| anyhow::anyhow!("Definition of Done item is missing a dod-* ID"))?;
            if text.trim().is_empty() {
                anyhow::bail!("Definition of Done item text must not be empty");
            }
            Ok(DefinitionOfDoneItem {
                id: id.to_string(),
                text: text.trim().to_string(),
                completed,
            })
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
        .filter(|task| is_protocol_task(&document, task.index, "Todo Graph"))
        .filter(|task| task.id.as_deref().is_some_and(|id| id.starts_with("todo-")))
        .collect::<Vec<_>>();
    let mut ids = HashSet::new();
    for task in &tasks {
        let id = task.id.as_deref().expect("filtered to Todo ID");
        if !ids.insert(id) {
            anyhow::bail!("duplicate Todo ID in central graph: {id}");
        }
        let locator = locate_task(source, "Todo Graph", id)?;
        if let Some(field) = locator
            .fields
            .iter()
            .find(|field| !matches!(field.name.as_str(), "Depends On" | "Children"))
        {
            anyhow::bail!(
                "Todo Graph field is not graph structure: {id} {}",
                field.name
            );
        }
    }

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
                created_by: None,
                owner: None,
                completed_by: None,
                depends_on: field_value(source, task.line, "Depends On")
                    .map(|value| split_csv(&value))
                    .unwrap_or_default(),
                block_reason: None,
                cancel_reason: None,
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

/// A mutation-safe view of one real OFM task node.
///
/// All ranges are sourced from the parsed Markdown tree.  In particular, the
/// block id belongs to the task's inline node, rather than to text which merely
/// resembles a task inside a code fence or an ordinary list.
#[derive(Clone, Debug)]
pub(crate) struct TaskLocator {
    pub(crate) task_line: Range<usize>,
    pub(crate) task: Range<usize>,
    pub(crate) marker: Range<usize>,
    pub(crate) title: Range<usize>,
    pub(crate) indent: usize,
    pub(crate) status: TodoStatus,
    pub(crate) title_text: String,
    pub(crate) fields: Vec<TaskFieldLocator>,
    pub(crate) children: Option<TaskChildrenLocator>,
}

#[derive(Clone, Debug)]
pub(crate) struct TaskFieldLocator {
    pub(crate) name: String,
    pub(crate) value: String,
    pub(crate) value_range: Range<usize>,
    pub(crate) line: Range<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct TaskChildrenLocator {
    pub(crate) label: Range<usize>,
    pub(crate) contents: Range<usize>,
    pub(crate) child_indent: usize,
}

/// Locates exactly one OFM task with `id` in `section`.
///
/// This deliberately walks `ListItem::Task` nodes and their direct child list
/// items.  It does not inspect Markdown lines, so fenced text and ordinary
/// lists cannot be mistaken for Blueprint tasks or fields.
pub(crate) fn locate_task(source: &str, section: &str, id: &str) -> anyhow::Result<TaskLocator> {
    let parsed = ParsedDocument::parse(
        "document.md",
        source,
        DocumentSchema {
            name: "",
            required_sections: &[],
        },
    )?;
    let section_range = parsed.section_body_range(section)?;
    let document =
        Parser::new_with_options(source, ParserOptions::default().enabled_gfm().enabled_ofm())
            .parse_checked()
            .map_err(|error| anyhow::anyhow!("invalid Markdown: {error:?}"))?;

    let matches = active_node_indices(&document)
        .into_iter()
        .filter(|index| is_task_node(&document, *index))
        .filter(|index| list_item_block_id(&document, *index).as_deref() == Some(id))
        .filter(|index| {
            let start = location_to_byte(source, document.tree[*index].start);
            section_range.contains(&start)
        })
        .filter(|index| is_protocol_task(&document, *index, section))
        .collect::<Vec<_>>();
    let [index] = matches.as_slice() else {
        if matches.is_empty() {
            anyhow::bail!("unknown {section} task: {id}");
        }
        anyhow::bail!("duplicate {section} task ID: {id}");
    };
    task_locator(source, &document, *index)
}

fn is_task_node(document: &Document, index: usize) -> bool {
    matches!(
        &document.tree[index].body,
        MarkdownNode::ListItem(item) if matches!(item.as_ref(), ListItem::Task(_))
    )
}

fn is_protocol_task(document: &Document, index: usize, section: &str) -> bool {
    let parent_list = document.tree.get_parent(index);
    let is_top_level = document.tree.get_parent(parent_list) == 0;
    match section {
        "Todo Graph" => is_top_level || has_ancestor_label(document, index, "Children:"),
        "Definition of Done" => is_top_level,
        _ => true,
    }
}

fn task_locator(source: &str, document: &Document, index: usize) -> anyhow::Result<TaskLocator> {
    let node = &document.tree[index];
    let MarkdownNode::ListItem(item) = &node.body else {
        unreachable!("task_locator is called only for task nodes")
    };
    let ListItem::Task(task) = item.as_ref() else {
        unreachable!("task_locator is called only for task nodes")
    };
    let status = TodoStatus::from_marker(
        task.task
            .ok_or_else(|| anyhow::anyhow!("task has no marker"))?,
    )
    .ok_or_else(|| anyhow::anyhow!("task has no supported marker"))?;
    let task_start = location_to_byte(source, node.start);
    let task_line_end = line_start_after(source, node.start.line);
    let task_end = subtree_end(source, document, index).max(task_line_end);
    let marker = task_start + 2..task_start + 5;
    if source.get(marker.clone()) != Some(&format!("[{}]", status.marker())) {
        anyhow::bail!("task marker location is invalid")
    }
    let title = task_title_range(source, document, index)?;
    let mut fields = Vec::new();
    let mut children = None;
    let mut child = document.tree.get_first_child(index);
    while let Some(child_index) = child {
        if matches!(document.tree[child_index].body, MarkdownNode::List(_)) {
            let mut field = document.tree.get_first_child(child_index);
            while let Some(field_index) = field {
                if let Some((name, value, value_range)) =
                    direct_field(source, document, field_index)
                {
                    let field_node = &document.tree[field_index];
                    let line = location_to_byte(source, field_node.start)
                        ..line_start_after(source, field_node.start.line);
                    if name == "Children" {
                        let mut nested = document.tree.get_first_child(field_index);
                        while let Some(nested_index) = nested {
                            if matches!(document.tree[nested_index].body, MarkdownNode::List(_)) {
                                let nested_node = &document.tree[nested_index];
                                let contents = location_to_byte(source, nested_node.start)
                                    ..subtree_end(source, document, nested_index);
                                let child_indent = indentation_at(source, contents.start);
                                children = Some(TaskChildrenLocator {
                                    label: line.clone(),
                                    contents,
                                    child_indent,
                                });
                                break;
                            }
                            nested = document.tree.get_next(nested_index);
                        }
                    }
                    fields.push(TaskFieldLocator {
                        name,
                        value,
                        value_range,
                        line,
                    });
                }
                field = document.tree.get_next(field_index);
            }
        }
        child = document.tree.get_next(child_index);
    }
    Ok(TaskLocator {
        task_line: task_start..task_line_end,
        task: task_start..task_end,
        marker,
        title,
        indent: indentation_at(source, task_start),
        status,
        title_text: direct_text(document, index).trim().to_string(),
        fields,
        children,
    })
}

fn task_title_range(
    source: &str,
    document: &Document,
    index: usize,
) -> anyhow::Result<Range<usize>> {
    fn text(source: &str, document: &Document, index: usize) -> Option<Range<usize>> {
        if matches!(document.tree[index].body, MarkdownNode::List(_)) {
            return None;
        }
        if matches!(document.tree[index].body, MarkdownNode::Text(_)) {
            let node = &document.tree[index];
            return Some(location_to_byte(source, node.start)..location_to_byte(source, node.end));
        }
        let mut child = document.tree.get_first_child(index);
        while let Some(child_index) = child {
            if let Some(range) = text(source, document, child_index) {
                return Some(range);
            }
            child = document.tree.get_next(child_index);
        }
        None
    }

    fn find(source: &str, document: &Document, index: usize) -> Option<Range<usize>> {
        if matches!(document.tree[index].body, MarkdownNode::List(_)) {
            return None;
        }
        if matches!(document.tree[index].body, MarkdownNode::Text(_)) {
            let node = &document.tree[index];
            return Some(location_to_byte(source, node.start)..location_to_byte(source, node.end));
        }
        if matches!(document.tree[index].body, MarkdownNode::Link(_)) {
            return text(source, document, index);
        }
        let mut child = document.tree.get_first_child(index);
        while let Some(child_index) = child {
            if let Some(range) = find(source, document, child_index) {
                return Some(range);
            }
            child = document.tree.get_next(child_index);
        }
        None
    }

    find(source, document, index).ok_or_else(|| anyhow::anyhow!("task has no Markdown link title"))
}

fn direct_field(
    source: &str,
    document: &Document,
    index: usize,
) -> Option<(String, String, Range<usize>)> {
    let MarkdownNode::ListItem(item) = &document.tree[index].body else {
        return None;
    };
    if !matches!(item.as_ref(), ListItem::Bullet(_)) {
        return None;
    }
    let text = direct_text(document, index);
    let text = text.trim();
    let (name, value) = text.split_once(':')?;
    let name = name.trim();
    let value = value.trim();
    let text_start = first_text_start(source, document, index)?;
    let colon = text.find(':')?;
    let raw_value = &text[colon + 1..];
    let leading = raw_value.len() - raw_value.trim_start().len();
    let value_start = text_start + colon + 1 + leading;
    (!name.is_empty()).then(|| {
        (
            name.to_string(),
            value.to_string(),
            value_start..value_start + value.len(),
        )
    })
}

fn first_text_start(source: &str, document: &Document, index: usize) -> Option<usize> {
    if matches!(document.tree[index].body, MarkdownNode::Text(_)) {
        return Some(location_to_byte(source, document.tree[index].start));
    }
    let mut child = document.tree.get_first_child(index);
    while let Some(child_index) = child {
        if !matches!(document.tree[child_index].body, MarkdownNode::List(_))
            && let Some(start) = first_text_start(source, document, child_index)
        {
            return Some(start);
        }
        child = document.tree.get_next(child_index);
    }
    None
}

fn indentation_at(source: &str, offset: usize) -> usize {
    let line_start = source[..offset].rfind('\n').map_or(0, |index| index + 1);
    source[line_start..offset].chars().count()
}

fn subtree_end(source: &str, document: &Document, index: usize) -> usize {
    let mut end = line_start_after(source, document.tree[index].start.line);
    let mut child = document.tree.get_first_child(index);
    while let Some(child_index) = child {
        end = end.max(subtree_end(source, document, child_index));
        child = document.tree.get_next(child_index);
    }
    end
}

fn line_start_after(source: &str, line: u64) -> usize {
    let target = line.saturating_add(1);
    if target <= 1 {
        return 0;
    }
    let mut current = 1;
    for (index, character) in source.char_indices() {
        if character == '\n' {
            current += 1;
            if current == target {
                return index + 1;
            }
        }
    }
    source.len()
}

/// Validates Evidence identity and standard Markdown references within one Blueprint aggregate.
/// External links without an Evidence block fragment deliberately remain outside this protocol.
pub(crate) fn validate_evidence_aggregate(
    blueprint_source: &str,
    todo_sources: &[(&str, &str)],
) -> anyhow::Result<()> {
    let mut documents = std::collections::HashMap::new();
    documents.insert("blueprint.md", blueprint_source);
    for (path, source) in todo_sources {
        documents.insert(*path, *source);
    }

    let mut evidence_by_document = std::collections::HashMap::new();
    let mut ids = std::collections::HashSet::new();
    BlueprintSource::parse("blueprint.md", blueprint_source)?;
    evidence_by_document.insert("blueprint.md", Vec::new());
    for (path, source) in todo_sources {
        crate::blueprint::TodoDetail::parse(path, source)?;
        let document_ids = evidence_ids_in_section(source)?;
        for id in &document_ids {
            if !ids.insert(id.clone()) {
                anyhow::bail!("duplicate Evidence ID: {id}");
            }
        }
        evidence_by_document.insert(*path, document_ids);
    }

    for (path, source) in documents {
        for (target, id) in standard_evidence_links(source)? {
            let resolved = resolve_evidence_target(path, &target)?;
            let Some(defined) = evidence_by_document.get(resolved.as_str()) else {
                anyhow::bail!("invalid Evidence target: {target}");
            };
            if !defined.contains(&id) {
                anyhow::bail!("dangling Evidence reference: {id}");
            }
        }
    }
    Ok(())
}

fn evidence_ids_in_section(source: &str) -> anyhow::Result<Vec<String>> {
    let parsed = ParsedDocument::parse(
        "document.md",
        source,
        DocumentSchema {
            name: "",
            required_sections: &[],
        },
    )?;
    Ok(
        markdown_entries(parsed.section("Evidence")?.body(source), "evidence-")
            .into_iter()
            .map(|(id, _)| id)
            .collect(),
    )
}

pub(crate) fn standard_evidence_links(source: &str) -> anyhow::Result<Vec<(String, String)>> {
    let document =
        Parser::new_with_options(source, ParserOptions::default().enabled_gfm().enabled_ofm())
            .parse_checked()
            .map_err(|error| anyhow::anyhow!("malformed Evidence reference: {error:?}"))?;
    let mut links = Vec::new();
    for index in active_node_indices(&document) {
        let MarkdownNode::Link(link) = &document.tree[index].body else {
            continue;
        };
        let Link::Default(link) = link.as_ref() else {
            continue;
        };
        // The parser percent-encodes `^` in URL fragments, while Blueprint block IDs use it.
        let destination = link.url.replace("%5E", "^").replace("%5e", "^");
        if let Some((target, id)) = destination.rsplit_once("#^") {
            if !id.starts_with("evidence-") || id.contains(char::is_whitespace) {
                anyhow::bail!("malformed Evidence reference");
            }
            links.push((target.to_string(), id.to_string()));
        }
    }
    Ok(links)
}

fn resolve_evidence_target(from: &str, target: &str) -> anyhow::Result<String> {
    if target.is_empty() {
        return Ok(from.to_string());
    }
    let resolved = match (from, target) {
        ("blueprint.md", target) if target.starts_with("todos/") => target.to_string(),
        ("blueprint.md", "blueprint.md") => "blueprint.md".to_string(),
        (from, target) if from.starts_with("todos/") && target.starts_with("todo-") => {
            format!("todos/{target}")
        }
        (from, target) if from.starts_with("todos/") && target.starts_with("./todo-") => {
            format!("todos/{}", &target[2..])
        }
        (from, "../blueprint.md") if from.starts_with("todos/") => "blueprint.md".to_string(),
        (from, target) if from.starts_with("todos/") && target == from => target.to_string(),
        _ => anyhow::bail!("invalid Evidence target: {target}"),
    };
    if !resolved.starts_with("todos/todo-") && resolved != "blueprint.md" {
        anyhow::bail!("invalid Evidence target: {target}");
    }
    Ok(resolved)
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
