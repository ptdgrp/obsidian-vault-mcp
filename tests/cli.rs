use std::{fs, process::Command};

use serde_json::Value;
use tempfile::{TempDir, tempdir};

fn write_note(dir: &TempDir, path: &str, content: &str) {
    let full_path = dir.path().join(path);
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent).expect("create parents");
    }
    fs::write(full_path, content).expect("write note");
}

fn run_cli(dir: &TempDir, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_obsidian-vault-mcp"))
        .arg("--vault")
        .arg(dir.path())
        .args(args)
        .output()
        .expect("run cli")
}

#[test]
fn resolve_ref_command_returns_machine_readable_json() {
    let dir = tempdir().expect("tempdir");
    write_note(
        &dir,
        "林动.md",
        "---\naliases:\n  - 动林\n---\n# 林动\n\n身体\n",
    );

    let output = run_cli(&dir, &["resolve-ref", "[[动林#身体]]"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(value["status"], "resolved");
    assert_eq!(value["path"], "林动.md");
    assert_eq!(value["heading"], "身体");
}

#[test]
fn get_note_structure_command_returns_machine_readable_json() {
    let dir = tempdir().expect("tempdir");
    write_note(
        &dir,
        "发动机.md",
        "# 发动机\n\n## 原理\n\n链接到 [[林动]]\n",
    );

    let output = run_cli(&dir, &["get-note-structure", "发动机.md"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(value["path"], "发动机.md");
    assert_eq!(value["headings"][0]["text"], "发动机");
    assert_eq!(value["links"][0]["target"], "林动");
}

#[test]
fn rename_heading_command_applies_changes_when_dry_run_disabled() {
    let dir = tempdir().expect("tempdir");
    write_note(
        &dir,
        "发动机.md",
        "# 发动机\n\n## 原理\n\n链接到 [[林动]]\n",
    );
    write_note(&dir, "引用.md", "# 引用\n\n[[发动机.md#原理]]\n");
    write_note(&dir, "林动.md", "# 林动\n");

    let output = run_cli(
        &dir,
        &[
            "rename-heading",
            "发动机.md",
            "--old-heading",
            "原理",
            "--new-heading",
            "机制",
            "--dry-run=false",
        ],
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(value["dry_run"], false);
    assert_eq!(value["updated_references"], 1);
    assert!(
        fs::read_to_string(dir.path().join("发动机.md"))
            .expect("read target")
            .contains("## 机制")
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("引用.md")).expect("read reference"),
        "# 引用\n\n[[发动机.md#机制]]\n"
    );
}

#[test]
fn invalid_tag_scope_exits_with_clear_diagnostic() {
    let dir = tempdir().expect("tempdir");
    write_note(&dir, "林动.md", "# 林动\n");

    let output = run_cli(&dir, &["list-tags", "--scope", "wrong"]);

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(
            "invalid tag scope 'wrong'; expected note, frontmatter, body, section, or line"
        )
    );
}

#[test]
fn read_section_without_selector_lists_available_selectors() {
    let dir = tempdir().expect("tempdir");
    write_note(&dir, "块.md", "# 块\n\n段落\n^state\n");

    let output = run_cli(&dir, &["read-section", "块.md"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Available selectors in \"块.md\""));
    assert!(!stderr.contains("headings:"));
    assert!(stderr.contains("block_ids: \"^state\""));
}

#[test]
fn read_note_character_limit_uses_unicode_characters() {
    let dir = tempdir().expect("tempdir");
    write_note(&dir, "字符.md", "甲乙丙丁");

    let output = run_cli(
        &dir,
        &["--max-read-note-chars", "2", "read-note", "字符.md"],
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(value["content"], "甲乙");
    assert_eq!(value["truncated"], true);
}
