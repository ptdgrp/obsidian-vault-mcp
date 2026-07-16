use std::net::TcpListener;
use std::{
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde_json::Value;
use tempfile::{TempDir, tempdir};

#[test]
fn observability_docs_cover_lifecycle_and_loki_query() {
    for path in ["README.md", "README.zh-CN.md"] {
        let content =
            fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
                .expect("read observability docs");
        for expected in [
            "telemetry.initialized",
            "cli.command.start",
            "cli.command.ok",
            "cli.command.error",
            "http://10.5.11.4:11418",
            r#"{service_name="obsidian-vault-mcp"}"#,
        ] {
            assert!(content.contains(expected), "{path} missing {expected}");
        }
    }
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

struct OtlpHttpCapture {
    endpoint: String,
    request: Receiver<(String, Vec<u8>)>,
    server: Option<JoinHandle<()>>,
}

impl OtlpHttpCapture {
    fn spawn() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind OTLP capture");
        let endpoint = format!("http://{}", listener.local_addr().expect("capture address"));
        let (sender, request) = mpsc::channel();
        let server = thread::spawn(move || capture_otlp_request(listener, sender));
        Self {
            endpoint,
            request,
            server: Some(server),
        }
    }

    fn recv(mut self) -> (String, Vec<u8>) {
        let request = self
            .request
            .recv_timeout(Duration::from_secs(5))
            .expect("receive OTLP request");
        self.server
            .take()
            .expect("OTLP server thread")
            .join()
            .expect("join OTLP server");
        request
    }
}

fn capture_otlp_request(listener: TcpListener, sender: Sender<(String, Vec<u8>)>) {
    let (mut stream, _) = listener.accept().expect("accept OTLP request");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set OTLP read timeout");
    let mut reader = BufReader::new(stream.try_clone().expect("clone OTLP stream"));
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .expect("read OTLP request line");
    let path = request_line
        .split_whitespace()
        .nth(1)
        .expect("OTLP request path")
        .to_string();

    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).expect("read OTLP header");
        if line == "\r\n" {
            break;
        }
        if let Some(value) = line
            .strip_prefix("content-length:")
            .or_else(|| line.strip_prefix("Content-Length:"))
        {
            content_length = value.trim().parse().expect("OTLP content length");
        }
    }

    let mut body = vec![0; content_length];
    reader.read_exact(&mut body).expect("read OTLP body");
    stream
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/x-protobuf\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )
        .expect("write OTLP response");
    sender.send((path, body)).expect("send captured request");
}

fn protobuf_contains(body: &[u8], value: &str) -> bool {
    body.windows(value.len())
        .any(|window| window == value.as_bytes())
}

#[test]
fn successful_cli_flushes_lifecycle_logs_before_exit() {
    let dir = tempdir().expect("tempdir");
    write_note(&dir, "note.md", "# Note\n");
    let capture = OtlpHttpCapture::spawn();
    let output = Command::new(env!("CARGO_BIN_EXE_obsidian-vault-mcp"))
        .args(["--vault", dir.path().to_str().expect("vault path")])
        .args(["--otel-endpoint", &capture.endpoint])
        .arg("doctor")
        .output()
        .expect("run instrumented CLI");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let (path, body) = capture.recv();
    assert_eq!(path, "/v1/logs");
    for event in [
        "telemetry.initialized",
        "cli.command.start",
        "cli.command.ok",
    ] {
        assert!(
            protobuf_contains(&body, event),
            "missing {event} in OTLP payload"
        );
    }
}

#[test]
fn failing_cli_flushes_error_log_before_exit() {
    let capture = OtlpHttpCapture::spawn();
    let output = Command::new(env!("CARGO_BIN_EXE_obsidian-vault-mcp"))
        .args(["--otel-endpoint", &capture.endpoint])
        .arg("doctor")
        .output()
        .expect("run failing instrumented CLI");
    assert!(!output.status.success());

    let (path, body) = capture.recv();
    assert_eq!(path, "/v1/logs");
    assert!(protobuf_contains(&body, "cli.command.error"));
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
        let mut child = Command::new(env!("CARGO_BIN_EXE_obsidian-vault-mcp"))
            .arg("--vault")
            .arg(dir.path())
            .args(["--log-level", "warn"])
            .arg("serve")
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
fn removed_commands_and_flags_are_unknown_to_cli() {
    let dir = tempdir().expect("tempdir");
    write_note(&dir, "note.md", "# Note\n");

    for command in [
        "find-unresolved-links",
        "find-ambiguous-links",
        "get-vault-graph",
        "get-graph-neighborhood",
        "collect-note-context",
        "collect-reference-context",
        "list-vault-files",
    ] {
        let output = run_cli(&dir, &[command]);
        assert!(!output.status.success(), "{command} should be rejected");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(command),
            "{command} stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    for args in [
        &["list-notes", "--page-size", "50"][..],
        &["list-notes", "--cursor", "next"][..],
        &["get-outlinks", "note.md", "--verbose"][..],
        &["get-backlinks", "note.md", "--verbose"][..],
        &["list-tags", "--verbose"][..],
        &["get-tag", "tag", "--verbose"][..],
        &["search-text", "needle", "--context-lines", "2"][..],
        &["search-regex", "needle", "--context-lines", "2"][..],
        &["get-note-outline", "note.md", "--heading", "Section"][..],
    ] {
        let output = run_cli(&dir, args);
        assert!(!output.status.success(), "{args:?} should be rejected");
        let stderr = String::from_utf8_lossy(&output.stderr);
        let flag = args
            .iter()
            .find(|arg| arg.starts_with("--"))
            .expect("removed flag");
        assert!(stderr.contains(flag), "{args:?} stderr: {stderr}");
    }
}

#[test]
fn removed_commands_new_command_help_uses_mcp_task_definitions() {
    for (command, description) in [
        (
            "list-notes",
            "Page through visible Markdown notes for lightweight navigation.",
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
fn observability_uses_one_debug_log_filter() {
    let output = run_raw_cli(&["--help"]);
    assert!(
        output.status.success(),
        "help stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let help = String::from_utf8_lossy(&output.stdout);
    assert!(help.contains("--log-level <LOG_LEVEL>"), "help: {help}");
    assert!(help.contains("[default: debug]"), "help: {help}");
    assert!(!help.contains("--otel-log-level"), "help: {help}");
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
