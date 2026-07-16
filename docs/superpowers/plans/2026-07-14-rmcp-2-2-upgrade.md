# rmcp 2.2.0 Upgrade Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Upgrade this MCP server to rmcp 2.2.0 without changing its public tool contracts, then prove schemas and stdio transport behavior end-to-end.

**Architecture:** Keep `ObsidianVaultMcp` as the existing router-backed server. First freeze the public tool definition contract in a unit test. Then update the dependency and make only compiler-directed rmcp 2.2.0 API substitutions. Finally test the compiled `serve` command as a line-oriented JSON-RPC process.

**Tech Stack:** Rust 2024, rmcp 2.2.0, Tokio, schemars, serde_json, Cargo integration tests.

## Global Constraints

- Set `rmcp` to exactly `2.2.0` in `Cargo.toml` and refresh `Cargo.lock`.
- Do not retain rmcp 1.x compatibility code, conditional compilation, shims, or wrappers.
- Do not rename tools or change request fields, response fields, default values, queries, or mutations.
- Make only rmcp API changes required by rmcp 2.2.0 compiler diagnostics.
- Preserve the unrelated untracked plan files already in `docs/superpowers/plans/`.

---

### Task 1: Freeze the public tool-schema contract

**Files:**
- Modify: `src/server/tests.rs:42-77`

**Interfaces:**
- Consumes: `ObsidianVaultMcp::tool_definitions() -> Vec<rmcp::model::Tool>`.
- Produces: A regression test that records all 23 public MCP tool names and validates schema roots before and after the dependency change.

- [ ] **Step 1: Write the failing schema-contract test**

  Add this test immediately above `all_tool_input_schemas_have_plain_object_roots`:

  ```rust
  #[test]
  fn public_tool_definitions_preserve_names_and_object_schemas() {
      let tools = ObsidianVaultMcp::tool_definitions();
      let names = tools.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>();
      assert_eq!(names, vec![
          "list_notes", "audit_links", "get_note_neighborhood", "read_note",
          "get_note_structure", "get_note_stats", "get_note_outline", "search_text",
          "search_regex", "resolve_ref", "get_outlinks", "get_backlinks", "list_tags",
          "get_tag", "list_categories", "get_category", "query_frontmatter",
          "append_section", "replace_section", "delete_section", "rename_heading",
          "rename_note", "rename_block_id",
      ]);
      for tool in tools {
          assert_eq!(tool.input_schema.get("type").and_then(|v| v.as_str()), Some("object"));
          if let Some(output_schema) = &tool.output_schema {
              assert_eq!(output_schema.get("type").and_then(|v| v.as_str()), Some("object"));
          }
      }
  }
  ```

- [ ] **Step 2: Run the test to establish the pre-upgrade baseline**

  Run: `cargo test -- --exact server::tests::public_tool_definitions_preserve_names_and_object_schemas`

  Expected: PASS. If the copied ordered name list or an object-root assumption disagrees with the current generated definitions, correct the test expectation from the current router output before proceeding; do not change production code in this task.

- [ ] **Step 3: Run the schema test green before the upgrade**

  Run: `cargo test -- --exact server::tests::public_tool_definitions_preserve_names_and_object_schemas`

  Expected: PASS, providing the pre-upgrade contract baseline.

- [ ] **Step 4: Commit the contract baseline**

  ```bash
  git add src/server/tests.rs
  git commit -m "test: freeze MCP tool schema contract"
  ```

### Task 2: Upgrade rmcp and migrate only incompatible SDK calls

**Files:**
- Modify: `Cargo.toml:20-25`
- Modify: `Cargo.lock`
- Modify: `src/server.rs:12-29` and only lines reported by `cargo check`

**Interfaces:**
- Consumes: Task 1's generated tool-definition contract.
- Produces: `run_mcp_server(vault: Vault) -> anyhow::Result<()>` that serves the unchanged `ObsidianVaultMcp` using rmcp 2.2.0.

- [ ] **Step 1: Change the dependency only**

  Replace the rmcp dependency declaration with:

  ```toml
  rmcp = { version = "2.2.0", features = ["server", "macros", "schemars", "transport-io"] }
  ```

- [ ] **Step 2: Refresh the lockfile and capture compiler diagnostics**

  Run: `cargo update -p rmcp --precise 2.2.0 && cargo check`

  Expected: `Cargo.lock` resolves `rmcp 2.2.0`; `cargo check` may fail exclusively at rmcp import, wrapper, trait, router, or stdio-service API differences.

- [ ] **Step 3: Make the smallest compiler-directed migration**

  For every rmcp diagnostic, substitute the 2.2.0 public path or call shape without introducing an adapter. Examples to verify against diagnostics and rmcp 2.2.0 docs:

  ```rust
  use rmcp::{
      handler::server::{router::tool::ToolRouter, tool::Parameters},
      Json, ServerHandler, ServiceExt,
  };
  ```

  Retain the existing `#[tool_router]`, `#[tool_handler(router = self.tool_router)]`, `ServerHandler::get_info`, `serve(...)`, and `waiting()` structure unless the compiler reports a changed signature. Do not convert synchronous tool methods to async unless rmcp 2.2.0 requires it.

- [ ] **Step 4: Verify compilation and the frozen schema contract**

  Run: `cargo check && cargo test -- --exact server::tests::public_tool_definitions_preserve_names_and_object_schemas && cargo test -- --exact server::tests::mcp_tool_schemas_do_not_use_uint_format`

  Expected: all commands exit 0; no tool name, object root, or unsupported `uint` format changes.

- [ ] **Step 5: Commit the dependency migration**

  ```bash
  git add Cargo.toml Cargo.lock src/server.rs src/server/tests.rs
  git commit -m "chore: upgrade rmcp to 2.2.0"
  ```

### Task 3: Add a real stdio JSON-RPC smoke test

**Files:**
- Modify: `tests/cli.rs:1-25` and append one integration test

**Interfaces:**
- Consumes: `env!("CARGO_BIN_EXE_obsidian-vault-mcp")`, temporary vault fixtures, and the `serve` subcommand.
- Produces: A subprocess test that verifies `initialize`, `tools/list`, and `tools/call` over stdio, not direct Rust handler dispatch.

- [ ] **Step 1: Write the failing end-to-end test**

  Add `BufRead`, `BufReader`, `Write`, `Stdio`, and `Duration` imports; then add a test which starts `serve`, sends these newline-delimited messages, and parses each response with `serde_json::Value`:

  ```json
  {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"smoke","version":"1"}}}
  {"jsonrpc":"2.0","method":"notifications/initialized"}
  {"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
  {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"read_note","arguments":{"note":"smoke.md"}}}
  ```

  The fixture must write `smoke.md` containing `# Smoke\n\nready\n`. Spawn the binary as `--vault <tempdir> serve`, pipe stdin/stdout, and inherit no extra output into stdout. Assert response ids 1, 2, and 3; assert `tools/list` contains `read_note`; assert the tool-call structured result contains `smoke.md` and `ready`.

- [ ] **Step 2: Run the test and confirm it fails before its implementation is complete**

  Run: `cargo test --test cli -- --exact mcp_stdio_initialize_lists_tools_and_calls_read_note`

  Expected: FAIL because the test or its request/response synchronization helper does not exist yet. If the failure is a protocol-version rejection, retain the server's advertised protocol version from the initialize response and update only the request constant.

- [ ] **Step 3: Implement deterministic process cleanup**

  Implement a small test-local helper that writes one JSON line, flushes, reads the next stdout line with a bounded wait, and parses it. After the final response, drop stdin and call `child.wait()`. Assert successful termination so no process remains after test completion.

- [ ] **Step 4: Run the smoke test green**

  Run: `cargo test --test cli -- --exact mcp_stdio_initialize_lists_tools_and_calls_read_note`

  Expected: PASS. The test has observed a real MCP initialize response, a tool list, and `read_note` result via stdio.

- [ ] **Step 5: Commit the black-box regression**

  ```bash
  git add tests/cli.rs
  git commit -m "test: smoke test MCP stdio transport"
  ```

### Task 4: Run the release gate

**Files:**
- Verify only: `Cargo.toml`, `Cargo.lock`, `src/server.rs`, `src/server/tests.rs`, `tests/cli.rs`, `docs/tools.md`

**Interfaces:**
- Consumes: all migration and regression tests above.
- Produces: fresh evidence that source compilation, schemas, generated docs, integration tests, and release build are sound.

- [ ] **Step 1: Run all test targets**

  Run: `cargo test --all-targets`

  Expected: exit 0 with no failed tests.

- [ ] **Step 2: Check generated tool documentation**

  Run: `cargo run -- generate-docs --check`

  Expected: exit 0. If it reports schema-rendered documentation drift, regenerate only via `cargo run -- generate-docs` and review the resulting `docs/tools.md` diff before committing it.

- [ ] **Step 3: Build the distributable artifact**

  Run: `cargo build --release`

  Expected: exit 0.

- [ ] **Step 4: Inspect the final scope and commit any generated docs only when changed**

  Run: `git diff --check && git status --short`

  Expected: no whitespace errors; changed paths limited to the dependency files, required rmcp API call sites, regression tests, and `docs/tools.md` if the generator changed it.

  If and only if the generator changed that file, run:

  ```bash
  git add docs/tools.md
  git commit -m "docs: refresh MCP tool reference"
  ```
