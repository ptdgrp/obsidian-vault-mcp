use super::{fixture, read_note, write_note};

#[test]
fn edit_note_replaces_one_exact_occurrence() {
    let (dir, mutations) = fixture();
    write_note(&dir, "Text.md", "# Text\n\nold value\n");
    let preview = mutations
        .edit_note("Text.md", "old value", "new value", true)
        .expect("preview edit");
    assert!(preview.dry_run);
    assert_eq!(read_note(&dir, "Text.md"), "# Text\n\nold value\n");
    let result = mutations
        .edit_note("Text.md", "old value", "new value", false)
        .expect("edit");
    assert_eq!(result.path, "Text.md");
    assert!(!result.dry_run);
    assert_eq!(read_note(&dir, "Text.md"), "# Text\n\nnew value\n");
}

#[test]
fn edit_note_rejects_missing_or_ambiguous_text_without_writing() {
    let (dir, mutations) = fixture();
    write_note(&dir, "Text.md", "old old\n");
    for old in ["", "missing", "old"] {
        assert!(mutations.edit_note("Text.md", old, "new", false).is_err());
    }
    assert_eq!(read_note(&dir, "Text.md"), "old old\n");

    write_note(&dir, "Text.md", "banana\n");
    assert!(mutations.edit_note("Text.md", "ana", "x", false).is_err());
    assert_eq!(read_note(&dir, "Text.md"), "banana\n");
}

#[test]
fn edit_note_refuses_to_break_preserved_heading_and_block_links() {
    let (dir, mutations) = fixture();
    write_note(&dir, "Target.md", "# Old\n\nBody ^keep\n");
    write_note(&dir, "Source.md", "[[Target#Old]]\n[[Target#^keep]]\n");
    let heading = mutations
        .edit_note("Target.md", "# Old", "# New", false)
        .expect_err("heading link");
    assert!(heading.to_string().contains("[[Target#Old]]"));
    let block = mutations
        .edit_note("Target.md", "^keep", "^gone", false)
        .expect_err("block link");
    assert!(block.to_string().contains("[[Target#^keep]]"));
    assert_eq!(read_note(&dir, "Target.md"), "# Old\n\nBody ^keep\n");
}

#[test]
fn apply_patch_updates_multiple_notes_and_their_references_together() {
    let (dir, mutations) = fixture();
    write_note(&dir, "Target.md", "# Old\n\nBody\n");
    write_note(&dir, "Source.md", "[[Target#Old]]\n");
    let patch = "diff --git a/Target.md b/Target.md\n--- a/Target.md\n+++ b/Target.md\n@@ -1,3 +1,3 @@\n-# Old\n+# New\n \n Body\ndiff --git a/Source.md b/Source.md\n--- a/Source.md\n+++ b/Source.md\n@@ -1 +1 @@\n-[[Target#Old]]\n+[[Target#New]]\n";
    let preview = mutations
        .apply_patch(patch, true)
        .expect("preview both changes");
    assert!(preview.dry_run);
    assert_eq!(preview.changed_notes, ["Source.md", "Target.md"]);
    assert_eq!(read_note(&dir, "Target.md"), "# Old\n\nBody\n");
    assert_eq!(read_note(&dir, "Source.md"), "[[Target#Old]]\n");
    let result = mutations
        .apply_patch(patch, false)
        .expect("apply both changes");
    assert!(!result.dry_run);
    assert_eq!(result.changed_notes, ["Source.md", "Target.md"]);
    assert_eq!(read_note(&dir, "Target.md"), "# New\n\nBody\n");
    assert_eq!(read_note(&dir, "Source.md"), "[[Target#New]]\n");
}

#[test]
fn apply_patch_rejects_stale_hunks_and_broken_links_before_any_write() {
    let (dir, mutations) = fixture();
    write_note(&dir, "Target.md", "# Old\n\nBody\n");
    write_note(&dir, "Source.md", "[[Target#Old]]\n");
    let stale = "--- a/Target.md\n+++ b/Target.md\n@@ -1 +1 @@\n-# Wrong\n+# New\n";
    assert!(mutations.apply_patch(stale, false).is_err());
    let broken = "--- a/Target.md\n+++ b/Target.md\n@@ -1 +1 @@\n-# Old\n+# New\n";
    let error = mutations
        .apply_patch(broken, false)
        .expect_err("inbound reference");
    assert!(error.to_string().contains("[[Target#Old]]"));
    assert_eq!(read_note(&dir, "Target.md"), "# Old\n\nBody\n");

    let stale_second = "--- a/Target.md\n+++ b/Target.md\n@@ -1 +1 @@\n-# Old\n+# New\n--- a/Source.md\n+++ b/Source.md\n@@ -1 +1 @@\n-wrong\n+fixed\n";
    assert!(mutations.apply_patch(stale_second, false).is_err());
    assert_eq!(read_note(&dir, "Target.md"), "# Old\n\nBody\n");
}

#[test]
fn apply_patch_preserves_missing_final_newline_and_rejects_path_escape() {
    let (dir, mutations) = fixture();
    write_note(&dir, "Plain.md", "alpha");
    let patch = "--- a/Plain.md\n+++ b/Plain.md\n@@ -1 +1 @@\n-alpha\n\\ No newline at end of file\n+beta\n\\ No newline at end of file\n";
    mutations
        .apply_patch(patch, false)
        .expect("patch no-final-newline file");
    assert_eq!(read_note(&dir, "Plain.md"), "beta");
    let escape = "--- a/../outside.md\n+++ b/../outside.md\n@@ -1 +1 @@\n-old\n+new\n";
    assert!(mutations.apply_patch(escape, false).is_err());
}

#[test]
fn apply_patch_accepts_git_quoted_utf8_note_paths() {
    let (dir, mutations) = fixture();
    write_note(&dir, "中文.md", "old\n");
    let patch = "--- \"a/\\344\\270\\255\\346\\226\\207.md\"\n+++ \"b/\\344\\270\\255\\346\\226\\207.md\"\n@@ -1 +1 @@\n-old\n+new\n";
    mutations
        .apply_patch(patch, false)
        .expect("quoted UTF-8 path");
    assert_eq!(read_note(&dir, "中文.md"), "new\n");
}

#[test]
fn apply_patch_treats_three_dash_hunk_line_as_content() {
    let (dir, mutations) = fixture();
    write_note(&dir, "Plain.md", "-- note\n");
    let patch = "--- a/Plain.md\n+++ b/Plain.md\n@@ -1 +1 @@\n--- note\n+fixed\n";
    mutations
        .apply_patch(patch, false)
        .expect("replace line beginning with dashes");
    assert_eq!(read_note(&dir, "Plain.md"), "fixed\n");
}
