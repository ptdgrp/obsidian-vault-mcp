# Note Suggestions and Output Schema Contracts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make unresolved note-reference suggestions prefer same-directory filename prefixes without relaxing resolution, and ensure `get_outlinks` always conforms to its declared MCP output schema.

**Architecture:** Keep strict resolution in `RefResolver` unchanged. Extend only the unresolved-suggestion path in `src/query.rs` so it ranks exact same-directory filenames, then `stem + "-"` prefixes, before the existing edit-distance fallback; preserve all equally ranked prefix suggestions for error text and expose one only when unique in `resolve_ref`. Make outlink target groups a stable, always-present collection shape and lock the behavior with schema-aware serialization tests.

**Tech Stack:** Rust, serde, schemars, rmcp, Cargo test suite.

## Global Constraints

- Do not make filename-prefix candidates resolvable by `RefResolver`.
- Prefix matching applies only to the filename stem; parent directory and extension must match exactly.
- Treat `-` as the sole prefix separator.
- Retain the existing bounded Levenshtein fallback only after exact and prefix candidates fail.
- Stable output collections required by the output schema serialize as empty arrays, never omitted fields.

---

## File Structure

- Modify: `src/query.rs` — rank unresolved note suggestions, render multi-candidate errors, and keep `resolve_ref` singular suggestions unambiguous.
- Modify: `src/query/tests/edge_cases.rs` — integration coverage for strict reads and human-facing `did you mean` errors.
- Modify: `src/query/tests/mod.rs` — structured `resolve_ref` contract and schema/serialization regression coverage.
- Modify: `src/query.rs` — remove conditional omission from required outlink collection fields.

### Task 1: Rank unresolved note suggestions without changing resolution

**Files:**
- Modify: `src/query.rs:584-649`
- Test: `src/query/tests/edge_cases.rs:106-124`
- Test: `src/query/tests/mod.rs:945-978`

**Interfaces:**
- Consumes: `find_indexed_note`, `suggested_note_reference`, `suggested_note_path`, `ObsidianRef`, and `IndexedNote` from `src/query.rs`.
- Produces: an internal `suggested_note_paths(target: &str, notes: &[IndexedNote]) -> Vec<String>` sorted by natural path order; `suggested_note_reference` returns `Some` only for exactly one candidate whose requested heading/block selector exists.

- [ ] **Step 1: Write failing same-directory prefix and strict-resolution tests**

  Add a temporary fixture directory and files to `src/query/tests/edge_cases.rs`. The test must call `read_note` (rather than `RefResolver` directly) to cover the user-visible error path:

  ```rust
  #[test]
  fn missing_path_prefers_same_directory_stem_prefix_suggestions_without_resolving() {
      let (dir, queries) = fixture();
      let chapter_dir = dir.path().join("正文/vol01-卷一/章节");
      fs::create_dir_all(&chapter_dir).expect("create chapter directory");
      fs::write(chapter_dir.join("ch005-归途与北声点火.md"), "# ch005\n")
          .expect("write prefixed chapter");
      fs::write(chapter_dir.join("ch006.md"), "# ch006\n").expect("write near chapter");

      let error = queries
          .read_note("正文/vol01-卷一/章节/ch005.md", None, None)
          .expect_err("prefix candidate must not resolve the request");

      assert!(error.to_string().contains("ch005-归途与北声点火.md"));
      assert!(!error.to_string().contains("ch006.md"));
  }
  ```

  Add a separate test with `ch005-a.md`, `ch005-b.md`, `other/ch005-c.md`, `ch005c.md`, and `ch005-note.txt`. Assert that the error includes both same-directory Markdown hyphen-prefix candidates in natural order and excludes the differing-directory, separator-less, and differing-extension paths.

- [ ] **Step 2: Run the focused tests and verify RED**

  Run:

  ```sh
  cargo test missing_path_prefers_same_directory_stem_prefix_suggestions_without_resolving
  cargo test missing_path_lists_same_directory_stem_prefix_suggestions_only
  ```

  Expected: both fail because the current Levenshtein-only fallback can select `ch006.md` and has no multi-candidate prefix output.

- [ ] **Step 3: Write a failing structured-result uniqueness test**

  In `src/query/tests/mod.rs`, construct two same-directory `ch005-*.md` notes and call `queries.resolve_ref("正文/vol01-卷一/章节/ch005.md")`. Assert that the result serializes as:

  ```rust
  serde_json::json!({
      "unresolved_target": "正文/vol01-卷一/章节/ch005.md"
  })
  ```

  Then use a fixture with exactly one `ch005-*.md` and assert `suggested_target` contains that path. This preserves a single-valued structured API while allowing the human-facing error to list multiples.

- [ ] **Step 4: Run the structured-result tests and verify RED**

  Run:

  ```sh
  cargo test resolve_ref_
  ```

  Expected: new prefix-specific cases fail; existing non-prefix suggestion tests still pass.

- [ ] **Step 5: Implement the smallest ranking helper and error formatter**

  Replace the single-path fallback with a helper that:

  ```rust
  fn suggested_note_paths(target: &str, notes: &[IndexedNote]) -> Vec<String> {
      // 1. collect exact path candidates;
      // 2. retain candidates with identical parent and extension;
      // 3. return exact filename stems if any;
      // 4. otherwise return stems beginning with format!("{requested_stem}-");
      // 5. otherwise return the one bounded-Levenshtein fallback, if present.
  }
  ```

  Use `camino::Utf8Path` to derive `parent`, `extension`, and `file_stem` instead of splitting paths manually. Sort returned paths with `natord::compare` and deduplicate them. Update `find_indexed_note` to format one candidate as the existing `did you mean "…"` message and multiple candidates as `did you mean one of: "…", "…"`. Update `suggested_note_reference` to call the new helper and return a path only when the filtered candidate list has exactly one item and its selector exists.

  Do not edit `RefResolver::resolve` or `find_candidates` in `src/resolver.rs`.

- [ ] **Step 6: Run focused tests and verify GREEN**

  Run:

  ```sh
  cargo test missing_path_
  cargo test resolve_ref_outputs_compact_unresolved_json
  ```

  Expected: all focused tests pass; the prefix request remains unresolved while its error contains the correct candidates.

- [ ] **Step 7: Commit the resolver-suggestion change**

  ```sh
  git add src/query.rs src/query/tests/edge_cases.rs src/query/tests/mod.rs
  git commit -m "fix: rank unresolved note suggestions by filename prefix"
  ```

### Task 2: Make outlink collection output schema-safe

**Files:**
- Modify: `src/query.rs:273-281`
- Test: `src/query/tests/mod.rs:1008-1105`
- Test: `src/server/tests/dispatch_more.rs:171-181`

**Interfaces:**
- Consumes: `OutlinksResult`, `ObsidianVaultMcp::tool_definitions()`, and `serde_json::to_value`.
- Produces: every serialized `OutlinksResult` contains `note`, `targets`, `ambiguous_targets`, `unresolved_targets`, and `pagination` whenever those keys are required by the tool output schema.

- [ ] **Step 1: Write a failing empty-group schema contract test**

  Add a test in `src/query/tests/mod.rs` using a note whose links all resolve. Serialize `queries.get_outlinks("Links", 1)` and retrieve the `get_outlinks` `output_schema` through `ObsidianVaultMcp::tool_definitions()`. For each string in `output_schema["required"]`, assert the serialized JSON object contains that property:

  ```rust
  let required = definition.output_schema
      .as_ref()
      .expect("outlinks output schema")["required"]
      .as_array()
      .expect("required output properties");
  for field in required {
      assert!(
          value.get(field.as_str().expect("field name")).is_some(),
          "serialized outlinks result is missing required field {field}"
      );
  }
  assert_eq!(value["ambiguous_targets"], serde_json::json!([]));
  assert_eq!(value["unresolved_targets"], serde_json::json!([]));
  ```

- [ ] **Step 2: Run the test and verify RED**

  Run:

  ```sh
  cargo test serialized_outlinks_include_all_schema_required_empty_groups
  ```

  Expected: FAIL because `ambiguous_targets` and `unresolved_targets` are skipped when empty.

- [ ] **Step 3: Emit required empty arrays**

  Remove only these attributes from `OutlinksResult` in `src/query.rs`:

  ```rust
  #[serde(skip_serializing_if = "Vec::is_empty")]
  ```

  Do not change the output schema or make these collections nullable. They remain `Vec<_>` and serialize as `[]` when empty.

- [ ] **Step 4: Run the schema contract and outlink pagination tests**

  Run:

  ```sh
  cargo test serialized_outlinks_include_all_schema_required_empty_groups
  cargo test outlinks_
  ```

  Expected: all pass. Update existing exact JSON expectations for page two and unresolved-only results to include empty required arrays.

- [ ] **Step 5: Add an output-schema audit assertion for all advertised tools**

  In `src/server/tests/dispatch_more.rs`, extend the existing output-schema test to assert that every advertised output schema has an object root and an array-valued `required` property when it declares `properties`. This guards schema generation shape; Task 2's serialization test guards the concrete empty-collection response that caused the MCP failure.

- [ ] **Step 6: Run the server schema test and verify GREEN**

  Run:

  ```sh
  cargo test tool_output_schemas_have_object_roots_when_present
  ```

  Expected: PASS.

- [ ] **Step 7: Run formatting and the full regression suite**

  Run:

  ```sh
  cargo fmt --check
  cargo test
  ```

  Expected: formatting succeeds and the complete test suite passes.

- [ ] **Step 8: Commit the output-contract fix**

  ```sh
  git add src/query.rs src/query/tests/mod.rs src/server/tests/dispatch_more.rs
  git commit -m "fix: serialize required outlink target groups"
  ```

## Plan Self-Review

- Spec coverage: Task 1 implements strict unresolved-only prefix ranking, exact directory/extension constraints, `-` separator handling, multi-candidate error output, and edit-distance fallback. Task 2 serializes required outlink groups and adds both concrete and schema-shape regression checks.
- Placeholder scan: no deferred implementation placeholders remain; each implementation step names the target functions, data shape, and verification command.
- Type consistency: `suggested_note_paths` is internal and vector-valued; the existing singular `suggested_target` remains `Option<String>` and is produced only for a unique candidate.
