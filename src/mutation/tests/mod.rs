use std::fs;

use camino::Utf8PathBuf;
use tempfile::tempdir;

use crate::{
    mutation::VaultMutations,
    query::{SectionSelector, VaultQueries},
    vault::{Vault, VaultConfig},
};

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
fn rename_heading_updates_resolved_wikilink_references_after_preview() {
    let (dir, mutations) = fixture();
    fs::write(dir.path().join("引用.md"), "# 引用\n\n[[发动机.md#原理]]\n")
        .expect("write reference");
    let preview = mutations
        .rename_heading("发动机.md", "原理", "机制", true)
        .expect("preview");
    assert!(preview.dry_run);
    assert_eq!(preview.updated_references, 1);
    let applied = mutations
        .rename_heading("发动机.md", "原理", "机制", false)
        .expect("apply");
    assert!(!applied.dry_run);
    assert!(
        fs::read_to_string(dir.path().join("引用.md"))
            .expect("read")
            .contains("[[发动机.md#机制]]")
    );
}
