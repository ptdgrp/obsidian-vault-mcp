# OTel CLI Lifecycle Logging Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Export startup and CLI lifecycle logs through OTLP and synchronously flush pending logs and traces before every normal CLI or MCP-server exit.

**Architecture:** Keep telemetry ownership in `TelemetryGuard` and centralize command lifecycle logging around the existing command dispatcher. Use one `--log-level` filter for stderr, traces, and OTLP logs, and validate real process-exit delivery with a minimal local OTLP HTTP receiver in the CLI integration tests.

**Tech Stack:** Rust 2024, clap 4, tracing, OpenTelemetry Rust 0.32, OTLP/HTTP protobuf, Cargo integration tests.

## Global Constraints

- Each implementation task is executed by a fresh subagent using model `gpt-5.6-terra` with reasoning effort `medium`.
- The main agent reviews the diff and fresh test evidence between tasks.
- Remove `--otel-log-level` and `OBSIDIAN_VAULT_MCP_OTEL_LOG_LEVEL`.
- `--log-level` is the only filter for stderr, OpenTelemetry traces, and OpenTelemetry logs; its default is `debug`.
- Record complete CLI and MCP inputs in a single `arguments` field on start events, without truncation or redaction. Record the complete formatted error chain on error events. Do not copy successful result/output bodies into lifecycle logs.
- Telemetry delivery failures must not change a successful business command into a failed command.
- Follow red-green-refactor: every production behavior change requires a test that was observed failing first.

## File Map

- Modify `src/main.rs`: CLI flags, unified filter, stable command names, lifecycle events, and shutdown ordering.
- Modify `tests/cli.rs`: CLI surface regression test and real OTLP/HTTP process-exit integration tests.
- Modify `README.md`: English observability contract and examples.
- Modify `README.zh-CN.md`: Chinese observability contract and examples.

---

### Task 1: Unify the stderr and OTLP log filter

**Files:**
- Modify: `tests/cli.rs`
- Modify: `src/main.rs:62-80`
- Modify: `src/main.rs:726-777`

**Interfaces:**
- Consumes: existing clap `Cli` and `telemetry_filter(&str) -> anyhow::Result<EnvFilter>`.
- Produces: `Cli::log_level` with default `debug`; `init_tracing(level, endpoint, service_name)` using the same filter for all layers.

- [ ] **Step 1: Add the failing CLI surface test**

Add this test near the existing CLI help tests in `tests/cli.rs`:

```rust
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
```

- [ ] **Step 2: Run the focused test and verify RED**

Run:

```bash
cargo test --test cli observability_uses_one_debug_log_filter -- --exact
```

Expected: FAIL because help still reports `warn` and exposes `--otel-log-level`.

- [ ] **Step 3: Implement the single filter**

In `Cli`, replace the current logging options with:

```rust
/// Log level for stderr, OpenTelemetry traces, and OpenTelemetry logs.
#[arg(long, default_value = "debug")]
log_level: String,

/// Optional OTLP HTTP endpoint. When absent, telemetry stays on stderr only.
#[arg(long, env = "OTEL_EXPORTER_OTLP_ENDPOINT")]
otel_endpoint: Option<String>,

/// OpenTelemetry service name
#[arg(long, env = "OTEL_SERVICE_NAME", default_value = "obsidian-vault-mcp")]
otel_service_name: String,
```

Change the call to:

```rust
let mut telemetry = init_tracing(
    &cli.log_level,
    cli.otel_endpoint.as_deref(),
    &cli.otel_service_name,
)?;
```

Change the signature to:

```rust
fn init_tracing(
    level: &str,
    otel_endpoint: Option<&str>,
    otel_service_name: &str,
) -> anyhow::Result<TelemetryGuard> {
```

Build one `EnvFilter` from `level` and attach `filter.clone()` to the format layer, trace layer, and `OpenTelemetryTracingBridge`:

```rust
let otel_log_layer =
    opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(&logger_provider)
        .with_filter(filter.clone());
```

- [ ] **Step 4: Run the focused test and verify GREEN**

Run:

```bash
cargo test --test cli observability_uses_one_debug_log_filter -- --exact
```

Expected: PASS.

- [ ] **Step 5: Run the existing CLI suite**

Run:

```bash
cargo test --test cli
```

Expected: all CLI integration tests pass.

- [ ] **Step 6: Commit Task 1**

```bash
git add src/main.rs tests/cli.rs
git commit -m "feat: unify telemetry log filtering"
```

---

### Task 2: Export command lifecycle events before process exit

**Files:**
- Modify: `tests/cli.rs`
- Modify: `src/main.rs:5-7`
- Modify: `src/main.rs:86-408`
- Modify: `src/main.rs:413-637`
- Modify: `src/main.rs:694-787`

**Interfaces:**
- Consumes: `init_tracing(level, endpoint, service_name)` from Task 1 and the existing `TelemetryGuard::shutdown()`.
- Produces: `Command::telemetry_name() -> &'static str`, `TelemetryGuard::is_enabled() -> bool`, and the events `telemetry.initialized`, `cli.command.start`, `cli.command.ok`, and `cli.command.error`.

- [ ] **Step 1: Add a minimal OTLP HTTP capture helper**

Extend the imports in `tests/cli.rs` with `Read`, `TcpListener`, and `Sender`, then add a helper that binds `127.0.0.1:0`, accepts one HTTP request, reads its `Content-Length` body, returns an empty successful protobuf response, and sends `(path, body)` to the test:

```rust
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

fn capture_otlp_request(listener: TcpListener, sender: mpsc::Sender<(String, Vec<u8>)>) {
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
    body.windows(value.len()).any(|window| window == value.as_bytes())
}
```

- [ ] **Step 2: Add success and failure process-exit tests**

Add two tests:

```rust
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
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));

    let (path, body) = capture.recv();
    assert_eq!(path, "/v1/logs");
    for event in ["telemetry.initialized", "cli.command.start", "cli.command.ok"] {
        assert!(protobuf_contains(&body, event), "missing {event} in OTLP payload");
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
```

- [ ] **Step 3: Run both tests and verify RED**

Run:

```bash
cargo test --test cli cli_flushes -- --nocapture
```

Expected: FAIL or timeout because the current binary emits no lifecycle events and therefore sends no log request.

- [ ] **Step 4: Add stable command names**

Add `Command::telemetry_name()` with a complete match that returns these exact low-cardinality names:

```rust
impl Command {
    fn telemetry_name(&self) -> &'static str {
        match self {
            Self::Serve => "serve",
            Self::GenerateDocs { .. } => "generate_docs",
            Self::Doctor => "doctor",
            Self::ListNotes { .. } => "list_notes",
            Self::AuditLinks { .. } => "audit_links",
            Self::GetNoteNeighborhood { .. } => "get_note_neighborhood",
            Self::ReadNote { .. } => "read_note",
            Self::GetNoteStructure { .. } => "get_note_structure",
            Self::GetNoteOutline { .. } => "get_note_outline",
            Self::GetNoteStats { .. } => "get_note_stats",
            Self::ResolveRef { .. } => "resolve_ref",
            Self::GetOutlinks { .. } => "get_outlinks",
            Self::GetBacklinks { .. } => "get_backlinks",
            Self::ListTags { .. } => "list_tags",
            Self::GetTag { .. } => "get_tag",
            Self::ListCategories { .. } => "list_categories",
            Self::GetCategory { .. } => "get_category",
            Self::QueryFrontmatter { .. } => "query_frontmatter",
            Self::SearchText { .. } => "search_text",
            Self::SearchRegex { .. } => "search_regex",
            Self::AppendSection { .. } => "append_section",
            Self::ReplaceSection { .. } => "replace_section",
            Self::DeleteSection { .. } => "delete_section",
            Self::RenameHeading { .. } => "rename_heading",
            Self::RenameNote { .. } => "rename_note",
            Self::RenameBlockId { .. } => "rename_block_id",
        }
    }
}
```

- [ ] **Step 5: Add centralized lifecycle logging**

Import `Instant` alongside `Duration`. Derive the stable command name by borrowing `cli.command` before telemetry initialization, without moving the command out of `Cli`:

```rust
let command_name = cli
    .command
    .as_ref()
    .map_or("serve", Command::telemetry_name);
let mut telemetry = init_tracing(
    &cli.log_level,
    cli.otel_endpoint.as_deref(),
    &cli.otel_service_name,
)?;
tracing::debug!(otel.enabled = telemetry.is_enabled(), "telemetry.initialized");
```

Leave `vault_config(&cli)` before the existing `cli.command.unwrap_or(Command::Serve)` move. This preserves the current ownership order while retaining `command_name` as a static string.

Immediately before the existing `let result = async {` line, add:

```rust
let started = Instant::now();
tracing::debug!(command = command_name, "cli.command.start");
```

Keep the existing async command body unchanged. Immediately after its `.await`, replace the current shutdown tail with:

```rust
let duration_ms = started.elapsed().as_millis() as u64;
match &result {
    Ok(()) => tracing::debug!(command = command_name, duration_ms, "cli.command.ok"),
    Err(error) => tracing::error!(
        command = command_name,
        duration_ms,
        error = %error,
        "cli.command.error"
    ),
}
telemetry.shutdown();
result
```

Implement the guard query without exposing endpoint data:

```rust
fn is_enabled(&self) -> bool {
    self.tracer_provider.is_some() || self.logger_provider.is_some()
}
```

Keep `TelemetryGuard::shutdown()` idempotent through the existing `Option::take()` calls. Its order remains: emit terminal event first, call each provider's `force_flush`, then call its timeout-bounded shutdown, then return from `main`.

- [ ] **Step 6: Run both tests and verify GREEN**

Run:

```bash
cargo test --test cli cli_flushes -- --nocapture
```

Expected: both process-exit tests pass and each receives `/v1/logs` before the child process has returned.

- [ ] **Step 7: Run all tests**

Run:

```bash
cargo test
```

Expected: all unit and integration tests pass.

- [ ] **Step 8: Commit Task 2**

```bash
git add src/main.rs tests/cli.rs
git commit -m "feat: export cli lifecycle logs"
```

---

### Task 3: Document the unified lifecycle contract

**Files:**
- Modify: `README.md:196-223`
- Modify: `README.zh-CN.md:156-175`

**Interfaces:**
- Consumes: the CLI behavior and event names implemented by Tasks 1 and 2.
- Produces: user-facing English and Chinese configuration and Loki-query guidance.

- [ ] **Step 1: Add a failing documentation assertion**

Add a test in `tests/cli.rs` that reads both README files through `env!("CARGO_MANIFEST_DIR")` and asserts that both contain `telemetry.initialized`, `cli.command.start`, `cli.command.ok`, `cli.command.error`, `http://10.5.11.4:11418`, and `{service_name="obsidian-vault-mcp"}`.

```rust
#[test]
fn observability_docs_cover_lifecycle_and_loki_query() {
    for path in ["README.md", "README.zh-CN.md"] {
        let content = fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path),
        )
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
```

- [ ] **Step 2: Run the documentation test and verify RED**

Run:

```bash
cargo test --test cli observability_docs_cover_lifecycle_and_loki_query -- --exact
```

Expected: FAIL because the lifecycle contract and Loki query are not documented.

- [ ] **Step 3: Update both observability sections**

Document all of the following in both languages:

```text
--log-level defaults to debug and controls stderr, OTEL traces, and OTEL logs.
--otel-endpoint http://10.5.11.4:11418 enables OTLP/HTTP export.
telemetry.initialized, cli.command.start, cli.command.ok, and cli.command.error describe CLI lifecycles.
MCP tool calls retain mcp.tool and tool.call.* events.
CLI completion and MCP shutdown force-flush pending logs and traces before process exit.
Grafana Loki query: {service_name="obsidian-vault-mcp"}
```

Remove wording that says OTEL logs have a separate warning/error default.

- [ ] **Step 4: Run the documentation test and verify GREEN**

Run:

```bash
cargo test --test cli observability_docs_cover_lifecycle_and_loki_query -- --exact
```

Expected: PASS.

- [ ] **Step 5: Run formatting and complete verification**

Run:

```bash
cargo fmt --check
cargo test
git diff --check
```

Expected: all commands exit 0; the test output reports no failures; `git diff --check` prints nothing.

- [ ] **Step 6: Commit Task 3**

```bash
git add README.md README.zh-CN.md tests/cli.rs
git commit -m "docs: explain otel lifecycle logging"
```

## Final Review Gate

After Task 3, the main agent must inspect the complete diff and run fresh verification:

```bash
cargo fmt --check
cargo test
git diff --check HEAD~3..HEAD
```

Then verify the requirements line by line:

- only `--log-level` remains and defaults to `debug`;
- successful and failing commands export lifecycle logs;
- shutdown happens after terminal-event emission and before returning from `main`;
- no dedicated lifecycle field contains command arguments, vault paths or content, search text, regular expressions, or OTLP endpoint values, while `cli.command.error` retains the full `error = %error` value;
- both READMEs document the Loki query and flush behavior.
