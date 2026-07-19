use super::super::TodoDetail;

fn evidence() -> &'static str {
    "### 测试已通过 ^evidence-review\n\n- Command: `cargo test`\n"
}

fn todo_v3(id: &str, blueprint: &str, criteria: &str, results: &str, evidence: &str) -> String {
    format!(
        "---\nschema: blueprint/todo/v3\nid: {id}\nblueprint: {blueprint}\ncreated_by: planner\nowner: planner\n---\n\n# 审阅章节\n\n## Plan\n\n~~~\n阅读相邻章节\n~~~\n\n## Completion Criteria\n\n{criteria}\n\n## Handoff\n\n~~~\n无需交接\n~~~\n\n## Result\n\n~~~\n{results}\n~~~\n\n## Evidence\n\n{evidence}\n## Notes\n\n~~~\n<!-- preserve -->\n~~~\n\n## Revision History\n\n### 初始范围 ^revision-1\n\n- Changed By: planner\n\n"
    )
}

#[test]
fn parses_todo_sections_without_duplicating_graph_state() {
    let source = todo_v3(
        "todo-1",
        "bp-01",
        "- [x] 已检查前后段落",
        "评估完成",
        evidence(),
    );
    let detail = TodoDetail::parse("todos/todo-1.md", &source).unwrap();

    assert_eq!(detail.id, "todo-1");
    assert_eq!(detail.blueprint_id, "bp-01");
    assert!(detail.completion_criteria.iter().all(|item| item.completed));
    assert_eq!(detail.result, "评估完成");
    assert_eq!(detail.evidence.len(), 1);
    assert!(detail.evidence[0].markdown.contains("`cargo test`"));
    assert_eq!(detail.revisions[0].id, "revision-1");
    assert!(detail.notes.contains("<!-- preserve -->"));
}

#[test]
fn rejects_todo_without_required_v3_frontmatter() {
    let source = todo_v3("todo-1", "bp-01", "- [ ] 检查", "", evidence());
    let invalid = source.replacen("schema: blueprint/todo/v3", "schema: blueprint/v3", 1);
    let error = TodoDetail::parse("todos/todo-1.md", &invalid).unwrap_err();
    assert!(
        error.to_string().contains("unsupported schema"),
        "{error:#}"
    );
}

#[test]
fn preserves_crlf_and_boundary_blank_lines_in_evidence_and_revisions() {
    let source = todo_v3("todo-1", "bp-01", "- [ ] 检查", "", evidence()).replace('\n', "\r\n");
    let detail = TodoDetail::parse("todos/todo-1.md", &source).unwrap();

    assert_eq!(
        detail.evidence[0].markdown,
        "### 测试已通过 ^evidence-review\r\n\r\n- Command: `cargo test`\r\n\r\n"
    );
    assert_eq!(
        detail.revisions[0].markdown,
        "### 初始范围 ^revision-1\r\n\r\n- Changed By: planner\r\n\r\n"
    );
}
