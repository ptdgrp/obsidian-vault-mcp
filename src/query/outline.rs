use crate::parser::ParsedNote;

use super::{NoteOutlineResult, OutlineNode, VaultQueries};

impl VaultQueries {
    pub fn get_note_outline(&self, note: &str) -> anyhow::Result<NoteOutlineResult> {
        let parsed = self.parse_note(note)?;
        let outline = build_outline(&parsed);
        Ok(NoteOutlineResult {
            note: parsed.path,
            outline,
        })
    }
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
