use super::super::source::{standard_evidence_links, validate_evidence_aggregate};
use super::super::{BlueprintSource, BlueprintState, TodoStatus};

fn blueprint_v3(todos: &str) -> String {
    format!(
        "---\nschema: blueprint/v3\nid: bp-01\nstate: active\n---\n\n# 写作计划\n\n## Record\n\n- Created By: planner\n\n## Intent\n\n~~~\n完成初稿\n~~~\n\n## Constraints\n\n~~~\n- 保持风格\n~~~\n\n## Definition of Done\n\n- [ ] 初稿完成 ^dod-1\n\n## Plan\n\n~~~\n先研究，再写作\n~~~\n\n## Rubric\n\n~~~\n核对目标和约束\n~~~\n\n## Todos\n\n{todos}\n## Results\n\n~~~\n\n~~~\n\n## Evidence\n\n### 已确认素材 ^evidence-1\n\n- Observation: 可用。\n\n## Revision History\n\n### 创建计划 ^revision-1\n\n- Changed By: planner\n\n## Notes\n\n~~~\n<!-- preserve -->\n~~~\n"
    )
}

#[test]
fn parses_v3_graph_links_without_loading_todo_details() {
    let source = blueprint_v3(
        "- [/] [完成初稿](todos/todo-1.md) ^todo-1\n  - Created By: planner\n  - Owner: writer\n",
    );

    let parsed = BlueprintSource::parse("bp-01/blueprint.md", &source).unwrap();

    assert_eq!(parsed.state, BlueprintState::Active);
    assert_eq!(parsed.todos[0].id, "todo-1");
    assert_eq!(parsed.todos[0].title, "完成初稿");
    assert_eq!(parsed.todos[0].document, "todos/todo-1.md");
    assert_eq!(parsed.todos[0].status, TodoStatus::InProgress);
    assert_eq!(parsed.rubric, "核对目标和约束");
    assert_eq!(parsed.evidence[0].id, "evidence-1");
    assert!(parsed.evidence[0].markdown.contains("Observation: 可用。"));
    assert_eq!(parsed.revisions[0].id, "revision-1");
    assert!(parsed.notes.contains("<!-- preserve -->"));
}

#[test]
fn evidence_link_resolver_rejects_duplicate_ids_and_targets_outside_the_aggregate() {
    let duplicate = blueprint_v3("").replace(
        "### 已确认素材 ^evidence-1",
        "### 已确认素材 ^evidence-1\n\n### 重复 ^evidence-1",
    );
    assert!(
        validate_evidence_aggregate(&duplicate, &[])
            .unwrap_err()
            .to_string()
            .contains("duplicate Evidence ID")
    );

    let invalid = blueprint_v3("").replace(
        "### 已确认素材 ^evidence-1",
        "[bad](../../outside.md#^evidence-1)\n\n### 已确认素材 ^evidence-1",
    );
    assert!(
        validate_evidence_aggregate(&invalid, &[])
            .unwrap_err()
            .to_string()
            .contains("invalid Evidence target")
    );
}

#[test]
fn evidence_link_resolver_uses_only_real_markdown_links() {
    let links = standard_evidence_links(
        "not-a-link](#^evidence-fake)\n\n```md\n[example](#^evidence-literal)\n```\n\n[real](#^evidence-real)",
    )
    .unwrap();
    assert_eq!(links, vec![("".into(), "evidence-real".into())]);
}

#[test]
fn assembles_nested_children_without_losing_grandchildren() {
    let source = blueprint_v3(
        "- [ ] [父任务](todos/todo-1.md) ^todo-1\n  - Created By: planner\n  - Children:\n    - [ ] [子任务](todos/todo-2.md) ^todo-2\n      - Created By: planner\n      - Children:\n        - [ ] [孙任务](todos/todo-3.md) ^todo-3\n          - Created By: planner\n",
    );

    let parsed = BlueprintSource::parse("bp-01/blueprint.md", &source).unwrap();

    assert_eq!(parsed.todos.len(), 1);
    assert_eq!(parsed.todos[0].id, "todo-1");
    assert_eq!(parsed.todos[0].children[0].id, "todo-2");
    assert_eq!(parsed.todos[0].children[0].children[0].id, "todo-3");
}

#[test]
fn requires_v3_frontmatter_and_standard_todo_links() {
    let source = blueprint_v3("- [ ] 普通任务 ^todo-1\n");
    let error = BlueprintSource::parse("bp-01/blueprint.md", &source).unwrap_err();
    assert!(
        error.to_string().contains("standard Markdown link"),
        "{error:#}"
    );

    let missing_frontmatter = source.replacen(
        "---\nschema: blueprint/v3\nid: bp-01\nstate: active\n---\n\n",
        "",
        1,
    );
    let error = BlueprintSource::parse("bp-01/blueprint.md", &missing_frontmatter).unwrap_err();
    assert!(error.to_string().contains("frontmatter"), "{error:#}");
}

#[test]
fn rejects_an_empty_rubric() {
    let source = blueprint_v3("").replace("~~~\n核对目标和约束\n~~~", "~~~\n   \n~~~");

    let error = BlueprintSource::parse("bp-01/blueprint.md", &source).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("Rubric external body must contain non-whitespace content"),
        "{error:#}"
    );
}
