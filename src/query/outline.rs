use crate::parser::ParsedNote;

use super::{
    NoteOutlineResult, OutlineNode, VaultQueries,
    section::{ParsedHeadingSelector, find_selectable_heading, heading_not_found_message},
};

impl VaultQueries {
    pub fn get_note_outline(
        &self,
        note: &str,
        heading: Option<&str>,
    ) -> anyhow::Result<NoteOutlineResult> {
        let parsed = self.parse_note(note)?;
        let outline = build_outline(&parsed);
        let outline = match heading {
            Some(heading) => {
                let selector = ParsedHeadingSelector::parse(heading);
                let target = find_selectable_heading(&parsed, &selector).ok_or_else(|| {
                    anyhow::anyhow!("{}", heading_not_found_message(heading, &parsed))
                })?;
                outline_chain(&outline, target.source.line_start)
                    .expect("selected heading must be present in its outline")
            }
            None => outline,
        };
        Ok(NoteOutlineResult {
            note: parsed.path,
            outline,
        })
    }
}

fn outline_chain(nodes: &[OutlineNode], target_line: u64) -> Option<Vec<OutlineNode>> {
    for node in nodes {
        if node.source.line_start == target_line {
            let mut target = node.clone();
            target.children.clear();
            return Some(vec![target]);
        }
        if let Some(children) = outline_chain(&node.children, target_line) {
            let mut ancestor = node.clone();
            ancestor.children = children;
            return Some(vec![ancestor]);
        }
    }
    None
}

fn build_outline(parsed: &ParsedNote) -> Vec<OutlineNode> {
    let mut roots: Vec<OutlineNode> = Vec::new();
    let mut stack: Vec<(u8, Vec<usize>)> = Vec::new();

    for heading in &parsed.headings {
        if heading.level == 1 {
            stack.clear();
            continue;
        }

        while stack
            .last()
            .is_some_and(|(level, _)| *level >= heading.level)
        {
            stack.pop();
        }
        let node = OutlineNode {
            heading: heading.text.clone(),
            level: heading.level,
            heading_path: heading.path.clone(),
            source: heading.source.clone().into(),
            children: Vec::new(),
        };
        let child_index = if let Some((_, parent_path)) = stack.last() {
            let children = children_at_mut(&mut roots, parent_path);
            children.push(node);
            children.len() - 1
        } else {
            roots.push(node);
            roots.len() - 1
        };
        let mut path = stack
            .last()
            .map(|(_, path)| path.clone())
            .unwrap_or_default();
        path.push(child_index);
        stack.push((heading.level, path));
    }

    roots
}

fn children_at_mut<'a>(
    roots: &'a mut Vec<OutlineNode>,
    path: &[usize],
) -> &'a mut Vec<OutlineNode> {
    let mut current = roots;
    for &index in path {
        current = &mut current[index].children;
    }
    current
}
