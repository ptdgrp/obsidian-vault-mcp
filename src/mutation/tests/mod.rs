use std::fs;

use camino::Utf8PathBuf;
use tempfile::tempdir;

use crate::{
    mutation::VaultMutations,
    query::{SectionSelector, VaultQueries},
    resolver::{RefResolver, ResolveResult},
    vault::{Vault, VaultConfig},
};

mod rename_more;

fn fixture() -> (tempfile::TempDir, VaultMutations) {
    let dir = tempdir().expect("tempdir");
    fs::write(
        dir.path().join("发动机.md"),
        "# 发动机\n\n## 原理\n\n链接到 [[林动]]\n",
    )
    .expect("write target note");
    fs::write(dir.path().join("河流.md"), "# 河流\n").expect("write linked note");
    let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path");
    let vault = Vault::open(root, VaultConfig::default()).expect("vault");
    (dir, VaultMutations::new(VaultQueries::new(vault)))
}

fn read_note(dir: &tempfile::TempDir, path: &str) -> String {
    fs::read_to_string(dir.path().join(path)).expect("read note")
}

fn write_note(dir: &tempfile::TempDir, path: &str, content: &str) {
    let full_path = dir.path().join(path);
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent).expect("create parents");
    }
    fs::write(full_path, content).expect("write note");
}

#[test]
fn section_edits_target_structural_boundaries_without_text_matching() {
    let (dir, mutations) = fixture();
    mutations
        .append_section(
            "发动机.md",
            SectionSelector::Heading {
                heading: "原理".to_string(),
            },
            "\n补充说明。\n",
        )
        .expect("append");
    mutations
        .replace_section(
            "发动机.md",
            SectionSelector::Heading {
                heading: "原理".to_string(),
            },
            "## 原理\n\n已整体替换。\n",
        )
        .expect("replace");
    mutations
        .delete_section(
            "发动机.md",
            SectionSelector::Heading {
                heading: "原理".to_string(),
            },
        )
        .expect("delete");
    assert_eq!(
        fs::read_to_string(dir.path().join("发动机.md")).expect("read"),
        "# 发动机\n\n"
    );
}

#[test]
fn section_edits_accept_markdown_heading_syntax() {
    let (dir, mutations) = fixture();

    mutations
        .replace_section(
            "发动机.md",
            SectionSelector::Heading {
                heading: "## 原理".to_string(),
            },
            "## 原理\n\n已整体替换。\n",
        )
        .expect("replace heading with markdown marker");

    assert_eq!(
        read_note(&dir, "发动机.md"),
        "# 发动机\n\n## 原理\n\n已整体替换。\n"
    );
}

#[test]
fn rename_heading_updates_resolved_wikilink_references_after_preview() {
    let (dir, mutations) = fixture();
    write_note(
        &dir,
        "引用.md",
        "# 引用\n\n[[发动机.md#原理]]\n[[发动机#原理|查看原理]]\n[[不存在#原理]]\n",
    );
    let before_target = read_note(&dir, "发动机.md");
    let before_reference = read_note(&dir, "引用.md");
    let preview = mutations
        .rename_heading("发动机.md", "原理", "机制", true)
        .expect("preview");
    assert!(preview.dry_run);
    assert_eq!(preview.updated_references, 2);
    assert_eq!(read_note(&dir, "发动机.md"), before_target);
    assert_eq!(read_note(&dir, "引用.md"), before_reference);
    let applied = mutations
        .rename_heading("发动机.md", "原理", "机制", false)
        .expect("apply");
    assert!(!applied.dry_run);
    assert_eq!(
        read_note(&dir, "发动机.md"),
        "# 发动机\n\n## 机制\n\n链接到 [[林动]]\n"
    );
    assert_eq!(
        read_note(&dir, "引用.md"),
        "# 引用\n\n[[发动机.md#机制]]\n[[发动机#机制|查看原理]]\n[[不存在#原理]]\n"
    );
}

#[test]
fn rename_heading_reports_missing_heading_without_changing_files() {
    let (dir, mutations) = fixture();
    let before_target = read_note(&dir, "发动机.md");

    let error = mutations
        .rename_heading("发动机.md", "不存在", "机制", false)
        .expect_err("missing heading should fail");

    assert!(error.to_string().contains("heading not found: 不存在"));
    assert_eq!(read_note(&dir, "发动机.md"), before_target);
}

#[test]
fn rename_note_updates_only_resolved_wikilinks_and_keeps_them_navigable() {
    let (dir, mutations) = fixture();
    write_note(&dir, "notes/推进器.md", "# 推进器\n\n## 原理\n");
    write_note(
        &dir,
        "引用.md",
        "# 引用\n\n[[notes/推进器.md]]\n[[推进器#原理|原理入口]]\n[[missing/推进器.md]]\n[外部](https://example.com/notes/推进器.md)\n",
    );

    let renamed = mutations
        .rename_note("notes/推进器.md", "archive/推进器-新版.md", false)
        .expect("rename note");

    assert_eq!(renamed.updated_references, 2);
    assert!(!dir.path().join("notes/推进器.md").exists());
    assert!(dir.path().join("archive/推进器-新版.md").exists());
    let updated_reference = read_note(&dir, "引用.md");
    assert_eq!(
        updated_reference,
        "# 引用\n\n[[archive/推进器-新版.md]]\n[[archive/推进器-新版.md#原理|原理入口]]\n[[missing/推进器.md]]\n[外部](https://example.com/notes/推进器.md)\n"
    );

    let vault = Vault::open(
        Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path"),
        VaultConfig::default(),
    )
    .expect("vault");
    let notes = VaultQueries::new(vault).index_notes().expect("index notes");
    for target in ["archive/推进器-新版.md", "archive/推进器-新版.md#原理"] {
        assert!(matches!(
            RefResolver::resolve(target, &notes),
            ResolveResult::Resolved { path, .. } if path == "archive/推进器-新版.md"
        ));
    }
}

#[test]
fn rename_note_rejects_paths_outside_vault_without_touching_sources() {
    let (dir, mutations) = fixture();
    write_note(&dir, "notes/推进器.md", "# 推进器\n");
    write_note(&dir, "引用.md", "# 引用\n\n[[notes/推进器.md]]\n");
    let before_reference = read_note(&dir, "引用.md");

    let error = mutations
        .rename_note("notes/推进器.md", "../vault-outside.md", false)
        .expect_err("path escape should fail");

    assert!(error.to_string().contains("path escapes vault root"));
    assert!(dir.path().join("notes/推进器.md").exists());
    assert_eq!(read_note(&dir, "引用.md"), before_reference);
}
