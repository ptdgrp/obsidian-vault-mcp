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
