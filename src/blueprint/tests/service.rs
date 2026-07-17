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
