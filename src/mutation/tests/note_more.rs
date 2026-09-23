use super::{fixture, read_note, write_note};

#[test]
fn create_note_creates_nested_file_and_never_overwrites() {
    let (dir, mutations) = fixture();
    let result = mutations
        .create_note("drafts/New.md", "# New\n\nBody\n")
        .expect("create note");
    assert_eq!(result.path, "drafts/New.md");
    assert_eq!(read_note(&dir, "drafts/New.md"), "# New\n\nBody\n");
    let error = mutations
        .create_note("drafts/New.md", "different")
        .expect_err("existing note must not be overwritten");
    assert!(error.to_string().contains("cannot create note"));
    assert_eq!(read_note(&dir, "drafts/New.md"), "# New\n\nBody\n");
}

#[test]
fn note_creation_and_deletion_require_exact_safe_markdown_paths() {
    let (dir, mutations) = fixture();
    for path in ["drafts/New", "../outside.md", "/outside.md"] {
        assert!(
            mutations.create_note(path, "body").is_err(),
            "create {path}"
        );
        assert!(mutations.delete_note(path, false).is_err(), "delete {path}");
    }
    assert!(!dir.path().join("drafts/New.md").exists());
}

#[test]
fn delete_note_previews_then_removes_unreferenced_note() {
    let (dir, mutations) = fixture();
    write_note(&dir, "drafts/Unused.md", "# Unused\n");
    let preview = mutations
        .delete_note("drafts/Unused.md", true)
        .expect("preview");
    assert_eq!(preview.path, "drafts/Unused.md");
    assert!(preview.dry_run);
    assert!(dir.path().join("drafts/Unused.md").exists());
    let applied = mutations
        .delete_note("drafts/Unused.md", false)
        .expect("delete");
    assert!(!applied.dry_run);
    assert!(!dir.path().join("drafts/Unused.md").exists());
}

#[test]
fn delete_note_refuses_external_links_and_embeds() {
    let (dir, mutations) = fixture();
    write_note(&dir, "drafts/Target.md", "# Target\n");
    write_note(
        &dir,
        "External.md",
        "[[drafts/Target.md]]\n![[drafts/Target.md]]\n[target](drafts/Target.md)\n",
    );
    let error = mutations
        .delete_note("drafts/Target.md", false)
        .expect_err("referenced note must not be deleted");
    assert!(error.to_string().contains("[[drafts/Target.md]]"));
    assert!(error.to_string().contains("![[drafts/Target.md]]"));
    assert!(error.to_string().contains("[target](drafts/Target.md)"));
    assert!(dir.path().join("drafts/Target.md").exists());
}

#[cfg(unix)]
#[test]
fn note_tools_reject_paths_through_symlinks_outside_vault() {
    use std::os::unix::fs::symlink;

    let (dir, mutations) = fixture();
    let outside = tempfile::tempdir().expect("outside directory");
    std::fs::write(outside.path().join("Existing.md"), "outside").expect("outside note");
    symlink(outside.path(), dir.path().join("escape")).expect("create symlink");

    assert!(mutations.create_note("escape/New.md", "# New\n").is_err());
    assert!(!outside.path().join("New.md").exists());
    assert!(mutations.delete_note("escape/Existing.md", false).is_err());
    assert_eq!(
        std::fs::read_to_string(outside.path().join("Existing.md")).unwrap(),
        "outside"
    );
}
