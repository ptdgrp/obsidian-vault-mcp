# Read-note Character Limit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate `read_note` from byte-oriented truncation to Unicode-character limits throughout its public and configuration APIs.

**Architecture:** `VaultQueries::read_note` receives an optional character budget and falls back to `VaultConfig::max_read_note_chars`. It compares and truncates Markdown using `str::chars()`, while the MCP adapter, CLI, help text, and generated documentation expose only the new character-oriented names.

**Tech Stack:** Rust 2024, `clap`, `schemars`, `rmcp`, Rust unit tests, generated Markdown documentation.

## Global Constraints

- This is a breaking migration: do not provide `max_bytes` or old CLI/configuration aliases.
- Keep the numeric default at `4096`, now representing Unicode scalar values.
- Use `str::chars()` for both the limit check and prefix construction.
- Preserve valid UTF-8 and source-order prefix behavior.

---

### Task 1: Character-based query behavior and configuration

**Files:**

- Modify: `src/vault.rs:8,139,155`
- Modify: `src/query.rs:928-937`
- Modify: `src/query/notes.rs:14,35-47`
- Modify: `src/query/tests/mod.rs:8,579-610`

**Interfaces:**

- Consumes: `VaultConfig::max_read_note_chars: usize` and `VaultQueries::read_note(&self, note: &str, max_chars: Option<usize>)`.
- Produces: a `ReadNoteResult` whose `content` is at most the requested or configured count of `str::chars()` and whose guidance names `max_chars`.

- [ ] **Step 1: Write the failing multibyte-character regression tests**

```rust
#[test]
fn read_note_counts_unicode_characters_not_utf8_bytes() {
    let (dir, mut queries) = fixture();
    fs::write(dir.path().join("字符.md"), "甲乙丙丁").expect("write unicode note");
    queries.vault.config.max_read_note_chars = 2;

    let result = queries.read_note("字符.md", None).expect("read note");

    assert_eq!(result.content, "甲乙");
    assert!(result.truncated);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test read_note_counts_unicode_characters_not_utf8_bytes --lib`

Expected: compilation failure because `max_read_note_chars` does not exist.

- [ ] **Step 3: Implement the renamed configuration and query API**

```rust
pub const DEFAULT_MAX_READ_NOTE_CHARS: usize = 4 * 1024;

fn truncate_chars(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect()
}

let budget = max_chars.unwrap_or(self.vault.config.max_read_note_chars);
if content.chars().count() > budget {
    content = truncate_chars(&content, budget);
    truncated = true;
}
```

Rename the existing budget test and override test to use `.chars().count()`,
`DEFAULT_MAX_READ_NOTE_CHARS`, and `max_chars` in their expectations.

- [ ] **Step 4: Run focused query tests to verify they pass**

Run: `cargo test read_note_ --lib`

Expected: all `read_note_` tests pass, including the multibyte regression.

- [ ] **Step 5: Commit the query migration**

Run:

```bash
git add src/vault.rs src/query.rs src/query/notes.rs src/query/tests/mod.rs
git commit -m "feat: limit read note by characters"
```

### Task 2: MCP, CLI, and generated documentation contract

**Files:**

- Modify: `src/main.rs:22-23,52-58,617`
- Modify: `src/server.rs:80-85,377-383`
- Modify: `src/server/tests.rs:161`
- Modify: `docs/tools.md`

**Interfaces:**

- Consumes: `VaultQueries::read_note(&self, note, max_chars)` and `DEFAULT_MAX_READ_NOTE_CHARS` from Task 1.
- Produces: MCP schema field `max_chars`, CLI flag `--max-read-note-chars`, and generated docs that have no `max_bytes` read-note contract.

- [ ] **Step 1: Write a failing schema-contract test**

```rust
#[test]
fn read_note_schema_exposes_max_chars_not_max_bytes() {
    let tools = ObsidianVaultMcp::tool_definitions();
    let read_note = tools.iter().find(|tool| tool.name == "read_note").expect("read_note tool");
    let schema = serde_json::to_value(&read_note.input_schema).expect("input schema json");

    assert!(schema["properties"].get("max_chars").is_some());
    assert!(schema["properties"].get("max_bytes").is_none());
}
```

- [ ] **Step 2: Run the schema test to verify it fails**

Run: `cargo test read_note_schema_exposes_max_chars_not_max_bytes --lib`

Expected: assertion failure because the old schema exposes `max_bytes`.

- [ ] **Step 3: Rename the MCP and CLI contract**

```rust
pub struct ReadNoteRequest {
    /// Optional character limit for this request.
    pub max_chars: Option<usize>,
}

/// Max Unicode characters returned by read_note
#[arg(long, default_value_t = DEFAULT_MAX_READ_NOTE_CHARS)]
max_read_note_chars: usize,
```

Remove `parse_byte_size` and its `value_parser` only from this option; retain
byte-size parsing for output-size options. Update the adapter, descriptions, and
configuration construction, then run `cargo run -- generate-docs`.

- [ ] **Step 4: Run contract and documentation checks**

Run: `cargo test read_note_schema_exposes_max_chars_not_max_bytes --lib && cargo run -- check-docs`

Expected: both commands exit 0.

- [ ] **Step 5: Commit the public contract migration**

Run:

```bash
git add src/main.rs src/server.rs src/server/tests.rs docs/tools.md
git commit -m "feat: expose read note character limits"
```

### Task 3: Full verification

**Files:**

- Verify only: repository test suite and generated tools documentation.

**Interfaces:**

- Consumes: the finished renamed API and generated documentation.
- Produces: fresh evidence that the entire repository remains green.

- [ ] **Step 1: Format all Rust changes**

Run: `cargo fmt --check`

Expected: exit status 0. If it reports formatting changes, run `cargo fmt` and repeat the check.

- [ ] **Step 2: Run all tests**

Run: `cargo test`

Expected: exit status 0 with no test failures.

- [ ] **Step 3: Check generated documentation**

Run: `cargo run -- check-docs`

Expected: exit status 0.

- [ ] **Step 4: Inspect the final migration surface**

Run: `rg -n "max_bytes|max_read_note_bytes|DEFAULT_MAX_READ_NOTE_BYTES" src docs/tools.md`

Expected: no results for the read-note public and configuration contract.
