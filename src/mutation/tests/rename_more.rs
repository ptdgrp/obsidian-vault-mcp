use super::{fixture, read_note, write_note};

#[test]
fn set_block_id_replaces_id_without_deleting_block_content_and_updates_references() {
    let (dir, mutations) = fixture();
    write_note(&dir, "发动机.md", "# 发动机\n\n段落\n^state\n");
    write_note(
        &dir,
        "引用.md",
        "# 引用\n\n[[发动机.md#^state]]\n[[不存在#^state]]\n",
    );

    let preview = mutations
        .set_block_id("发动机.md", Some("state"), None, Some("status"), true)
        .expect("preview");
    assert!(preview.dry_run);
    assert_eq!(preview.updated_references, 1);
    assert_eq!(
        read_note(&dir, "引用.md"),
        "# 引用\n\n[[发动机.md#^state]]\n[[不存在#^state]]\n"
    );

    let applied = mutations
        .set_block_id("发动机.md", Some("state"), None, Some("status"), false)
        .expect("apply");
    assert!(!applied.dry_run);
    assert_eq!(applied.previous_block_id.as_deref(), Some("state"));
    assert_eq!(applied.block_id.as_deref(), Some("status"));
    assert_eq!(read_note(&dir, "发动机.md"), "# 发动机\n\n段落\n^status\n");
    assert_eq!(
        read_note(&dir, "引用.md"),
        "# 引用\n\n[[发动机.md#^status]]\n[[不存在#^state]]\n"
    );
}

#[test]
fn set_block_id_finds_unique_block_content_and_adds_an_id() {
    let (dir, mutations) = fixture();
    write_note(&dir, "块.md", "# 块\n\n目标段落\n\n其他段落\n");

    let result = mutations
        .set_block_id("块.md", None, Some("目标段落"), Some("target1"), false)
        .expect("set block id by content");

    assert_eq!(result.previous_block_id, None);
    assert_eq!(result.block_id.as_deref(), Some("target1"));
    assert_eq!(
        read_note(&dir, "块.md"),
        "# 块\n\n目标段落 ^target1\n\n其他段落\n"
    );
}

#[test]
fn set_block_id_reports_ambiguous_content_with_candidate_previews() {
    let (dir, mutations) = fixture();
    write_note(&dir, "块.md", "# 块\n\n重复内容 alpha\n\n重复内容 beta\n");

    let error = mutations
        .set_block_id("块.md", None, Some("重复内容"), Some("target1"), false)
        .expect_err("ambiguous content should fail");
    let message = error.to_string();

    assert!(message.contains("块.md#L3"), "{message}");
    assert!(message.contains("重复内容 alpha"), "{message}");
    assert!(message.contains("块.md#L5"), "{message}");
    assert!(message.contains("重复内容 beta"), "{message}");
}

#[test]
fn set_block_id_generates_a_lowercase_timx8_when_id_is_omitted() {
    let (dir, mutations) = fixture();
    write_note(&dir, "块.md", "# 块\n\n目标段落\n");

    let preview = mutations
        .set_block_id("块.md", None, Some("目标段落"), None, true)
        .expect("preview generated block id");
    let generated = preview.block_id.expect("generated block id");

    assert_eq!(generated.len(), 8);
    assert!(
        generated
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte.is_ascii_lowercase()),
        "{generated}"
    );
    assert_eq!(read_note(&dir, "块.md"), "# 块\n\n目标段落\n");

    mutations
        .set_block_id("块.md", None, Some("目标段落"), Some(&generated), false)
        .expect("apply generated block id");
    assert!(read_note(&dir, "块.md").contains(&format!("^{generated}")));
}

#[test]
fn set_block_id_rejects_explicit_ids_that_are_not_lowercase_obsidian_ids() {
    let (dir, mutations) = fixture();
    write_note(&dir, "块.md", "# 块\n\n目标段落\n");
    let before = read_note(&dir, "块.md");

    let error = mutations
        .set_block_id("块.md", None, Some("目标段落"), Some("Bad_ID"), false)
        .expect_err("uppercase and underscore should fail");

    assert!(error.to_string().contains("lowercase"));
    assert_eq!(read_note(&dir, "块.md"), before);
}

#[test]
fn set_block_id_refuses_deletion_and_lists_inbound_references() {
    let (dir, mutations) = fixture();
    write_note(&dir, "块.md", "# 块\n\n目标段落\n^state\n");
    write_note(&dir, "引用.md", "# 引用\n\n[[块.md#^state]]\n");
    let before_target = read_note(&dir, "块.md");
    let before_reference = read_note(&dir, "引用.md");

    let error = mutations
        .set_block_id("块.md", Some("state"), None, Some(""), false)
        .expect_err("referenced block id should not be deleted");
    let message = error.to_string();

    assert!(message.contains("引用.md#L3"), "{message}");
    assert!(message.contains("[[块.md#^state]]"), "{message}");
    assert_eq!(read_note(&dir, "块.md"), before_target);
    assert_eq!(read_note(&dir, "引用.md"), before_reference);
}

#[test]
fn set_block_id_deletes_an_unreferenced_id_and_preserves_block_content() {
    let (dir, mutations) = fixture();
    write_note(&dir, "块.md", "# 块\n\n目标段落\n^state\n");

    let result = mutations
        .set_block_id("块.md", Some("state"), None, Some(""), false)
        .expect("delete unreferenced block id");

    assert_eq!(result.previous_block_id.as_deref(), Some("state"));
    assert_eq!(result.block_id, None);
    assert_eq!(read_note(&dir, "块.md"), "# 块\n\n目标段落\n");
}

#[test]
fn set_block_id_rejects_deleting_from_a_block_without_an_id() {
    let (dir, mutations) = fixture();
    write_note(&dir, "块.md", "# 块\n\n目标段落\n");

    let error = mutations
        .set_block_id("块.md", None, Some("目标段落"), Some(""), false)
        .expect_err("block without id cannot delete id");

    assert!(error.to_string().contains("does not have a block id"));
    assert_eq!(read_note(&dir, "块.md"), "# 块\n\n目标段落\n");
}

#[test]
fn set_block_id_rejects_an_empty_content_selector() {
    let (dir, mutations) = fixture();
    write_note(&dir, "块.md", "# 块\n\n目标段落\n");

    let error = mutations
        .set_block_id("块.md", None, Some("  "), Some("target1"), false)
        .expect_err("empty content selector should fail");

    assert!(
        error
            .to_string()
            .contains("content selector must not be empty")
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
        .rename_note("推进器.md", "archive/推进器-新版.md", true)
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
fn rename_note_reports_complete_natural_changed_notes_and_excludes_target_move_from_reference_count()
 {
    let (_dir, mutations) = fixture();
    write_note(&_dir, "target.md", "# Target\n\n自引用 [[target.md]]\n");
    write_note(&_dir, "ref10.md", "# Ref 10\n\n[[target.md]]\n");
    write_note(&_dir, "ref2.md", "# Ref 2\n\n[[target.md]]\n");

    let preview = mutations
        .rename_note("target.md", "archive/target.md", true)
        .expect("preview");

    assert_eq!(preview.updated_references, 2);
    assert_eq!(
        preview.changed_notes,
        vec![
            "archive/target.md".to_string(),
            "ref2.md".to_string(),
            "ref10.md".to_string()
        ]
    );
    let value = serde_json::to_value(preview).expect("rename note json");
    assert!(value.get("note").is_none());
    assert!(value.get("new_path").is_none());
    assert!(value.get("old_path").is_none());
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
fn set_block_id_reports_missing_block_without_writing() {
    let (dir, mutations) = fixture();
    write_note(&dir, "块.md", "# 块\n");
    let before = read_note(&dir, "块.md");

    let error = mutations
        .set_block_id("块.md", Some("missing"), None, Some("status"), false)
        .expect_err("missing block should fail");

    assert!(error.to_string().contains("block not found: missing"));
    assert_eq!(read_note(&dir, "块.md"), before);
}
