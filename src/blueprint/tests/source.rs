use super::super::source::ParsedBlueprintSource;

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
