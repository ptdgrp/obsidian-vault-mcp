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
        "---\naliases:\n  - 动林\n---\n# 林动\n\n## 身体\n",
    );

    let output = run_cli(&dir, &["resolve-ref", "[[动林#身体]]"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(value, serde_json::json!({"target": "林动.md#身体"}));
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
fn list_notes_command_uses_request_filters_and_fixed_page_output() {
    let dir = tempdir().expect("tempdir");
    write_note(&dir, "正文/keep.md", "# 保留\n");
    write_note(&dir, "资料/skip.md", "# 跳过\n");

    let output = run_cli(
        &dir,
        &["list-notes", "--include", "正文/**/*.md", "--page", "1"],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(value["notes"][0]["path"], "正文/keep.md");
    assert_eq!(value["pagination"]["page"], 1);
    assert_eq!(value["pagination"]["total_notes"], 1);
    assert!(value["notes"][0].get("size").is_none());
}

#[test]
fn task_oriented_link_commands_return_compact_json() {
    let dir = tempdir().expect("tempdir");
    write_note(&dir, "甲.md", "# 甲\n\n[[缺失]]\n");
    write_note(&dir, "乙.md", "# 乙\n\n[[甲]]\n");

    let audit = run_cli(&dir, &["audit-links", "--page", "1"]);
    assert!(
        audit.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&audit.stderr)
    );
    let audit: Value = serde_json::from_slice(&audit.stdout).expect("audit json");
    assert_eq!(audit["unresolved"][0]["target"], "缺失");

    let neighborhood = run_cli(&dir, &["get-note-neighborhood", "甲", "--direction", "in"]);
    assert!(
        neighborhood.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&neighborhood.stderr)
    );
    let neighborhood: Value =
        serde_json::from_slice(&neighborhood.stdout).expect("neighborhood json");
    assert_eq!(neighborhood["center"]["path"], "甲.md");
    assert_eq!(neighborhood["notes"][0]["path"], "乙.md");
}

#[test]
fn get_note_outline_command_returns_selected_heading_ancestor_chain() {
    let dir = tempdir().expect("tempdir");
    write_note(
        &dir,
        "outline.md",
        "# Note title\n\n## Parent\n\n### Child\n\n#### Target\n\n### Other\n",
    );

    let output = run_cli(
        &dir,
        &[
            "get-note-outline",
            "outline.md",
            "--heading",
            "Parent/Child/Target",
        ],
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(value["outline"][0]["heading"], "Parent");
    assert_eq!(value["outline"][0]["children"][0]["heading"], "Child");
    assert_eq!(
        value["outline"][0]["children"][0]["children"][0]["heading"],
        "Target"
    );
    assert_eq!(value["outline"][0]["children"].as_array().unwrap().len(), 1);
}

#[test]
fn query_commands_apply_repeatable_request_path_filters() {
    let dir = tempdir().expect("tempdir");
    for path in ["正文/keep.md", "资料/keep.md", "正文/草稿/drop.md"] {
        write_note(
            &dir,
            path,
            "# Filtered\n\n#筛选标签\n\nshared filtered content\n",
        );
    }
    let filters = [
        "--include",
        "正文/**/*.md",
        "--include",
        "资料/**/*.md",
        "--exclude",
        "**/草稿/**",
    ];
    let assertions: [(&[&str], &str); 6] = [
        (&["list-tags"], "筛选标签"),
        (&["get-tags", "筛选标签"], "正文/keep.md"),
        (&["list-categories"], "正文"),
        (&["get-categories", "正文", "资料"], "正文/keep.md"),
        (&["search-text", "shared filtered content"], "正文/keep.md"),
        (&["search-regex", "shared filtered content"], "正文/keep.md"),
    ];

    for (command, expected) in assertions {
        let mut args = command.to_vec();
        args.extend(filters);
        let output = run_cli(&dir, &args);
        assert!(
            output.status.success(),
            "{command:?} stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).expect("json");
        let output = value.to_string();
        assert!(output.contains(expected), "{command:?}: {output}");
        assert!(
            !output.contains("正文/草稿/drop.md"),
            "{command:?}: {output}"
        );
    }

    let legacy = run_cli(
        &dir,
        &[
            "search-regex",
            "shared filtered content",
            "--path-glob",
            "正文/**/*.md",
        ],
    );
    assert!(!legacy.status.success());
    assert!(String::from_utf8_lossy(&legacy.stderr).contains("--path-glob"));
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

#[test]
fn read_note_accepts_explicit_selectors() {
    let dir = tempdir().expect("tempdir");
    write_note(
        &dir,
        "发动机.md",
        "# 发动机\n\n## 原理\n\n链接到 [[林动]]\n",
    );

    let output = run_cli(
        &dir,
        &[
            "read-note",
            "发动机",
            "--heading",
            "原理",
            "--max-chars",
            "4",
        ],
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(value["source"]["path"], "发动机.md#L3-L5");
    assert!(value["truncated"].as_bool().expect("truncated"));
}

#[test]
fn get_backlinks_accepts_repeatable_source_path_filters() {
    let dir = tempdir().expect("tempdir");
    write_note(&dir, "Target.md", "# Target\n");
    write_note(&dir, "来源/保留.md", "[[Target]]\n");
    write_note(&dir, "来源/排除.md", "[[Target]]\n");

    let output = run_cli(
        &dir,
        &[
            "get-backlinks",
            "Target",
            "--include",
            "来源/**/*.md",
            "--exclude",
            "**/排除.md",
            "--page",
            "1",
        ],
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(
        value,
        serde_json::json!({
            "scope": "Target.md",
            "references": [{
                "target": "Target.md",
                "sources": ["来源/保留.md#L1"]
            }],
            "pagination": {
                "page": 1,
                "total_pages": 1,
                "total_backlinks": 1
            }
        })
    );
}
