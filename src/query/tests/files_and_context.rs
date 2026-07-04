use super::fixture;
use crate::query::VaultFilesOptions;

#[test]
fn list_vault_files_can_return_attachments_only_and_truncation_counts() {
    let (_dir, queries) = fixture();

    let result = queries
        .list_vault_files(VaultFilesOptions {
            include_files: false,
            include_attachments: true,
            include_readme_outline: false,
            max_files: 1,
        })
        .expect("list attachments");

    assert_eq!(result.summary.notes, 6);
    assert_eq!(result.summary.attachments, 1);
    assert_eq!(result.files.len(), 1);
    assert_eq!(result.files[0].path, "地图.png");
    assert_eq!(
        result.files[0].kind,
        crate::query::VaultFileKind::Attachment
    );
    assert!(result.files[0].title.is_none());
    assert_eq!(result.truncated_files, 0);
}

#[test]
fn list_vault_files_can_include_readme_outline_and_limit_results() {
    let (_dir, queries) = fixture();

    let result = queries
        .list_vault_files(VaultFilesOptions {
            include_files: true,
            include_attachments: true,
            include_readme_outline: true,
            max_files: 100,
        })
        .expect("list limited files");

    assert_eq!(result.files.len(), 7);
    assert_eq!(result.truncated_files, 0);
    let readme = result
        .files
        .iter()
        .find(|file| file.path == "正文/README.md")
        .expect("readme file");
    assert_eq!(readme.title.as_deref(), Some("正文索引"));
    assert_eq!(readme.outline.as_ref(), Some(&Vec::<String>::new()));
    assert!(readme.modified.is_some());

    let limited = queries
        .list_vault_files(VaultFilesOptions {
            include_files: true,
            include_attachments: true,
            include_readme_outline: false,
            max_files: 2,
        })
        .expect("limited files");
    assert_eq!(limited.files.len(), 2);
    assert_eq!(limited.truncated_files, 5);
}

#[test]
fn collect_reference_context_returns_empty_groups_for_unresolved_reference() {
    let (_dir, queries) = fixture();

    let result = queries
        .collect_reference_context("[[缺失设定#不存在]]")
        .expect("collect unresolved context");

    assert_eq!(result.reference, "[[缺失设定#不存在]]");
    assert!(result.groups.is_empty());
    assert!(!result.truncated);
    assert_eq!(result.omitted_count, 0);
}

#[test]
fn collect_note_context_truncates_after_current_item_budget() {
    let (_dir, mut queries) = fixture();
    queries.vault.config.max_results = 1;

    let result = queries
        .collect_note_context("林动")
        .expect("collect note context");

    assert_eq!(result.groups.len(), 1);
    assert_eq!(result.groups[0].kind, "current");
    assert_eq!(result.groups[0].items.len(), 1);
    assert_eq!(result.groups[0].items[0].path, "林动.md");
    assert!(result.truncated);
    assert_eq!(result.omitted_count, 1);
}
