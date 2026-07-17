use camino::Utf8PathBuf;
use tempfile::tempdir;

use super::super::service::{BlueprintCreateRequest, BlueprintService};

#[test]
fn creating_a_blueprint_automatically_creates_workspace_and_generates_protocol_document() {
    let directory = tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
    let service = BlueprintService::new(root);
    let created = service
        .blueprint_create(BlueprintCreateRequest {
            title: "实现 Blueprint".to_string(),
            created_by: "agent".to_string(),
            intent: "完成协议".to_string(),
            constraints: vec!["保留 Markdown".to_string()],
            definition_of_done: vec!["完成实现".to_string()],
            plan: "逐步完成".to_string(),
        })
        .unwrap();
    assert!(created.id.starts_with("bp-"));
    assert!(created.source.contains("- Created By: agent"));
    assert!(created.source.contains("^dod-"));
    assert!(
        service
            .workspace_root()
            .join("active")
            .join(format!("{}.md", created.id))
            .is_file()
    );
}

#[test]
fn get_list_and_status_return_the_active_blueprint_and_derived_empty_todo_state() {
    let directory = tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
    let service = BlueprintService::new(root);
    let created = service
        .blueprint_create(BlueprintCreateRequest {
            title: "实现 Blueprint".to_string(),
            created_by: "agent".to_string(),
            intent: "完成协议".to_string(),
            constraints: vec![],
            definition_of_done: vec!["完成实现".to_string()],
            plan: "逐步完成".to_string(),
        })
        .unwrap();
    assert_eq!(service.blueprint_list().unwrap(), vec![created.id.clone()]);
    assert_eq!(
        service.blueprint_get(&created.id).unwrap().etag,
        created.etag
    );
    let status = service.blueprint_status(&created.id).unwrap();
    assert!(status.ready_todos.is_empty());
    assert!(status.not_ready_todos.is_empty());
}

#[test]
fn todo_create_and_start_write_protocol_state_after_owner_assignment() {
    let directory = tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
    let service = BlueprintService::new(root);
    let blueprint = service
        .blueprint_create(BlueprintCreateRequest {
            title: "实现".into(),
            created_by: "agent".into(),
            intent: "完成".into(),
            constraints: vec![],
            definition_of_done: vec!["完成".into()],
            plan: "执行".into(),
        })
        .unwrap();
    let todo = service
        .todo_create(&blueprint.id, "实现存储", "agent", Some("agent"), &[], None)
        .unwrap();
    assert_eq!(todo.status, super::super::model::TodoStatus::Pending);
    let started = service.todo_start(&blueprint.id, &todo.id, None).unwrap();
    assert_eq!(started.status, super::super::model::TodoStatus::InProgress);
}

#[test]
fn todo_completion_requires_criteria_then_close_moves_the_document() {
    let directory = tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
    let service = BlueprintService::new(root);
    let blueprint = service
        .blueprint_create(BlueprintCreateRequest {
            title: "实现".into(),
            created_by: "agent".into(),
            intent: "完成".into(),
            constraints: vec![],
            definition_of_done: vec!["完成".into()],
            plan: "执行".into(),
        })
        .unwrap();
    let todo = service
        .todo_create_full(
            &blueprint.id,
            "实现存储",
            "agent",
            None,
            Some("agent"),
            &[],
            &["测试通过".into()],
            None,
        )
        .unwrap();
    service.todo_start(&blueprint.id, &todo.id, None).unwrap();
    assert!(
        service
            .todo_complete(&blueprint.id, &todo.id, "agent", "完成", None)
            .is_err()
    );
    service
        .todo_update(
            &blueprint.id,
            &todo.id,
            None,
            None,
            Some(&[super::super::service::CheckUpdate {
                text: "测试通过".into(),
                completed: true,
            }]),
            None,
            None,
            None,
        )
        .unwrap();
    let completed = service
        .todo_complete(&blueprint.id, &todo.id, "agent", "完成", None)
        .unwrap();
    assert_eq!(completed.status, super::super::model::TodoStatus::Completed);
    let closed = service
        .blueprint_close(&blueprint.id, "agent", Some("DoD 尚未更新"), None)
        .unwrap();
    assert_eq!(closed.state, "closed");
    assert_eq!(
        service.blueprint_list_in("closed").unwrap(),
        vec![blueprint.id]
    );
}

#[test]
fn creates_child_todos_in_the_parent_children_list() {
    let directory = tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
    let service = BlueprintService::new(root);
    let blueprint = service
        .blueprint_create(BlueprintCreateRequest {
            title: "实现".into(),
            created_by: "agent".into(),
            intent: "完成".into(),
            constraints: vec![],
            definition_of_done: vec!["完成".into()],
            plan: "执行".into(),
        })
        .unwrap();
    let parent = service
        .todo_create_full(&blueprint.id, "父任务", "agent", None, None, &[], &[], None)
        .unwrap();
    let child = service
        .todo_create_full(
            &blueprint.id,
            "子任务",
            "agent",
            Some(&parent.id),
            None,
            &[],
            &[],
            None,
        )
        .unwrap();
    let loaded = service.todo_get(&blueprint.id, &parent.id).unwrap();
    assert_eq!(loaded.children.len(), 1);
    assert_eq!(loaded.children[0].id, child.id);
}
