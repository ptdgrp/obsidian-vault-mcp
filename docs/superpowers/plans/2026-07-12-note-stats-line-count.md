# Note Stats Line Count Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Return the raw Markdown source line count from `get-note-stats`.

**Architecture:** Extend the shared `NoteStatsResult` response model with a required `line_count` field. Calculate it in `VaultQueries::get_note_stats` from the already-loaded source string, which automatically exposes it through the existing CLI and MCP serialization paths.

**Tech Stack:** Rust 2024, Serde, Schemars, RMCP, Cargo tests.

## Global Constraints

- The public field name is exactly `line_count` and its type is `usize`.
- Count raw source lines with `content.lines().count()`.
- Blank lines count; a trailing final line terminator does not add a line.
- Line count is independent of `word_count_mode`.

---

### Task 1: Implement and verify line counts with TDD

**Files:**
- Modify: `src/query/tests/mod.rs:177-230`
- Modify: `src/server/tests.rs:238-255`
- Modify: `src/query.rs:58-70`
- Modify: `src/query/notes.rs:57-73`
- Modify: `src/server.rs:397-412`

**Interfaces:**
- Consumes: `VaultQueries::get_note_stats(&str, WordCountMode) -> anyhow::Result<NoteStatsResult>`.
- Produces: `NoteStatsResult { line_count: usize, .. }` plus query and MCP-server regression coverage.

- [ ] **Step 1: Write the failing query-layer assertions**

  In `get_note_stats_counts_words_characters_and_total_backlinks`, add:

  ```rust
  assert_eq!(result.line_count, content.lines().count());
  ```

  Add a separate test after it to lock down trailing-newline semantics:

  ```rust
  #[test]
  fn get_note_stats_counts_blank_lines_without_extra_trailing_line() {
      let (dir, queries) = fixture();
      fs::write(dir.path().join("line-count.md"), "first\n\nthird\n")
          .expect("write line count note");

      let result = queries
          .get_note_stats("line-count.md", WordCountMode::Source)
          .expect("note stats");

      assert_eq!(result.line_count, 3);
  }
  ```

- [ ] **Step 2: Run the new query test and verify it fails because the result lacks `line_count`**

  Run: `cargo test get_note_stats_counts_blank_lines_without_extra_trailing_line`

  Expected: compilation failure that `NoteStatsResult` has no field `line_count`.

- [ ] **Step 3: Write the failing MCP-server assertion**

  In `note_stats_tool_returns_word_character_and_backlink_counts`, add:

  ```rust
  assert_eq!(result.line_count, 14);
  ```

  The fixture contents for `林动.md` have 14 lines under `str::lines()` semantics.

- [ ] **Step 4: Run the server test and verify it fails for the same missing field**

  Run: `cargo test note_stats_tool_returns_word_character_and_backlink_counts`

  Expected: compilation failure that `NoteStatsResult` has no field `line_count`.

- [ ] **Step 5: Add the result field**

  In `NoteStatsResult`, directly after `character_count`, add:

  ```rust
  /// Line count computed from the note's Markdown source text.
  pub line_count: usize,
  ```

- [ ] **Step 6: Populate it while assembling stats**

  In the `NoteStatsResult` construction in `VaultQueries::get_note_stats`, directly after `character_count`, add:

  ```rust
  line_count: content.lines().count(),
  ```

- [ ] **Step 7: Make the tool description reflect its response**

  Change the `get_note_stats` `#[tool]` description in `src/server.rs` to:

  ```rust
  #[tool(description = "Return one note's word, character, line, and total backlink counts")]
  ```

- [ ] **Step 8: Run the focused regression tests and verify they pass**

  Run: `cargo test get_note_stats_counts_blank_lines_without_extra_trailing_line && cargo test note_stats_tool_returns_word_character_and_backlink_counts`

  Expected: both commands exit successfully and each runs one passing test.

- [ ] **Step 9: Run all project tests and formatting checks**

  Run: `cargo fmt --check && cargo test`

  Expected: both commands exit successfully with no test failures.

- [ ] **Step 10: Commit the implementation**

  ```bash
  git add src/query.rs src/query/notes.rs src/query/tests/mod.rs src/server.rs src/server/tests.rs
  git commit -m "feat: return note line count in stats"
  ```
