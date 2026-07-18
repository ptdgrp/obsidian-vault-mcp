use std::fs;

use camino::Utf8PathBuf;
use tempfile::tempdir;

use super::super::store::BlueprintStore;

#[test]
fn automatically_creates_protocol_layout_and_rejects_stale_etag() {
    let directory = tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
    let store = BlueprintStore::new(root);
    let created = store.create("bp-01", "# Test\n").expect("create Blueprint");
    assert_eq!(
        fs::read_to_string(store.workspace_root().join("manifest.md")).unwrap(),
        "---\nschema: blueprint/v1\n---\n\n# Blueprint Workspace\n"
    );
    assert!(store.workspace_root().join("active").is_dir());
    assert!(store.workspace_root().join("closed").is_dir());
    let error = store
        .write("bp-01", Some("stale"), |_| Ok("# Changed\n".to_string()))
        .expect_err("stale ETag must fail");
    assert!(error.to_string().contains("etag"));
    assert_eq!(created.id, "bp-01");
}

#[test]
fn rejects_an_existing_workspace_with_the_wrong_manifest() {
    let directory = tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
    let store = BlueprintStore::new(root);
    fs::create_dir_all(store.workspace_root()).expect("create workspace root");
    let manifest = store.workspace_root().join("manifest.md");
    fs::write(&manifest, "---\nschema: blueprint/v0\n---\n").expect("write invalid manifest");

    let error = store
        .ensure_workspace()
        .expect_err("invalid manifest must fail");
    assert!(
        error
            .to_string()
            .contains("invalid Blueprint workspace manifest")
    );
    assert_eq!(
        fs::read_to_string(manifest).expect("manifest remains readable"),
        "---\nschema: blueprint/v0\n---\n"
    );
}

#[test]
fn rejects_invalid_ids_states_duplicate_creation_and_missing_reads() {
    let directory = tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
    let store = BlueprintStore::new(root);

    for id in ["bp-", "plain", "bp-a/b", "bp-a\\b"] {
        let error = store
            .create(id, "# Test\n")
            .expect_err("invalid ID must fail");
        assert!(error.to_string().contains("invalid Blueprint ID"));
    }
    assert!(
        store
            .list("archived")
            .unwrap_err()
            .to_string()
            .contains("state")
    );
    assert!(
        store
            .read_in("bp-01", "archived")
            .unwrap_err()
            .to_string()
            .contains("state")
    );

    store.create("bp-01", "# Test\n").expect("create Blueprint");
    assert!(
        store
            .create("bp-01", "# Duplicate\n")
            .unwrap_err()
            .to_string()
            .contains("already exists")
    );
    assert!(
        store
            .read("bp-missing")
            .unwrap_err()
            .to_string()
            .contains("does not exist")
    );
}

#[test]
fn move_enforces_etag_and_relocates_the_active_document() {
    let directory = tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
    let store = BlueprintStore::new(root);
    let created = store.create("bp-01", "# Test\n").expect("create Blueprint");

    let error = store
        .move_to("bp-01", "active", Some(&created.etag), |source| {
            Ok(source.to_string())
        })
        .expect_err("moving to active must fail");
    assert!(
        error
            .to_string()
            .contains("cannot move Blueprint to active")
    );
    let error = store
        .move_to("bp-01", "closed", Some("stale"), |source| {
            Ok(source.to_string())
        })
        .expect_err("stale move must fail");
    assert!(error.to_string().contains("etag"));

    let closed = store
        .move_to("bp-01", "closed", Some(&created.etag), |source| {
            Ok(format!("{source}closed\n"))
        })
        .expect("move Blueprint");
    assert_eq!(closed.state, "closed");
    assert_eq!(closed.source, "# Test\nclosed\n");
    assert!(!store.workspace_root().join("active/bp-01.md").exists());
    assert!(store.workspace_root().join("closed/bp-01.md").is_file());
    assert!(
        store
            .write("bp-01", None, |source| Ok(source.to_string()))
            .is_err()
    );
}
