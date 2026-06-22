use super::{fixture, read_note, write_note};

#[test]
fn rename_block_id_updates_resolved_references_after_preview() {
    let (dir, mutations) = fixture();
    write_note(&dir, "发动机.md", "# 发动机\n\n段落\n^state\n");
    write_note(
        &dir,
        "引用.md",
        "# 引用\n\n[[发动机.md#^state]]\n[[不存在#^state]]\n",
    );

    let preview = mutations
        .rename_block_id("发动机.md", "state", "status", true)
        .expect("preview");
    assert!(preview.dry_run);
    assert_eq!(preview.updated_references, 1);
    assert_eq!(
        read_note(&dir, "引用.md"),
        "# 引用\n\n[[发动机.md#^state]]\n[[不存在#^state]]\n"
    );

    let applied = mutations
        .rename_block_id("发动机.md", "state", "status", false)
        .expect("apply");
    assert!(!applied.dry_run);
    assert_eq!(read_note(&dir, "发动机.md"), "# 发动机\n\n^status\n");
    assert_eq!(
        read_note(&dir, "引用.md"),
        "# 引用\n\n[[发动机.md#^status]]\n[[不存在#^state]]\n"
    );
}

#[test]
fn rename_note_dry_run_reports_changes_without_writing_files() {
    let (dir, mutations) = fixture();
    write_note(&dir, "推进器.md", "# 推进器\n");
    write_note(&dir, "引用.md", "# 引用\n\n[[推进器]]\n");

    let before_target = read_note(&dir, "推进器.md");
    let before_reference = read_note(&dir, "引用.md");
    let preview = mutations
        .rename_note("推进器", "archive/推进器-新版.md", true)
        .expect("preview");

    assert!(preview.dry_run);
    assert_eq!(preview.updated_references, 1);
    assert_eq!(
        preview.changed_notes,
        vec!["archive/推进器-新版.md".to_string(), "引用.md".to_string()]
    );
    assert_eq!(read_note(&dir, "推进器.md"), before_target);
    assert_eq!(read_note(&dir, "引用.md"), before_reference);
}

#[test]
fn rename_note_rejects_existing_destination_before_writing() {
    let (dir, mutations) = fixture();
    write_note(&dir, "推进器.md", "# 推进器\n");
    write_note(&dir, "archive/推进器.md", "# 已存在\n");
    write_note(&dir, "引用.md", "# 引用\n\n[[推进器]]\n");
    let before_reference = read_note(&dir, "引用.md");

    let error = mutations
        .rename_note("推进器.md", "archive/推进器.md", false)
        .expect_err("existing destination should fail");

    assert!(
        error
            .to_string()
            .contains("destination note already exists")
    );
    assert!(dir.path().join("推进器.md").exists());
    assert_eq!(read_note(&dir, "引用.md"), before_reference);
}

#[test]
fn rename_heading_leaves_ambiguous_links_unchanged() {
    let (dir, mutations) = fixture();
    write_note(&dir, "资料/发动机.md", "# 备用发动机\n\n## 原理\n");
    write_note(&dir, "引用.md", "# 引用\n\n[[发动机#原理]]\n");

    let preview = mutations
        .rename_heading("发动机.md", "原理", "机制", true)
        .expect("preview");

    assert_eq!(preview.updated_references, 0);
    assert_eq!(read_note(&dir, "引用.md"), "# 引用\n\n[[发动机#原理]]\n");
}

#[test]
fn rename_heading_updates_self_and_multi_heading_links() {
    let (dir, mutations) = fixture();
    write_note(
        &dir,
        "发动机.md",
        "# 发动机\n\n## 原理\n\n自引用 [[#原理]]\n多级 [[发动机#章节#原理]]\n",
    );

    let applied = mutations
        .rename_heading("发动机.md", "原理", "机制", false)
        .expect("rename heading");

    assert_eq!(applied.updated_references, 2);
    let updated = read_note(&dir, "发动机.md");
    assert!(updated.contains("[[#机制]]"));
    assert!(updated.contains("[[发动机#章节#机制]]"));
}

#[test]
fn rename_block_id_reports_missing_block_without_writing() {
    let (dir, mutations) = fixture();
    write_note(&dir, "块.md", "# 块\n");
    let before = read_note(&dir, "块.md");

    let error = mutations
        .rename_block_id("块.md", "missing", "status", false)
        .expect_err("missing block should fail");

    assert!(error.to_string().contains("block id not found: missing"));
    assert_eq!(read_note(&dir, "块.md"), before);
}
