use std::{
    collections::{HashMap, HashSet},
    ops::Range,
};

use markdown::{
    Document, MarkdownNode, Parser, ParserOptions,
    ast::{heading::HeadingLevel, list::ListItem},
};

use crate::blueprint::model::{CheckItem, Todo, TodoStatus};

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

#[allow(dead_code)]
pub(crate) struct ParsedBlueprintSource {
    pub todos: Vec<Todo>,
    sections: HashMap<String, Range<usize>>,
}

#[allow(dead_code)]
pub struct SourcePatch {
    range: Range<usize>,
    replacement: String,
}

#[allow(dead_code)]
impl SourcePatch {
    pub fn apply(self, source: &str) -> String {
        format!(
            "{}{}{}",
            &source[..self.range.start],
            self.replacement,
            &source[self.range.end..]
        )
    }
}

impl ParsedBlueprintSource {
    pub(crate) fn parse(path: &str, source: &str) -> anyhow::Result<Self> {
        if !path.ends_with(".md") {
            anyhow::bail!("Blueprint path must end with .md");
        }
        let document =
            Parser::new_with_options(source, ParserOptions::default().enabled_gfm().enabled_ofm())
                .parse_checked()
                .map_err(|error| anyhow::anyhow!("invalid Markdown: {error:?}"))?;

        let mut h2_headings = active_node_indices(&document)
            .into_iter()
            .filter_map(|index| match &document.tree[index].body {
                MarkdownNode::Heading(heading) if heading.level() == &HeadingLevel::H2 => Some((
                    direct_text(&document, index).trim().to_string(),
                    document.tree[index].start.line,
                )),
                _ => None,
            })
            .collect::<Vec<_>>();
        h2_headings.sort_by_key(|(_, line)| *line);
        let headings = h2_headings
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<HashSet<_>>();
        for section in REQUIRED_SECTIONS {
            if !headings.contains(section) {
                anyhow::bail!("missing required section: {section}");
            }
            if h2_headings
                .iter()
                .filter(|(name, _)| name == section)
                .count()
                != 1
            {
                anyhow::bail!("required section must occur exactly once: {section}");
            }
        }
        let sections = h2_headings
            .iter()
            .enumerate()
            .map(|(index, (name, line))| {
                let start = byte_offset_for_line(source, line + 1);
                let end = h2_headings
                    .get(index + 1)
                    .map(|(_, next_line)| byte_offset_for_line(source, *next_line))
                    .unwrap_or(source.len());
                (name.clone(), start..end)
            })
            .collect::<HashMap<_, _>>();

        let tasks = active_node_indices(&document)
            .into_iter()
            .filter_map(|index| task_node(&document, index))
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
        Ok(Self { todos, sections })
    }

    #[allow(dead_code)]
    pub fn replace_results(&self, source: &str, replacement: &str) -> anyhow::Result<SourcePatch> {
        let range = self
            .sections
            .get("Results")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("missing required section: Results"))?;
        let replacement = if replacement.is_empty() || replacement.ends_with('\n') {
            replacement.to_string()
        } else {
            format!("{replacement}\n")
        };
        let replacement = if replacement.is_empty() {
            replacement
        } else {
            format!("\n{replacement}")
        };
        if range.end > source.len() {
            anyhow::bail!("Results section range is outside source");
        }
        Ok(SourcePatch { range, replacement })
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

fn byte_offset_for_line(source: &str, line: u64) -> usize {
    if line <= 1 {
        return 0;
    }
    let mut current_line = 1;
    for (index, byte) in source.bytes().enumerate() {
        if current_line == line {
            return index;
        }
        if byte == b'\n' {
            current_line += 1;
        }
    }
    source.len()
}
