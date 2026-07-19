use super::super::source::validate_evidence_aggregate;
use super::super::{BlueprintSource, BlueprintState, TodoStatus};

fn blueprint_v2(todos: &str) -> String {
    format!(
        "---\nschema: blueprint/v2\nid: bp-01\nstate: active\n---\n\n# 写作计划\n\n## Record\n\n- Created By: planner\n\n## Intent\n\n完成初稿\n\n## Constraints\n\n- 保持风格\n\n## Definition of Done\n\n- [ ] 初稿完成 ^dod-draft\n\n## Plan\n\n先研究，再写作\n\n## Rubric\n\n核对目标和约束\n\n## Todos\n\n{todos}\n## Results\n\n\n## Evidence\n\n### 已确认素材 ^evidence-source\n\n- Observation: 可用。\n\n## Revision History\n\n### 创建计划 ^revision-source\n\n- Changed By: planner\n\n## Notes\n\n<!-- preserve -->\n"
    )
}

#[test]
fn parses_v2_graph_links_without_loading_todo_details() {
    let source = blueprint_v2(
        "- [/] [完成初稿](todos/todo-draft.md) ^todo-draft\n  - Created By: planner\n  - Owner: writer\n",
    );

    let parsed = BlueprintSource::parse("bp-01/blueprint.md", &source).unwrap();

    assert_eq!(parsed.state, BlueprintState::Active);
    assert_eq!(parsed.todos[0].id, "todo-draft");
    assert_eq!(parsed.todos[0].title, "完成初稿");
    assert_eq!(parsed.todos[0].document, "todos/todo-draft.md");
    assert_eq!(parsed.todos[0].status, TodoStatus::InProgress);
    assert_eq!(parsed.rubric, "核对目标和约束");
    assert_eq!(parsed.evidence[0].id, "evidence-source");
    assert!(parsed.evidence[0].markdown.contains("Observation: 可用。"));
    assert_eq!(parsed.revisions[0].id, "revision-source");
    assert!(parsed.notes.contains("<!-- preserve -->"));
}

#[test]
fn evidence_link_resolver_rejects_duplicate_ids_and_targets_outside_the_aggregate() {
    let duplicate = blueprint_v2("").replace(
        "### 已确认素材 ^evidence-source",
        "### 已确认素材 ^evidence-source\n\n### 重复 ^evidence-source",
    );
    assert!(
        validate_evidence_aggregate(&duplicate, &[])
            .unwrap_err()
            .to_string()
            .contains("duplicate Evidence ID")
    );

    let invalid = blueprint_v2("").replace(
        "### 已确认素材 ^evidence-source",
        "[bad](../../outside.md#^evidence-source)\n\n### 已确认素材 ^evidence-source",
    );
    assert!(
        validate_evidence_aggregate(&invalid, &[])
            .unwrap_err()
            .to_string()
            .contains("invalid Evidence target")
    );
}

#[test]
fn assembles_nested_children_without_losing_grandchildren() {
    let source = blueprint_v2(
        "- [ ] [父任务](todos/todo-parent.md) ^todo-parent\n  - Created By: planner\n  - Children:\n    - [ ] [子任务](todos/todo-child.md) ^todo-child\n      - Created By: planner\n      - Children:\n        - [ ] [孙任务](todos/todo-grandchild.md) ^todo-grandchild\n          - Created By: planner\n",
    );

    let parsed = BlueprintSource::parse("bp-01/blueprint.md", &source).unwrap();

    assert_eq!(parsed.todos.len(), 1);
    assert_eq!(parsed.todos[0].id, "todo-parent");
    assert_eq!(parsed.todos[0].children[0].id, "todo-child");
    assert_eq!(
        parsed.todos[0].children[0].children[0].id,
        "todo-grandchild"
    );
}

#[test]
fn requires_v2_frontmatter_and_standard_todo_links() {
    let source = blueprint_v2("- [ ] 普通任务 ^todo-draft\n");
    let error = BlueprintSource::parse("bp-01/blueprint.md", &source).unwrap_err();
    assert!(
        error.to_string().contains("standard Markdown link"),
        "{error:#}"
    );

    let missing_frontmatter = source.replacen(
        "---\nschema: blueprint/v2\nid: bp-01\nstate: active\n---\n\n",
        "",
        1,
    );
    let error = BlueprintSource::parse("bp-01/blueprint.md", &missing_frontmatter).unwrap_err();
    assert!(error.to_string().contains("frontmatter"), "{error:#}");
}

#[test]
fn rejects_an_empty_rubric() {
    let source = blueprint_v2("").replace("## Rubric\n\n核对目标和约束", "## Rubric\n\n   ");

    let error = BlueprintSource::parse("bp-01/blueprint.md", &source).unwrap_err();

    assert!(
        error.to_string().contains("Rubric must not be empty"),
        "{error:#}"
    );
}
