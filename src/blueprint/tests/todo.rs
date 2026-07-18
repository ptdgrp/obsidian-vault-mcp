use super::super::TodoDetail;

fn evidence() -> &'static str {
    "### 测试已通过 ^evidence-review\n\n- Command: `cargo test`\n"
}

fn todo_v2(id: &str, blueprint: &str, criteria: &str, results: &str, evidence: &str) -> String {
    format!(
        "---\nschema: blueprint/todo/v2\nid: {id}\nblueprint: {blueprint}\n---\n\n# 审阅章节\n\n## Intent\n\n检查故事连续性\n\n## Completion Criteria\n\n{criteria}\n\n## Plan\n\n阅读相邻章节\n\n## Handoff\n\n无需交接\n\n## Results\n\n{results}\n\n## Evidence\n\n{evidence}\n## Revision History\n\n### 初始范围 ^revision-review\n\n- Changed By: planner\n\n## Notes\n\n<!-- preserve -->\n"
    )
}

#[test]
fn parses_todo_sections_without_duplicating_graph_state() {
    let source = todo_v2(
        "todo-review",
        "bp-01",
        "- [x] 已检查前后段落",
        "评估完成",
        evidence(),
    );
    let detail = TodoDetail::parse("todos/todo-review.md", &source).unwrap();

    assert_eq!(detail.id, "todo-review");
    assert_eq!(detail.blueprint_id, "bp-01");
    assert!(detail.completion_criteria.iter().all(|item| item.completed));
    assert_eq!(detail.results, "评估完成");
    assert_eq!(detail.evidence.len(), 1);
    assert!(detail.evidence[0].markdown.contains("`cargo test`"));
    assert_eq!(detail.revisions[0].id, "revision-review");
    assert!(detail.notes.contains("<!-- preserve -->"));
}

#[test]
fn rejects_todo_without_required_v2_frontmatter() {
    let source = todo_v2("todo-review", "bp-01", "- [ ] 检查", "", evidence());
    let invalid = source.replacen("schema: blueprint/todo/v2", "schema: blueprint/v2", 1);
    let error = TodoDetail::parse("todos/todo-review.md", &invalid).unwrap_err();
    assert!(
        error.to_string().contains("unsupported schema"),
        "{error:#}"
    );
}
