use std::fs;

use camino::Utf8PathBuf;
use tempfile::{TempDir, tempdir};

use super::super::store::BlueprintStore;
use super::super::{BlueprintSource, BlueprintState, TodoDetail};

// Legacy Service tests still compare the v2 enum with v1 string status values until Task 4
// replaces that Service. Keep this compatibility exclusively in the test crate.
impl PartialEq<&str> for BlueprintState {
    fn eq(&self, other: &&str) -> bool {
        matches!(
            (*self, *other),
            (BlueprintState::Active, "active")
                | (BlueprintState::Closed, "closed")
                | (BlueprintState::Cancelled, "cancelled")
        )
    }
}

fn blueprint_source() -> String {
    "---\nschema: blueprint/v2\nid: bp-01\nstate: active\n---\n\n# Test\n\n## Record\n\n- Created By: tester\n\n## Intent\n\nTest the store\n\n## Constraints\n\nNone\n\n## Definition of Done\n\n- [ ] Store works ^dod-store\n\n## Plan\n\nWrite tests\n\n## Rubric\n\nKeep aggregate files stable\n\n## Todos\n\n- [ ] [Todo A](todos/todo-a.md) ^todo-a\n\n## Results\n\n\n## Evidence\n\n\n## Revision History\n\n\n## Notes\n\n<!-- preserve -->\n".to_string()
}

fn todo_source() -> String {
    "---\nschema: blueprint/todo/v2\nid: todo-a\nblueprint: bp-01\n---\n\n# Todo A\n\n## Intent\n\nTest one Todo\n\n## Completion Criteria\n\n- [ ] It is stored\n\n## Plan\n\nWrite it\n\n## Handoff\n\nNone\n\n## Results\n\n\n## Evidence\n\n\n## Revision History\n\n\n## Notes\n\n<!-- preserve -->\n".to_string()
}

fn store() -> (TempDir, BlueprintStore) {
    let directory = tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
    (directory, BlueprintStore::new(root))
}

fn store_with_blueprint() -> (TempDir, BlueprintStore) {
    let (directory, store) = store();
    store.create("bp-01", &blueprint_source()).unwrap();
    (directory, store)
}

#[test]
fn creates_v2_aggregate_and_keeps_path_stable_across_state_changes() {
    let (_dir, store) = store();
    let created = store.create("bp-01", &blueprint_source()).unwrap();

    assert_eq!(created.state, BlueprintState::Active);
    assert!(
        store
            .workspace_root()
            .join("blueprints/bp-01/blueprint.md")
            .is_file()
    );
    assert!(
        store
            .workspace_root()
            .join("blueprints/bp-01/todos")
            .is_dir()
    );

    let closed = store
        .set_state("bp-01", BlueprintState::Closed, Some(&created.etag))
        .unwrap();
    assert_eq!(closed.state, BlueprintState::Closed);
    assert!(
        store
            .workspace_root()
            .join("blueprints/bp-01/blueprint.md")
            .is_file()
    );
    assert_eq!(
        BlueprintSource::parse("blueprint.md", &closed.source)
            .unwrap()
            .state,
        BlueprintState::Closed
    );
}

#[test]
fn stores_todo_documents_with_independent_etags_and_detects_orphans() {
    let (_dir, store) = store_with_blueprint();
    let todo = store
        .create_todo("bp-01", "todo-a", &todo_source())
        .unwrap();

    assert_ne!(store.read("bp-01").unwrap().etag, todo.etag);
    assert_eq!(
        TodoDetail::parse("todo-a.md", &todo.source).unwrap().id,
        "todo-a"
    );
    fs::write(store.todo_path("bp-01", "todo-orphan"), todo_source()).unwrap();

    assert!(
        store
            .validate_aggregate("bp-01")
            .unwrap_err()
            .to_string()
            .contains("orphan Todo document")
    );
}

#[test]
fn detects_missing_todo_documents_and_rejects_v1_workspaces() {
    let (_dir, store) = store_with_blueprint();
    assert!(
        store
            .validate_aggregate("bp-01")
            .unwrap_err()
            .to_string()
            .contains("missing Todo document")
    );

    fs::write(
        store.workspace_root().join("manifest.md"),
        "---\nschema: blueprint/v1\n---\n\n# Blueprint Workspace\n",
    )
    .unwrap();
    assert!(
        store
            .ensure_workspace()
            .unwrap_err()
            .to_string()
            .contains("unsupported Blueprint workspace schema: blueprint/v1")
    );
}

#[test]
fn write_operations_preserve_document_specific_etags_and_validate_sources() {
    let (_dir, store) = store_with_blueprint();
    let todo = store
        .create_todo("bp-01", "todo-a", &todo_source())
        .unwrap();
    let blueprint = store.read("bp-01").unwrap();

    let updated = store
        .write_todo("bp-01", "todo-a", Some(&todo.etag), |source| {
            Ok(source.replacen("Test one Todo", "Updated Todo", 1))
        })
        .unwrap();
    assert_ne!(todo.etag, updated.etag);
    assert_eq!(store.read("bp-01").unwrap().etag, blueprint.etag);

    assert!(
        store
            .write_blueprint("bp-01", Some(&blueprint.etag), |source| {
                Ok(source.replacen("blueprint/v2", "blueprint/v1", 1))
            })
            .unwrap_err()
            .to_string()
            .contains("unsupported schema: blueprint/v1")
    );
}
