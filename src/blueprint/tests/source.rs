use super::super::source::{standard_evidence_links, validate_evidence_aggregate};
use super::super::{BlueprintSource, BlueprintState, TodoStatus};

fn blueprint_v3(todos: &str) -> String {
    format!(
        "---\nschema: blueprint/v3\nid: bp-01\nstate: active\ncreated_by: planner\n---\n\n# 写作计划\n\n## Intent\n\n~~~\n完成初稿\n~~~\n\n## Constraints\n\n~~~\n- 保持风格\n~~~\n\n## Definition of Done\n\n- [ ] 初稿完成 ^dod-1\n\n## Plan\n\n~~~\n先研究，再写作\n~~~\n\n## Todo Graph\n\n{todos}\n## Results\n\n~~~\n\n~~~\n\n## Rubric\n\n~~~\n核对目标和约束\n~~~\n\n## Notes\n\n~~~\n<!-- preserve -->\n~~~\n\n## Revision History\n\n### 创建计划 ^revision-1\n\n- Changed By: planner\n\n"
    )
}

#[test]
fn parses_v3_graph_links_without_loading_todo_details() {
    let source = blueprint_v3("- [/] [完成初稿](todos/todo-1.md) ^todo-1\n");

    let parsed = BlueprintSource::parse("bp-01/blueprint.md", &source).unwrap();

    assert_eq!(parsed.state, BlueprintState::Active);
    assert_eq!(parsed.todos[0].id, "todo-1");
    assert_eq!(parsed.todos[0].title, "完成初稿");
    assert_eq!(parsed.todos[0].document, "todos/todo-1.md");
    assert_eq!(parsed.todos[0].status, TodoStatus::InProgress);
    assert_eq!(parsed.rubric, "核对目标和约束");
    assert_eq!(parsed.revisions[0].id, "revision-1");
    assert!(parsed.notes.contains("<!-- preserve -->"));
}

#[test]
fn evidence_link_resolver_rejects_duplicate_ids_and_targets_outside_the_aggregate() {
    let duplicate = blueprint_v3("");
    let todo = "---\nschema: blueprint/todo/v3\nid: todo-1\nblueprint: bp-01\ncreated_by: planner\nowner: planner\n---\n\n# Todo\n\n## Plan\n\n~~~\nwork\n~~~\n\n## Completion Criteria\n\n## Handoff\n\n~~~\n\n~~~\n\n## Result\n\n~~~\n\n~~~\n\n## Evidence\n\n### 已确认素材 ^evidence-1\n\n### 重复 ^evidence-1\n\n## Notes\n\n~~~\n\n~~~\n\n## Revision History\n\n";
    let error = validate_evidence_aggregate(&duplicate, &[("todos/todo-1.md", todo)]).unwrap_err();
    assert!(
        error.to_string().contains("duplicate Evidence ID"),
        "{error:#}"
    );

    let invalid = blueprint_v3("").replace(
        "## Results\n\n~~~\n\n~~~\n\n## Rubric",
        "## Results\n\n~~~\n\n~~~\n\n[bad](../../outside.md#^evidence-1)\n\n## Rubric",
    );
    let valid_todo = todo.replacen("\n### 重复 ^evidence-1\n", "", 1);
    let error =
        validate_evidence_aggregate(&invalid, &[("todos/todo-1.md", &valid_todo)]).unwrap_err();
    assert!(
        error.to_string().contains("invalid Evidence target"),
        "{error:#}"
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
        "- [ ] [父任务](todos/todo-1.md) ^todo-1\n  - Children:\n    - [ ] [子任务](todos/todo-2.md) ^todo-2\n      - Children:\n        - [ ] [孙任务](todos/todo-3.md) ^todo-3\n",
    );

    let parsed = BlueprintSource::parse("bp-01/blueprint.md", &source).unwrap();

    assert_eq!(parsed.todos.len(), 1);
    assert_eq!(parsed.todos[0].id, "todo-1");
    assert_eq!(parsed.todos[0].children[0].id, "todo-2");
    assert_eq!(parsed.todos[0].children[0].children[0].id, "todo-3");
}

#[test]
fn rejects_metadata_in_the_todo_graph() {
    let source = blueprint_v3("- [ ] [任务](todos/todo-1.md) ^todo-1\n  - Owner: agent\n");
    let error = BlueprintSource::parse("bp-01/blueprint.md", &source).unwrap_err();
    assert!(
        error.to_string().contains("not graph structure"),
        "{error:#}"
    );
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
        "---\nschema: blueprint/v3\nid: bp-01\nstate: active\ncreated_by: planner\n---\n\n",
        "",
        1,
    );
    let error = BlueprintSource::parse("bp-01/blueprint.md", &missing_frontmatter).unwrap_err();
    assert!(error.to_string().contains("frontmatter"), "{error:#}");
}

#[test]
fn allows_an_empty_optional_rubric() {
    let source = blueprint_v3("").replace("~~~\n核对目标和约束\n~~~", "~~~\n   \n~~~");

    BlueprintSource::parse("bp-01/blueprint.md", &source).unwrap();
}
