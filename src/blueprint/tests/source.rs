use super::super::{model::TodoStatus, source::ParsedBlueprintSource};

fn blueprint(todos: &str) -> String {
    format!(
        "# 测试\n\n## Record\n\n- Created By: agent\n\n## Intent\n\n目标\n\n## Constraints\n\n- 限制\n\n## Definition of Done\n\n- [ ] 完成 ^dod-01\n\n## Plan\n\n计划\n\n## Todos\n\n{todos}\n## Results\n\n\n## Notes\n\n"
    )
}

#[test]
fn parses_todos_but_excludes_completion_criteria_from_graph() {
    let source = blueprint(
        "- [/] 父任务 ^todo-parent\n  - Created By: a\n  - Completion Criteria:\n    - [ ] 局部条件\n  - Children:\n    - [ ] 子任务 ^todo-child\n      - Created By: a\n",
    );
    let parsed = ParsedBlueprintSource::parse("bp-01.md", &source).unwrap();
    assert_eq!(parsed.todos.len(), 1);
    assert_eq!(parsed.todos[0].id, "todo-parent");
    assert_eq!(parsed.todos[0].children[0].id, "todo-child");
    assert_eq!(parsed.todos[0].completion_criteria.len(), 1);
}

#[test]
fn rejects_invalid_path_and_missing_or_duplicate_required_sections() {
    let source = blueprint("");
    let error = ParsedBlueprintSource::parse("bp-01.txt", &source)
        .err()
        .expect("non-Markdown path must fail");
    assert!(error.to_string().contains("must end with .md"));

    let missing_notes = source.replacen("## Notes\n\n", "", 1);
    let error = ParsedBlueprintSource::parse("bp-01.md", &missing_notes)
        .err()
        .expect("missing required section must fail");
    assert!(
        error
            .to_string()
            .contains("missing required section: Notes")
    );

    let duplicate_plan = source.replace("## Todos", "## Plan\n\nduplicate\n\n## Todos");
    let error = ParsedBlueprintSource::parse("bp-01.md", &duplicate_plan)
        .err()
        .expect("duplicate required section must fail");
    assert!(
        error
            .to_string()
            .contains("required section must occur exactly once: Plan")
    );
}

#[test]
fn parses_protocol_fields_statuses_and_children_without_promoting_criteria() {
    let source = blueprint(
        "- [?] 父任务 ^todo-parent\n  - Created By: creator\n  - Depends On: todo-a, todo-b\n  - Handoff: first\n  - Handoff: second\n  - Reference: [A](../../a.md)\n  - Reference: [B](../../b.md)\n  - Block Reason: waiting\n  - Completion Criteria:\n    - [x] checked\n  - Children:\n    - [-] 子任务 ^todo-child\n      - Created By: creator\n      - Cancel Reason: obsolete\n",
    );
    let parsed = ParsedBlueprintSource::parse("bp-01.md", &source).expect("parse Blueprint");
    assert_eq!(parsed.todos.len(), 1);
    let parent = &parsed.todos[0];
    assert_eq!(parent.status, TodoStatus::Blocked);
    assert_eq!(parent.depends_on, vec!["todo-a", "todo-b"]);
    assert_eq!(parent.handoff, vec!["first", "second"]);
    assert_eq!(
        parent.references,
        vec!["[A](../../a.md)", "[B](../../b.md)"]
    );
    assert_eq!(parent.completion_criteria.len(), 1);
    assert!(parent.completion_criteria[0].completed);
    assert_eq!(parent.children.len(), 1);
    assert_eq!(parent.children[0].status, TodoStatus::Cancelled);
}

#[test]
fn ignores_todo_shaped_tasks_outside_the_todos_section() {
    let source = blueprint("- [ ] executable ^todo-inside\n  - Created By: creator\n").replace(
        "## Notes\n\n",
        "## Notes\n\n- [ ] ordinary note task ^todo-outside\n  - Created By: creator\n",
    );
    let parsed = ParsedBlueprintSource::parse("bp-01.md", &source).expect("parse Blueprint");
    assert_eq!(
        parsed
            .todos
            .iter()
            .map(|todo| todo.id.as_str())
            .collect::<Vec<_>>(),
        vec!["todo-inside"]
    );
}
