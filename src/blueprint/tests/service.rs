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
