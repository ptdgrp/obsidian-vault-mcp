use std::{
    fs,
    io::{self, BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use tempfile::{TempDir, tempdir};

#[test]
fn generate_docs_and_check_run_without_a_vault() {
    let dir = tempdir().expect("tempdir");
    let output = dir.path().join("tools.md");
    let output = output.to_str().expect("UTF-8 temp path");

    let generated = run_raw_cli(&["generate-docs", "--output", output]);
    assert!(
        generated.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let tools = fs::read_to_string(output).expect("generated tools document");
    assert!(tools.contains("`read_note`"));

    let checked = run_raw_cli(&["generate-docs", "--check", "--output", output]);
    assert!(
        checked.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&checked.stderr)
    );
}

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

fn run_raw_cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_obsidian-vault-mcp"))
        .args(args)
        .output()
        .expect("run cli")
}

#[test]
fn vault_argument_expands_home_directory() {
    let home = tempdir().expect("temporary home");
    let vault = home.path().join("vault");
    fs::create_dir(&vault).expect("create vault");
    fs::write(vault.join("home-note.md"), "# Home note\n").expect("write note");

    let output = Command::new(env!("CARGO_BIN_EXE_obsidian-vault-mcp"))
        .env("HOME", home.path())
        .args(["--vault", "~/vault", "list-notes"])
        .output()
        .expect("run CLI with home-relative vault");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("list-notes JSON");
    assert_eq!(value["notes"][0]["path"], "home-note.md");
}

#[test]
fn omitted_vault_discovers_nearest_obsidian_ancestor() {
    let root = tempdir().expect("temporary root");
    fs::create_dir(root.path().join(".obsidian")).expect("create outer marker");
    fs::write(root.path().join("outer-note.md"), "# Outer\n").expect("write outer note");

    let inner = root.path().join("projects/inner-vault");
    let working_directory = inner.join("notes/drafts");
    fs::create_dir_all(inner.join(".obsidian")).expect("create inner marker");
    fs::create_dir_all(&working_directory).expect("create working directory");
    fs::write(inner.join("inner-note.md"), "# Inner\n").expect("write inner note");

    let output = Command::new(env!("CARGO_BIN_EXE_obsidian-vault-mcp"))
        .env_remove("OBSIDIAN_VAULT_MCP_ROOT")
        .current_dir(&working_directory)
        .arg("list-notes")
        .output()
        .expect("run CLI without an explicit vault");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("list-notes JSON");
    assert_eq!(value["notes"][0]["path"], "inner-note.md");
    assert_eq!(value["pagination"]["total_notes"], 1);
}

#[test]
fn explicit_vault_takes_precedence_over_discovery() {
    let root = tempdir().expect("temporary root");
    let discovered = root.path().join("discovered");
    fs::create_dir_all(discovered.join(".obsidian")).expect("create discovered marker");
    fs::write(discovered.join("discovered.md"), "# Discovered\n").expect("write discovered note");

    let explicit = root.path().join("explicit");
    fs::create_dir(&explicit).expect("create explicit vault");
    fs::write(explicit.join("explicit.md"), "# Explicit\n").expect("write explicit note");

    let output = Command::new(env!("CARGO_BIN_EXE_obsidian-vault-mcp"))
        .env_remove("OBSIDIAN_VAULT_MCP_ROOT")
        .current_dir(&discovered)
        .arg("--vault")
        .arg(&explicit)
        .arg("list-notes")
        .output()
        .expect("run CLI with explicit and discoverable vaults");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("list-notes JSON");
    assert_eq!(value["notes"][0]["path"], "explicit.md");
    assert_eq!(value["pagination"]["total_notes"], 1);
}

#[test]
fn omitted_vault_reports_discovery_start_when_none_exists() {
    let working_directory = tempdir().expect("working directory");

    let output = Command::new(env!("CARGO_BIN_EXE_obsidian-vault-mcp"))
        .env_remove("OBSIDIAN_VAULT_MCP_ROOT")
        .current_dir(working_directory.path())
        .arg("list-notes")
        .output()
        .expect("run CLI outside a vault");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no Obsidian vault found"),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains(working_directory.path().to_str().expect("UTF-8 temp path")),
        "stderr: {stderr}"
    );
}

const MCP_STDIO_TIMEOUT: Duration = Duration::from_secs(5);

struct McpStdioClient {
    child: Child,
    stdin: Option<ChildStdin>,
    responses: Receiver<io::Result<String>>,
    reader: Option<JoinHandle<()>>,
}

impl McpStdioClient {
    fn spawn(dir: &TempDir) -> Self {
        Self::spawn_with_args(dir, &["--log-level", "warn"])
    }

    fn spawn_with_args(dir: &TempDir, args: &[&str]) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_obsidian-vault-mcp"));
        command
            .arg("--vault")
            .arg(dir.path())
            .args(args)
            .arg("serve");
        Self::spawn_command(command)
    }

    fn spawn_without_vault(dir: &TempDir) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_obsidian-vault-mcp"));
        command
            .env_remove("OBSIDIAN_VAULT_MCP_ROOT")
            .current_dir(dir.path())
            .args(["--log-level", "warn", "serve"]);
        Self::spawn_command(command)
    }

    fn spawn_command(mut command: Command) -> Self {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn MCP server");
        let stdin = child.stdin.take().expect("MCP server stdin");
        let stdout = child.stdout.take().expect("MCP server stdout");
        let (sender, responses) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut stdout = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match stdout.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) if sender.send(Ok(line)).is_err() => break,
                    Ok(_) => {}
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        break;
                    }
                }
            }
        });

        Self {
            child,
            stdin: Some(stdin),
            responses,
            reader: Some(reader),
        }
    }

    fn request(&mut self, message: Value) -> Value {
        self.write(message);
        let line = match self.responses.recv_timeout(MCP_STDIO_TIMEOUT) {
            Ok(Ok(line)) => line,
            Ok(Err(error)) => panic!("read MCP response: {error}"),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                panic!("timed out waiting for MCP response after {MCP_STDIO_TIMEOUT:?}")
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                panic!("MCP server closed stdout before responding")
            }
        };
        serde_json::from_str(&line).expect("parse MCP response")
    }

    fn notification(&mut self, message: Value) {
        self.write(message);
    }

    fn write(&mut self, message: Value) {
        let stdin = self.stdin.as_mut().expect("MCP server stdin is open");
        serde_json::to_writer(&mut *stdin, &message).expect("serialize MCP request");
        stdin.write_all(b"\n").expect("terminate MCP request");
        stdin.flush().expect("flush MCP request");
    }

    fn shutdown(mut self) {
        drop(self.stdin.take());
        let deadline = Instant::now() + MCP_STDIO_TIMEOUT;
        let status = loop {
            match self.child.try_wait().expect("check MCP server status") {
                Some(_) => break self.child.wait().expect("wait for MCP server"),
                None if Instant::now() >= deadline => {
                    self.child.kill().expect("kill unresponsive MCP server");
                    let status = self.child.wait().expect("wait for killed MCP server");
                    panic!("MCP server did not exit after stdin closed: {status}");
                }
                None => thread::sleep(Duration::from_millis(10)),
            }
        };
        self.reader
            .take()
            .expect("MCP stdout reader")
            .join()
            .expect("join MCP stdout reader");
        assert!(
            status.success(),
            "MCP server exited unsuccessfully: {status}"
        );
    }
}

fn initialize_mcp(client: &mut McpStdioClient) -> Value {
    let initialize = client.request(serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {"name": "smoke", "version": "1"}
        }
    }));
    assert_eq!(initialize["id"], 1);
    client.notification(serde_json::json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    }));
    initialize
}

impl Drop for McpStdioClient {
    fn drop(&mut self) {
        drop(self.stdin.take());
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

#[test]
fn command_help_uses_mcp_task_descriptions() {
    for (command, description) in [
        (
            "list-notes",
            "Page through visible Markdown notes for lightweight navigation. Set limit between 1 and 100 to keep responses compact; it defaults to 100.",
        ),
        (
            "audit-links",
            "Audit unresolved and ambiguous local links across the visible vault.",
        ),
        (
            "get-note-neighborhood",
            "Return a bounded resolved-link neighborhood around one note reference.",
        ),
    ] {
        let help = run_raw_cli(&[command, "--help"]);
        assert!(
            help.status.success(),
            "{command} stderr: {}",
            String::from_utf8_lossy(&help.stderr)
        );
        assert!(
            String::from_utf8_lossy(&help.stdout).contains(description),
            "{command} help should contain {description:?}"
        );
    }
}

#[test]
fn cli_help_documents_debug_log_level() {
    let output = run_raw_cli(&["--help"]);
    assert!(
        output.status.success(),
        "help stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let help = String::from_utf8_lossy(&output.stdout);
    assert!(help.contains("--log-level <LOG_LEVEL>"), "help: {help}");
    assert!(help.contains("[default: debug]"), "help: {help}");
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
    assert_eq!(value["note"], "发动机.md");
    assert_eq!(value["link_count"], 1);
    assert_eq!(
        value["headings"],
        serde_json::json!([{"heading": "原理", "line": 3}])
    );
    assert!(value.get("links").is_none());
}

#[test]
fn list_notes_command_uses_request_filters_and_configurable_page_output() {
    let dir = tempdir().expect("tempdir");
    write_note(&dir, "正文/keep.md", "# 保留\n");
    write_note(&dir, "正文/next.md", "# 下一篇\n");
    write_note(&dir, "资料/skip.md", "# 跳过\n");

    let output = run_cli(
        &dir,
        &[
            "list-notes",
            "--include",
            "正文/**/*.md",
            "--page",
            "1",
            "--limit",
            "1",
        ],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(value["notes"][0]["path"], "正文/keep.md");
    assert_eq!(value["pagination"]["page"], 1);
    assert_eq!(value["pagination"]["total_notes"], 2);
    assert_eq!(value["pagination"]["total_pages"], 2);
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
fn get_note_outline_command_returns_flat_paged_heading_list() {
    let dir = tempdir().expect("tempdir");
    write_note(
        &dir,
        "outline.md",
        "# Note title\n\n## Parent\n\n### Child\n\n#### Target\n\n### Other\n",
    );

    let output = run_cli(&dir, &["get-note-outline", "outline.md", "--page", "1"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(
        value["headings"],
        serde_json::json!([
            {"heading": "Parent", "line": 3},
            {"heading": "Parent/Child", "line": 5},
            {"heading": "Parent/Child/Target", "line": 7},
            {"heading": "Parent/Other", "line": 9}
        ])
    );
    assert_eq!(
        value["pagination"],
        serde_json::json!({"page": 1, "total_pages": 1, "total_headings": 4})
    );
    assert!(value.get("outline").is_none());
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
        (&["get-tag", "筛选标签"], "正文/keep.md"),
        (&["list-categories"], "正文"),
        (&["get-category", "正文"], "正文/keep.md"),
        (&["search-text", "shared filtered content"], "正文/keep.md"),
        (&["search-regex", "shared filtered content"], "正文/keep.md"),
    ];

    for (command, expected) in assertions {
        let mut args = command.to_vec();
        args.extend(filters);
        args.extend(["--page", "1"]);
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
}

#[test]
fn compact_discovery_commands_return_paged_public_json() {
    let dir = tempdir().expect("tempdir");
    write_note(
        &dir,
        "正文/keep.md",
        "---\nphase: active\n---\n# Keep\n\nshared content #状态/身体\n",
    );
    write_note(
        &dir,
        "正文/other.md",
        "# Other\n\nshared content #状态/身体\n",
    );

    let search = run_cli(&dir, &["search-text", "shared content", "--page", "1"]);
    assert!(
        search.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&search.stderr)
    );
    let search: Value = serde_json::from_slice(&search.stdout).expect("search json");
    assert_eq!(search["matches"][0]["source"], "正文/keep.md#L6");
    assert_eq!(search["matches"][0]["preview"], "shared content #状态/身体");
    assert_eq!(search["pagination"]["total_matches"], 2);
    assert!(search.get("query").is_none());
    assert!(search["matches"][0].get("snippet").is_none());

    let tag = run_cli(
        &dir,
        &["get-tag", "状态/身体", "--scope", "line", "--page", "1"],
    );
    assert!(
        tag.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&tag.stderr)
    );
    let tag: Value = serde_json::from_slice(&tag.stdout).expect("tag json");
    assert_eq!(
        tag["matches"],
        serde_json::json!(["正文/keep.md#L6", "正文/other.md#L3"])
    );
    assert_eq!(tag["pagination"]["total_matches"], 2);
    assert!(tag.get("tags").is_none());

    let category = run_cli(&dir, &["get-category", "/正文/", "--page", "1"]);
    assert!(
        category.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&category.stderr)
    );
    let category: Value = serde_json::from_slice(&category.stdout).expect("category json");
    assert_eq!(
        category["notes"],
        serde_json::json!(["正文/keep.md", "正文/other.md"])
    );
    assert_eq!(category["pagination"]["total_notes"], 2);
    assert!(category.get("categories").is_none());

    let frontmatter = run_cli(
        &dir,
        &[
            "query-frontmatter",
            "phase",
            "--mode",
            "equals",
            "--value",
            "active",
            "--include",
            "正文/**/*.md",
            "--page",
            "1",
        ],
    );
    assert!(
        frontmatter.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&frontmatter.stderr)
    );
    let frontmatter: Value = serde_json::from_slice(&frontmatter.stdout).expect("frontmatter json");
    assert_eq!(frontmatter["notes"], serde_json::json!(["正文/keep.md"]));
    assert_eq!(frontmatter["pagination"]["total_notes"], 1);
    assert!(frontmatter.get("matches").is_none());
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
fn rename_note_command_requires_exact_markdown_paths() {
    let dir = tempdir().expect("tempdir");
    write_note(&dir, "推进器.md", "# 推进器\n");
    write_note(&dir, "引用.md", "# 引用\n\n[[推进器.md]]\n");
    let before_target = fs::read_to_string(dir.path().join("推进器.md")).expect("read target");
    let before_reference = fs::read_to_string(dir.path().join("引用.md")).expect("read ref");

    let missing_extension = run_cli(
        &dir,
        &[
            "rename-note",
            "推进器",
            "archive/推进器.md",
            "--dry-run=false",
        ],
    );

    assert!(!missing_extension.status.success());
    assert!(
        String::from_utf8_lossy(&missing_extension.stderr)
            .contains("exact note path must end with .md")
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("推进器.md")).expect("read target"),
        before_target
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("引用.md")).expect("read ref"),
        before_reference
    );

    let output = run_cli(
        &dir,
        &[
            "rename-note",
            "推进器.md",
            "archive/推进器.md",
            "--dry-run=false",
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
            "dry_run": false,
            "updated_references": 1,
            "changed_notes": ["archive/推进器.md", "引用.md"]
        })
    );
}

#[test]
fn create_and_delete_note_commands_use_exact_paths_and_preview_deletion() {
    let dir = tempdir().expect("tempdir");
    let created = run_cli(&dir, &["create-note", "drafts/New.md", "# New\n"]);
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("drafts/New.md")).expect("created note"),
        "# New\n"
    );
    let preview = run_cli(&dir, &["delete-note", "drafts/New.md"]);
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    let value: Value = serde_json::from_slice(&preview.stdout).expect("preview json");
    assert_eq!(value["dry_run"], true);
    assert!(dir.path().join("drafts/New.md").exists());
    let applied = run_cli(&dir, &["delete-note", "drafts/New.md", "--dry-run=false"]);
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    assert!(!dir.path().join("drafts/New.md").exists());
}

#[test]
fn edit_note_and_apply_patch_commands_update_existing_notes() {
    let dir = tempdir().expect("tempdir");
    write_note(&dir, "Text.md", "# Text\n\nold\n");
    let preview = run_cli(&dir, &["edit-note", "Text.md", "old", "new"]);
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    let preview_value: Value = serde_json::from_slice(&preview.stdout).expect("preview json");
    assert_eq!(preview_value["dry_run"], true);
    assert_eq!(
        fs::read_to_string(dir.path().join("Text.md")).unwrap(),
        "# Text\n\nold\n"
    );
    let edited = run_cli(
        &dir,
        &["edit-note", "Text.md", "old", "new", "--dry-run=false"],
    );
    assert!(
        edited.status.success(),
        "{}",
        String::from_utf8_lossy(&edited.stderr)
    );
    let patch = "--- a/Text.md\n+++ b/Text.md\n@@ -3 +3 @@\n-new\n+patched\n";
    let patch_preview = run_cli(&dir, &["apply-patch", patch]);
    assert!(
        patch_preview.status.success(),
        "{}",
        String::from_utf8_lossy(&patch_preview.stderr)
    );
    let patch_preview_value: Value =
        serde_json::from_slice(&patch_preview.stdout).expect("patch preview json");
    assert_eq!(patch_preview_value["dry_run"], true);
    assert_eq!(
        fs::read_to_string(dir.path().join("Text.md")).unwrap(),
        "# Text\n\nnew\n"
    );
    let patched = run_cli(&dir, &["apply-patch", patch, "--dry-run=false"]);
    assert!(
        patched.status.success(),
        "{}",
        String::from_utf8_lossy(&patched.stderr)
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("Text.md")).unwrap(),
        "# Text\n\npatched\n"
    );
}

#[test]
fn edit_note_replace_all_flag_updates_repeated_text() {
    let dir = tempdir().expect("tempdir");
    write_note(&dir, "Text.md", "old old\n");
    let rejected = run_cli(
        &dir,
        &["edit-note", "Text.md", "old", "new", "--dry-run=false"],
    );
    assert!(!rejected.status.success());
    assert_eq!(
        fs::read_to_string(dir.path().join("Text.md")).unwrap(),
        "old old\n"
    );
    let edited = run_cli(
        &dir,
        &[
            "edit-note",
            "Text.md",
            "old",
            "new",
            "--replace-all",
            "--dry-run=false",
        ],
    );
    assert!(
        edited.status.success(),
        "{}",
        String::from_utf8_lossy(&edited.stderr)
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("Text.md")).unwrap(),
        "new new\n"
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
    write_note(&dir, "字符.md", "甲乙\n丙丁\n戊己");

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
    assert_eq!(value["content"], "甲乙\n");
    assert_eq!(value["truncated"], true);
    assert_eq!(value["returned_source"], "字符.md#L1");
    assert_eq!(value["next_line"], 2);
    assert!(value.get("path").is_none());
    assert!(value.get("next_step").is_none());
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
    assert_eq!(value["source"], "发动机.md#L3-L5");
    assert!(value["truncated"].as_bool().expect("truncated"));
    assert!(value.get("path").is_none());
    assert!(value.get("next_step").is_none());
}

#[test]
fn read_note_cli_preserves_invalid_line_format_error() {
    let dir = tempdir().expect("tempdir");
    write_note(&dir, "发动机.md", "# 发动机\n");

    let output = run_cli(&dir, &["read-note", "发动机", "--line", "L3-invalid"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("line must use"), "stderr: {stderr}");
    assert!(
        !stderr.contains("provide exactly one selector"),
        "stderr: {stderr}"
    );
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

#[test]
fn mcp_stdio_initialize_lists_tools_and_calls_read_note() {
    let dir = tempdir().expect("tempdir");
    write_note(&dir, "smoke.md", "# Smoke\n\nready\n");

    let mut client = McpStdioClient::spawn(&dir);
    initialize_mcp(&mut client);

    let tools = client.request(serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    }));
    assert_eq!(tools["id"], 2);
    assert!(
        tools["result"]["tools"]
            .as_array()
            .expect("tools array")
            .iter()
            .any(|tool| tool["name"] == "read_note")
    );

    let read_note = client.request(serde_json::json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {"name": "read_note", "arguments": {"note": "smoke.md"}}
    }));
    assert_eq!(read_note["id"], 3);
    let structured = &read_note["result"]["structuredContent"];
    assert!(
        structured["source"]
            .as_str()
            .expect("read source")
            .contains("smoke.md")
    );
    assert!(
        structured["content"]
            .as_str()
            .expect("read content")
            .contains("ready")
    );

    client.shutdown();
}

#[test]
fn mcp_server_without_discovered_vault_exposes_project_markdown_tools() {
    let working_directory = tempdir().expect("working directory");
    write_note(
        &working_directory,
        "docs/nested/guide.md",
        "# Guide\n\nProject-local note\n\n[Missing](../../missing.md)\n\n[[Missing wiki]]\n",
    );
    let mut client = McpStdioClient::spawn_without_vault(&working_directory);

    let initialize = initialize_mcp(&mut client);
    let instructions = initialize["result"]["instructions"]
        .as_str()
        .expect("project server instructions");
    assert!(instructions.contains("No Obsidian vault was found"));
    assert!(instructions.contains("project-relative paths"));
    assert!(initialize["result"]["capabilities"]["tools"].is_object());

    let tools = client.request(serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    }));
    let names = tools["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool name"))
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            "append_section",
            "apply_patch",
            "audit_links",
            "create_note",
            "delete_note",
            "delete_section",
            "edit_history",
            "edit_note",
            "get_note_outline",
            "get_note_stats",
            "get_note_structure",
            "list_files",
            #[cfg(feature = "attachments")]
            "read_attachment",
            "read_canvas",
            "read_note",
            "redo_edit",
            "replace_section",
            "set_frontmatter_field",
            "undo_edit",
        ]
    );

    let read_note = client.request(serde_json::json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {"name": "read_note", "arguments": {"note": "docs/nested/guide.md"}}
    }));
    assert_eq!(read_note["id"], 3);
    assert_eq!(
        read_note["result"]["structuredContent"]["content"],
        "# Guide\n\nProject-local note\n\n[Missing](../../missing.md)\n\n[[Missing wiki]]\n"
    );

    let audit_links = client.request(serde_json::json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": {"name": "audit_links", "arguments": {}}
    }));
    assert_eq!(audit_links["id"], 4);
    assert_eq!(
        audit_links["result"]["structuredContent"]["totals"]["unresolved"],
        1
    );
    assert_eq!(
        audit_links["result"]["structuredContent"]["unresolved"][0]["target"],
        "missing.md"
    );

    client.shutdown();
}

#[test]
fn explicit_missing_vault_still_fails_to_start_server() {
    let working_directory = tempdir().expect("working directory");
    let missing = working_directory.path().join("missing-vault");
    let output = Command::new(env!("CARGO_BIN_EXE_obsidian-vault-mcp"))
        .arg("--vault")
        .arg(&missing)
        .arg("serve")
        .output()
        .expect("run MCP server with explicit missing vault");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("vault root is not a directory"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn semantic_readers_cli_preserve_raw_reads_and_discovery_contracts() {
    let dir = tempdir().unwrap();
    write_note(&dir, "record.md", "---\nscore: 7\n---\n# Record\n");
    write_note(
        &dir,
        "records.base",
        "filters: 'file.ext == \"md\"'\nviews: [{type: table, order: [score]}]\n",
    );
    write_note(
        &dir,
        "graph.canvas",
        r#"{"nodes":[{"id":"text","type":"text","x":0,"y":0,"width":100,"height":100,"text":"完整内容"}],"edges":[]}"#,
    );
    let output = run_cli(&dir, &["read-base", "records.base", "--max-chars", "1"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let base: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        base.as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>(),
        [
            "content",
            "output_format",
            "pagination",
            "path",
            "view",
            "view_type"
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(base["pagination"], json!({"page":1,"total_pages":1}));
    assert!(
        base["content"]
            .as_str()
            .unwrap()
            .contains("| record.md | 7 |")
    );
    assert!(base["content"].as_str().unwrap().contains("| 7 |"));
    assert!(base.get("source").is_none());
    let output = run_cli(&dir, &["read-canvas", "graph.canvas"]);
    assert!(output.status.success());
    let canvas: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(canvas["content"].as_str().unwrap().contains("完整内容"));
    let output = run_cli(
        &dir,
        &["list-files", "--extension", "base", "--extension", "canvas"],
    );
    let files: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(files["pagination"]["total_files"], 2);
    let output = run_cli(&dir, &["list-notes"]);
    let notes: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(notes["pagination"]["total_notes"], 1);
    let output = run_cli(&dir, &["read-note", "record.md"]);
    let note: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(note["content"], "---\nscore: 7\n---\n# Record\n");
}

#[test]
fn deep_formulas_fail_safely_in_cli() {
    let dir = tempdir().unwrap();
    write_note(&dir, "record.md", "# Record\n");
    write_note(
        &dir,
        "deep.base",
        &format!(
            "filters: 'false'\nformulas:\n  deep: '1{}'\nviews: [{{type: table}}]",
            ".abs()".repeat(12000)
        ),
    );
    let output = run_cli(&dir, &["read-base", "deep.base"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("depth exceeds"));
}

#[test]
fn semantic_readers_stdio_in_vault_and_project_modes() {
    for project in [false, true] {
        let dir = tempdir().unwrap();
        write_note(&dir, "graph.canvas", "{}");
        write_note(&dir, "records.base", "views: [{type: table}]\n");
        let mut client = if project {
            McpStdioClient::spawn_without_vault(&dir)
        } else {
            McpStdioClient::spawn(&dir)
        };
        initialize_mcp(&mut client);
        for (id, name, arguments) in [
            (2, "read_canvas", serde_json::json!({"path":"graph.canvas"})),
            (
                3,
                "list_files",
                serde_json::json!({"extensions":["canvas"]}),
            ),
        ] {
            let reply=client.request(serde_json::json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}}));
            assert!(reply["result"]["structuredContent"].is_object(), "{reply}");
            assert_ne!(reply["result"]["isError"], true);
        }
        if !project {
            let reply=client.request(serde_json::json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"read_base","arguments":{"path":"records.base"}}}));
            assert!(reply["result"]["structuredContent"]["content"].is_string());
            assert_eq!(
                reply["result"]["structuredContent"]
                    .as_object()
                    .unwrap()
                    .len(),
                6
            );
        }
        client.shutdown();
    }
}

#[test]
#[cfg(unix)]
fn calendar_days_preserve_local_hour_across_dst() {
    let dir = tempdir().unwrap();
    write_note(&dir, "record.md", "# Record\n");
    for (start, duration, expected) in [
        ("2026-03-07 12:00:00", "1d", "2026-03-08 12:00"),
        ("2026-03-07 12:00:00", "24h", "2026-03-08 13:00"),
        ("2026-10-31 12:00:00", "1d", "2026-11-01 12:00"),
        ("2026-10-31 12:00:00", "24h", "2026-11-01 11:00"),
    ] {
        write_note(
            &dir,
            "dates.base",
            &format!(
                "filters: 'file.ext == \"md\"'\nformulas:\n  result: '(date(\"{start}\") + \"{duration}\").format(\"YYYY-MM-DD HH:mm\")'\nviews: [{{type: table, order: [formula.result]}}]"
            ),
        );
        let output = Command::new(env!("CARGO_BIN_EXE_obsidian-vault-mcp"))
            .env("TZ", "America/Los_Angeles")
            .arg("--vault")
            .arg(dir.path())
            .args(["read-base", "dates.base"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let base: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(
            base["content"].as_str().unwrap().contains(expected),
            "{}",
            base["content"]
        );
    }
}

fn run_cli_from(cwd: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_obsidian-vault-mcp"))
        .env_remove("OBSIDIAN_VAULT_MCP_ROOT")
        .current_dir(cwd)
        .args(["--log-level", "warn"])
        .args(args)
        .output()
        .unwrap()
}

fn successful_json(output: std::process::Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn local_markdown_cli_reads_work_without_a_vault_and_do_not_enable_vault_queries() {
    let dir = tempdir().unwrap();
    write_note(
        &dir,
        "README.md",
        "# Readme\n\n## Local\n\nproject evidence\n",
    );
    for command in [
        "read-note",
        "get-note-outline",
        "get-note-structure",
        "get-note-stats",
    ] {
        let result = successful_json(run_cli_from(dir.path(), &[command, "./README.md"]));
        if command == "read-note" {
            assert!(
                result["content"]
                    .as_str()
                    .unwrap()
                    .contains("project evidence")
            );
        } else {
            assert!(result.to_string().contains("README.md"));
        }
    }
    for args in [
        vec!["list-notes"],
        vec!["search-text", "evidence"],
        vec!["read-base", "./records.base"],
        vec!["audit-links"],
        vec!["create-note", "./new.md", "body"],
    ] {
        let output = run_cli_from(dir.path(), &args);
        assert!(!output.status.success(), "{args:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("no Obsidian vault found"));
    }
    let output = run_cli_from(
        dir.path(),
        &["--vault", "./missing", "read-note", "./README.md"],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("vault root is not a directory"));
    let help = run_cli_from(dir.path(), &["--help"]);
    assert!(String::from_utf8_lossy(&help.stdout).contains("process working directory"));
}

#[test]
fn cwd_note_paths_preserve_selectors_and_bare_vault_paths() {
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join(".obsidian")).unwrap();
    write_note(
        &dir,
        "deep/work/local.md",
        "# Local file\n\n## Local\n\nlocal evidence\n\nblock evidence ^block\n",
    );
    write_note(
        &dir,
        "deep/sibling.md",
        "# Sibling\n\nsibling evidence ^block\n",
    );
    write_note(&dir, "local.md", "# Root file\nroot evidence\n");
    let cwd = dir.path().join("deep/work");
    for (input, path, text) in [
        ("./local", "deep/work/local.md", "local evidence"),
        ("../sibling.md", "deep/sibling.md", "sibling evidence"),
        ("local.md", "local.md", "root evidence"),
        ("deep/work/local.md", "deep/work/local.md", "local evidence"),
    ] {
        let result = successful_json(run_cli_from(&cwd, &["read-note", input]));
        assert!(result["source"].as_str().unwrap().starts_with(path));
        assert!(result["content"].as_str().unwrap().contains(text));
    }
    for input in [
        "[[./local#Local|display]]",
        "![[./local.md#^block|display]]",
        "./local.md#L3-L5",
    ] {
        let result = successful_json(run_cli_from(&cwd, &["read-note", input]));
        assert!(
            result["source"]
                .as_str()
                .unwrap()
                .starts_with("deep/work/local.md#L")
        );
        assert!(!result["content"].as_str().unwrap().contains("# Local file"));
    }
    let result = successful_json(run_cli_from(
        &cwd,
        &["read-note", "./local.md", "--heading", "Local"],
    ));
    assert!(
        result["content"]
            .as_str()
            .unwrap()
            .contains("local evidence")
    );
    let result = successful_json(run_cli_from(
        &cwd,
        &["resolve-ref", "[[../sibling#^block|display]]"],
    ));
    assert!(result.to_string().contains("deep/sibling.md"));
    for command in [
        "get-note-outline",
        "get-note-structure",
        "get-note-stats",
        "get-outlinks",
        "get-backlinks",
        "get-note-neighborhood",
    ] {
        let result = successful_json(run_cli_from(&cwd, &[command, "./local.md"]));
        assert!(
            result.to_string().contains("deep/work/local.md"),
            "{command}: {result}"
        );
    }
}

#[test]
fn explicit_missing_and_escaping_cwd_paths_never_fall_back_to_other_notes() {
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join(".obsidian")).unwrap();
    fs::create_dir_all(dir.path().join("deep/work")).unwrap();
    write_note(&dir, "elsewhere/missing.md", "# Wrong file\n");
    let cwd = dir.path().join("deep/work");
    for command in [
        "read-note",
        "get-note-outline",
        "get-note-structure",
        "get-note-stats",
        "resolve-ref",
        "get-outlinks",
        "get-backlinks",
        "get-note-neighborhood",
    ] {
        let output = run_cli_from(&cwd, &[command, "./missing.md"]);
        assert!(!output.status.success(), "{command}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("CLI path does not exist"));
    }
    for command in ["read-note", "get-note-outline", "create-note"] {
        let mut args = vec![command, "../../../outside.md"];
        if command == "create-note" {
            args.push("body");
        }
        let output = run_cli_from(&cwd, &args);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("escapes selected root"));
    }
}

#[test]
fn configured_vault_relative_roots_and_environment_precedence_are_preserved() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("discovered/.obsidian")).unwrap();
    fs::create_dir_all(dir.path().join("chosen/deep/work")).unwrap();
    write_note(&dir, "chosen/deep/work/local.md", "# Chosen\n");
    write_note(&dir, "chosen/root.md", "# Chosen root\n");
    write_note(&dir, "discovered/root.md", "# Discovered root\n");
    let cwd = dir.path().join("chosen/deep/work");
    for root in ["../..", "../.././deep/.."] {
        let result = successful_json(run_cli_from(
            &cwd,
            &["--vault", root, "read-note", "./local.md"],
        ));
        assert_eq!(result["content"], "# Chosen\n");
        let result = successful_json(run_cli_from(
            &cwd,
            &["--vault", root, "read-note", "root.md"],
        ));
        assert_eq!(result["content"], "# Chosen root\n");
    }
    for (explicit, env_root, expected) in [
        (None, "../chosen", "# Chosen root\n"),
        (Some("../chosen"), "./missing", "# Chosen root\n"),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_obsidian-vault-mcp"));
        command
            .current_dir(dir.path().join("discovered"))
            .env("OBSIDIAN_VAULT_MCP_ROOT", env_root);
        if let Some(root) = explicit {
            command.args(["--vault", root]);
        }
        let result = successful_json(command.args(["read-note", "root.md"]).output().unwrap());
        assert_eq!(result["content"], expected);
    }
    let output = Command::new(env!("CARGO_BIN_EXE_obsidian-vault-mcp"))
        .current_dir(dir.path().join("discovered"))
        .env("OBSIDIAN_VAULT_MCP_ROOT", "./missing")
        .args(["read-note", "root.md"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("vault root is not a directory"));
}

#[test]
fn exact_cli_paths_keep_filename_punctuation_and_edit_destinations_use_cwd() {
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join(".obsidian")).unwrap();
    write_note(&dir, "deep/work/host#name.md", "# Host\n");
    write_note(&dir, "deep/work/graph#name.canvas", "{}");
    write_note(
        &dir,
        "deep/work/records#name.base",
        "filters: 'file == this.file'\nviews: [{type: table, order: [file.path]}]\n",
    );
    let cwd = dir.path().join("deep/work");
    let result = successful_json(run_cli_from(
        &cwd,
        &[
            "read-base",
            "./records#name.base",
            "--context-note",
            "./host#name.md",
        ],
    ));
    assert!(
        result["content"]
            .as_str()
            .unwrap()
            .contains("| deep/work/host#name.md |")
    );
    let result = successful_json(run_cli_from(&cwd, &["read-canvas", "./graph#name.canvas"]));
    assert_eq!(result["path"], "deep/work/graph#name.canvas");
    let body = "./literal content ../unchanged";
    successful_json(run_cli_from(&cwd, &["create-note", "./created.md", body]));
    assert_eq!(fs::read_to_string(cwd.join("created.md")).unwrap(), body);
    successful_json(run_cli_from(
        &cwd,
        &[
            "edit-note",
            "./created.md",
            "./literal",
            "../literal",
            "--dry-run",
            "false",
        ],
    ));
    assert_eq!(
        fs::read_to_string(cwd.join("created.md")).unwrap(),
        "../literal content ../unchanged"
    );
    successful_json(run_cli_from(
        &cwd,
        &[
            "rename-note",
            "./created.md",
            "../moved.md",
            "--dry-run",
            "false",
        ],
    ));
    assert!(!cwd.join("created.md").exists());
    assert!(dir.path().join("deep/moved.md").exists());
    successful_json(run_cli_from(
        &cwd,
        &["delete-note", "../moved.md", "--dry-run", "false"],
    ));
    assert!(!dir.path().join("deep/moved.md").exists());
    #[cfg(unix)]
    {
        write_note(&dir, "deep/work/graph|name.canvas", "{}");
        let result = successful_json(run_cli_from(&cwd, &["read-canvas", "./graph|name.canvas"]));
        assert_eq!(result["path"], "deep/work/graph|name.canvas");
    }
}

#[test]
#[cfg(unix)]
fn cwd_cli_paths_reject_symlink_escape_for_reads_and_new_destinations() {
    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::create_dir(dir.path().join(".obsidian")).unwrap();
    write_note(&dir, "inside.md", "# Inside\n");
    write_note(&outside, "outside.md", "# Outside\n");
    std::os::unix::fs::symlink(outside.path(), dir.path().join("escape")).unwrap();
    for args in [
        vec!["read-note", "./escape/outside.md"],
        vec!["get-note-outline", "./escape/outside.md"],
        vec!["create-note", "./escape/new.md", "body"],
        vec![
            "rename-note",
            "./inside.md",
            "./escape/new.md",
            "--dry-run",
            "false",
        ],
    ] {
        let output = run_cli_from(dir.path(), &args);
        assert!(!output.status.success(), "{args:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("escapes selected root through a symlink")
        );
    }
    assert!(!outside.path().join("new.md").exists());
    assert!(dir.path().join("inside.md").exists());
}

#[test]
fn mcp_dot_paths_remain_vault_relative_when_server_cwd_is_a_subdirectory() {
    let dir = tempdir().unwrap();
    write_note(&dir, "local.md", "# Vault root\n");
    write_note(&dir, "deep/local.md", "# Process cwd\n");
    let mut command = Command::new(env!("CARGO_BIN_EXE_obsidian-vault-mcp"));
    command
        .current_dir(dir.path().join("deep"))
        .arg("--vault")
        .arg(dir.path())
        .args(["--log-level", "warn", "serve"]);
    let mut client = McpStdioClient::spawn_command(command);
    initialize_mcp(&mut client);
    let reply=client.request(serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"read_note","arguments":{"note":"./local.md"}}}));
    assert_eq!(
        reply["result"]["structuredContent"]["content"],
        "# Vault root\n"
    );
    client.shutdown();
}

#[test]
fn frontmatter_and_base_view_cli_support_exact_dot_paths_and_typed_values() {
    let dir = tempdir().unwrap();
    write_note(&dir, "nested/item.md", "---\nscore: 7\n---\n# Body\n");
    write_note(
        &dir,
        "nested/views.base",
        "filters: 'file.ext == \"md\"'\nviews:\n  - {type: table, name: First, order: [score]}\n  - {type: table, name: Second, order: [file.path]}\n",
    );
    let invoke = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_obsidian-vault-mcp"))
            .arg("--vault")
            .arg(dir.path())
            .current_dir(dir.path().join("nested"))
            .args(args)
            .output()
            .unwrap()
    };
    let preview = invoke(&["set-frontmatter-field", "./item.md", "literal.dot", "null"]);
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    let value: Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(value["path"], "nested/item.md");
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["previous_exists"], false);
    assert!(
        !fs::read_to_string(dir.path().join("nested/item.md"))
            .unwrap()
            .contains("literal.dot")
    );
    let apply = invoke(&[
        "set-frontmatter-field",
        "./item.md",
        "literal.dot",
        "[1,true,null]",
        "--dry-run",
        "false",
    ]);
    assert!(
        apply.status.success(),
        "{}",
        String::from_utf8_lossy(&apply.stderr)
    );
    let source = fs::read_to_string(dir.path().join("nested/item.md")).unwrap();
    let yaml = source
        .strip_prefix("---\n")
        .unwrap()
        .split("---\n")
        .next()
        .unwrap();
    let properties: Value = serde_yaml_ng::from_str(yaml).unwrap();
    assert_eq!(properties["literal.dot"], json!([1, true, null]));
    assert!(source.ends_with("---\n# Body\n"));
    assert!(
        !invoke(&["set-frontmatter-field", "./item.md", "score", "unquoted"])
            .status
            .success()
    );
    assert!(
        !invoke(&["set-frontmatter-field", "item", "score", "0"])
            .status
            .success()
    );
    for number in ["-1", "-1.25", "-1.25e-10"] {
        for apply in [false, true] {
            let mut args = vec!["set-frontmatter-field", "./item.md", "score", number];
            if apply {
                args.extend(["--dry-run", "false"]);
            }
            let output = invoke(&args);
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let result: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(result["dry_run"], !apply);
            assert_eq!(
                result["result_value"],
                serde_json::from_str::<Value>(number).unwrap()
            );
        }
    }
    // Restore the score used to check the two view selections below.
    assert!(
        invoke(&[
            "set-frontmatter-field",
            "./item.md",
            "score",
            "7",
            "--dry-run",
            "false"
        ])
        .status
        .success()
    );
    let discovered = invoke(&["query-base-views", "./views.base"]);
    assert!(discovered.status.success());
    let views: Value = serde_json::from_slice(&discovered.stdout).unwrap();
    assert_eq!(views["views"][1]["name"], "Second");
    let first = invoke(&["read-base", "./views.base"]);
    let second = invoke(&["read-base", "./views.base", "--view", "Second"]);
    let first: Value = serde_json::from_slice(&first.stdout).unwrap();
    let second: Value = serde_json::from_slice(&second.stdout).unwrap();
    assert!(first["content"].as_str().unwrap().contains("| 7 |"));
    assert!(
        second["content"]
            .as_str()
            .unwrap()
            .contains("nested/item.md")
    );
    assert!(
        !invoke(&["read-base", "./views.base", "--view", "Missing"])
            .status
            .success()
    );
    write_note(
        &dir,
        "nested/views.base",
        "views: [{type: table, name: Same}, {type: table, name: Same}]\n",
    );
    assert!(
        !invoke(&["read-base", "./views.base", "--view", "Same"])
            .status
            .success()
    );
}

#[test]
fn frontmatter_and_view_discovery_stdio_contracts() {
    for project in [false, true] {
        let dir = tempdir().unwrap();
        write_note(&dir, "item.md", "---\nscore: 7\n---\n# Body\n");
        write_note(
            &dir,
            "views.base",
            "views: [{type: custom-plugin}, {type: table, name: Table, order: [score]}]\n",
        );
        let mut client = if project {
            McpStdioClient::spawn_without_vault(&dir)
        } else {
            McpStdioClient::spawn(&dir)
        };
        initialize_mcp(&mut client);
        let tools = client
            .request(serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}));
        let tools = tools["result"]["tools"].as_array().unwrap();
        let setter = tools
            .iter()
            .find(|tool| tool["name"] == "set_frontmatter_field")
            .unwrap();
        assert!(
            setter["inputSchema"]["required"]
                .as_array()
                .unwrap()
                .contains(&json!("value"))
        );
        assert_eq!(
            tools.iter().any(|tool| tool["name"] == "query_base_views"),
            !project
        );
        let mut id = 3;
        let mut call = |name: &str, arguments: Value| {
            id += 1;
            client.request(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}}))
        };
        let missing = call(
            "set_frontmatter_field",
            json!({"note":"item.md","field":"score"}),
        );
        assert!(missing.get("error").is_some() || missing["result"]["isError"] == true);
        let preview = call(
            "set_frontmatter_field",
            json!({"note":"item.md","field":"score","value":null}),
        );
        assert_eq!(preview["result"]["structuredContent"]["dry_run"], true);
        assert_eq!(preview["result"]["structuredContent"]["previous_value"], 7);
        let applied = call(
            "set_frontmatter_field",
            json!({"note":"item.md","field":"score","value":null,"dry_run":false}),
        );
        assert_ne!(applied["result"]["isError"], true);
        let noop = call(
            "set_frontmatter_field",
            json!({"note":"item.md","field":"score","value":null,"dry_run":false}),
        );
        assert_eq!(noop["result"]["structuredContent"]["changed"], false);
        let history = call("edit_history", json!({}));
        assert_eq!(
            history["result"]["structuredContent"]["undo"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        call("undo_edit", json!({}));
        assert!(
            fs::read_to_string(dir.path().join("item.md"))
                .unwrap()
                .contains("score: 7")
        );
        let discovery = call("query_base_views", json!({"path":"views.base"}));
        if project {
            assert!(discovery.get("error").is_some() || discovery["result"]["isError"] == true);
        } else {
            assert_eq!(
                discovery["result"]["structuredContent"]["views"][0]["name"],
                Value::Null
            );
            let selected = call("read_base", json!({"path":"views.base","view":"Table"}));
            assert!(
                selected["result"]["structuredContent"]["content"]
                    .as_str()
                    .unwrap()
                    .contains("| 7 |")
            );
        }
        client.shutdown();
    }
}
