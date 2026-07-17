use super::super::{
    model::{Todo, TodoStatus},
    validate::{derive_readiness, validate_dependency_graph},
};

#[test]
fn serializes_protocol_statuses() {
    assert_eq!(
        serde_json::to_string(&TodoStatus::Pending).unwrap(),
        "\"pending\""
    );
    assert_eq!(
        serde_json::to_string(&TodoStatus::InProgress).unwrap(),
        "\"in_progress\""
    );
    assert_eq!(TodoStatus::Blocked.marker(), "?");
}

fn todo(id: &str, status: TodoStatus, depends_on: &[&str]) -> Todo {
    Todo {
        id: id.to_string(),
        title: id.to_string(),
        status,
        created_by: Some("agent".to_string()),
        owner: None,
        completed_by: None,
        depends_on: depends_on.iter().map(|id| (*id).to_string()).collect(),
        completion_criteria: Vec::new(),
        handoff: Vec::new(),
        result_summary: None,
        references: Vec::new(),
        block_reason: None,
        cancel_reason: None,
        children: Vec::new(),
    }
}

#[test]
fn rejects_dependency_cycles_and_derives_cancelled_dependency_as_not_ready() {
    let cycle = vec![
        todo("todo-a", TodoStatus::Pending, &["todo-b"]),
        todo("todo-b", TodoStatus::Pending, &["todo-a"]),
    ];
    assert!(
        validate_dependency_graph(&cycle)
            .unwrap_err()
            .to_string()
            .contains("cycle")
    );

    let mut cancelled = todo("todo-a", TodoStatus::Cancelled, &[]);
    cancelled.cancel_reason = Some("不再需要".to_string());
    let todos = vec![cancelled, todo("todo-b", TodoStatus::Pending, &["todo-a"])];
    let readiness = derive_readiness(&todos).unwrap();
    assert!(readiness.ready.is_empty());
    assert_eq!(readiness.not_ready[0].id, "todo-b");
    assert_eq!(
        readiness.not_ready[0].unsatisfied_dependencies[0].status,
        TodoStatus::Cancelled
    );
}
