# Remove Note Stats Backlink Count Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `get_note_stats` a target-only operation by removing `backlink_count` and its whole-vault scan.

**Architecture:** Keep note scope resolution and local word, character, and line counting unchanged. Remove the backlink field at the public result boundary, then delete the now-unused stats-specific backlink helpers while leaving dedicated backlink queries intact.

**Tech Stack:** Rust 2024, rmcp, schemars, serde, Cargo tests, generated Markdown tool documentation.

## Global Constraints

- Do not add a compatibility field or optional backlink flag.
- Do not change dedicated backlink tools.
- Do not change scope, word, character, or line count semantics.
- Follow test-driven development and verify the regression test fails before production edits.

---

### Task 1: Remove Backlink Count from Note Statistics

**Files:**
- Modify: `src/query/tests/mod.rs`
- Modify: `src/server/tests.rs`
- Modify: `src/query.rs`
- Modify: `src/query/notes.rs`
- Modify: `src/query/links.rs`
- Modify: `docs/tools.md` (generated)

**Interfaces:**
- Consumes: `VaultQueries::get_note_stats(&self, note: &str) -> anyhow::Result<NoteStatsResult>`.
- Produces: `NoteStatsResult { scope, word_count, character_count, line_count }` with no `backlink_count` property.

- [ ] **Step 1: Write the failing query-contract test**

Update `get_note_stats_counts_words_characters_and_total_backlinks` to assert that serialized note stats omit `backlink_count`, while preserving existing local count assertions:

```rust
let value = serde_json::to_value(result).expect("stats json");
assert!(value.get("backlink_count").is_none());
```

Remove assertions that read `result.backlink_count`. Rename the test to
`get_note_stats_counts_only_the_selected_note`.

- [ ] **Step 2: Run the query test and verify RED**

Run:

```bash
cargo test get_note_stats_counts_only_the_selected_note -- --nocapture
```

Expected: FAIL because serialized `NoteStatsResult` still contains `backlink_count`.

- [ ] **Step 3: Add the failing MCP schema assertion**

In `src/server/tests.rs`, keep the behavior test focused on local counts and add this assertion to
`note_stats_schema_does_not_expose_word_count_mode`:

```rust
assert!(!properties.contains_key("backlink_count"));
```

Rename the behavior test to `note_stats_tool_returns_local_counts` and remove its backlink assertion.

- [ ] **Step 4: Implement the minimal production change**

Remove this field from `NoteStatsResult` in `src/query.rs`:

```rust
pub backlink_count: usize,
```

Remove backlink calculation and result assignment from `VaultQueries::get_note_stats` in
`src/query/notes.rs`. Preserve parsing because scoped word and line counts still require the parsed
target note.

Delete these helpers from `src/query/links.rs` after confirming no remaining call sites:

```rust
backlink_count_for_path
backlink_count_for_scope
```

Do not remove `count_matching_backlinks` if dedicated backlink behavior still uses it.

- [ ] **Step 5: Run focused tests and verify GREEN**

Run:

```bash
cargo test get_note_stats -- --nocapture
cargo test note_stats -- --nocapture
```

Expected: all matching query and server tests pass with no `backlink_count` field.

- [ ] **Step 6: Regenerate and check tool documentation**

Run:

```bash
cargo run -- generate-docs --output docs/tools.md
cargo run -- generate-docs --check --output docs/tools.md
```

Expected: `docs/tools.md` no longer lists `backlink_count`; the check exits 0.

- [ ] **Step 7: Verify the original large-vault symptom**

Run:

```bash
/usr/bin/time -p cargo run --release -- \
  --vault /Users/ashen/Projects/novel_proj \
  --log-level warn \
  get-note-stats '正文/vol01-卷一/章节/ch006.md'
```

Expected: output contains only `scope`, `word_count`, `character_count`, and `line_count`; elapsed
time reflects one-note work rather than a whole-vault backlink scan.

- [ ] **Step 8: Run full verification**

Run:

```bash
cargo fmt --all --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
git diff --check
```

Expected: every command exits 0.

- [ ] **Step 9: Commit the implementation**

```bash
git add src/query.rs src/query/notes.rs src/query/links.rs \
  src/query/tests/mod.rs src/server/tests.rs docs/tools.md
git commit -m "perf: keep note stats local"
```
