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

fn assert_invalid(todos: &[Todo], expected: &str) {
    let error = validate_dependency_graph(todos).expect_err("graph must be rejected");
    assert!(error.to_string().contains(expected), "{error:#}");
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

#[test]
fn rejects_self_unknown_and_duplicate_todo_dependencies() {
    assert_invalid(
        &[todo("todo-self", TodoStatus::Pending, &["todo-self"])],
        "cannot depend on itself",
    );
    assert_invalid(
        &[todo(
            "todo-dependent",
            TodoStatus::Pending,
            &["todo-missing"],
        )],
        "unknown Todo",
    );
    assert_invalid(
        &[
            todo("todo-duplicate", TodoStatus::Pending, &[]),
            todo("todo-duplicate", TodoStatus::Pending, &[]),
        ],
        "duplicate Todo ID",
    );
}

#[test]
fn enforces_required_fields_for_each_todo_state() {
    let mut missing_creator = todo("todo-created", TodoStatus::Pending, &[]);
    missing_creator.created_by = None;
    assert_invalid(&[missing_creator], "missing Created By");

    let running = todo("todo-running", TodoStatus::InProgress, &[]);
    assert_invalid(&[running], "missing Owner");

    let mut completed = todo("todo-completed", TodoStatus::Completed, &[]);
    completed.result_summary = Some("result".into());
    assert_invalid(&[completed.clone()], "missing Completed By");
    completed.completed_by = Some("agent".into());
    assert!(validate_dependency_graph(&[completed]).is_ok());

    let mut blocked = todo("todo-blocked", TodoStatus::Blocked, &[]);
    blocked.handoff = vec!["resume later".into()];
    assert_invalid(&[blocked.clone()], "missing Block Reason");
    blocked.block_reason = Some("external dependency".into());
    assert!(validate_dependency_graph(&[blocked]).is_ok());

    let cancelled = todo("todo-cancelled", TodoStatus::Cancelled, &[]);
    assert_invalid(&[cancelled], "missing Cancel Reason");
}

#[test]
fn completed_parent_requires_every_non_cancelled_child_to_be_completed() {
    let mut parent = todo("todo-parent", TodoStatus::Completed, &[]);
    parent.completed_by = Some("agent".into());
    parent.result_summary = Some("parent result".into());
    parent.children = vec![todo("todo-child", TodoStatus::Pending, &[])];
    assert_invalid(&[parent.clone()], "open child Todos");

    parent.children[0].status = TodoStatus::Cancelled;
    parent.children[0].cancel_reason = Some("not needed".into());
    validate_dependency_graph(&[parent.clone()]).expect("cancelled child is terminal");

    parent.children[0].status = TodoStatus::Completed;
    parent.children[0].cancel_reason = None;
    parent.children[0].completed_by = Some("agent".into());
    parent.children[0].result_summary = Some("child result".into());
    validate_dependency_graph(&[parent]).expect("completed child is terminal");
}

#[test]
fn readiness_contains_only_pending_todos_and_reports_dependency_status() {
    let mut completed = todo("todo-completed", TodoStatus::Completed, &[]);
    completed.completed_by = Some("agent".into());
    completed.result_summary = Some("done".into());
    let mut running = todo("todo-running", TodoStatus::InProgress, &[]);
    running.owner = Some("agent".into());
    let mut blocked = todo("todo-blocked", TodoStatus::Blocked, &[]);
    blocked.block_reason = Some("external".into());
    blocked.handoff = vec!["resume".into()];
    let mut cancelled = todo("todo-cancelled", TodoStatus::Cancelled, &[]);
    cancelled.cancel_reason = Some("obsolete".into());
    let ready = todo("todo-ready", TodoStatus::Pending, &["todo-completed"]);
    let waiting = todo("todo-waiting", TodoStatus::Pending, &["todo-blocked"]);

    let readiness = derive_readiness(&[completed, running, blocked, cancelled, ready, waiting])
        .expect("derive readiness");
    assert_eq!(readiness.ready, vec!["todo-ready"]);
    assert_eq!(readiness.not_ready.len(), 1);
    assert_eq!(readiness.not_ready[0].id, "todo-waiting");
    assert_eq!(
        readiness.not_ready[0].unsatisfied_dependencies[0].status,
        TodoStatus::Blocked
    );
}
