# Query Path Filters Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add request-level include/exclude glob arrays to tag, category, and search queries across the Rust query API, MCP tools, CLI, and documentation.

**Architecture:** A focused query-layer `PathFilter` compiles request patterns once and filters the globally visible `NoteFile` list before parsing or reading. Tags, categories, and both search implementations consume that shared filter, while MCP and CLI request types pass arrays without duplicating matching logic.

**Tech Stack:** Rust 2024, `globset`, `rayon`, `clap`, `rmcp`, `schemars`, Cargo tests.

## Global Constraints

- `include` and `exclude` are arrays; missing or empty arrays impose no additional restriction.
- A path passes include when include is empty or any include pattern matches; any exclude match rejects it.
- Patterns match vault-relative Markdown paths using `globset` syntax.
- Request filters only narrow the globally visible note set and cannot restore globally ignored notes.
- Invalid patterns fail the request and identify the field and offending pattern.
- Remove `search_regex.path_glob`; do not retain a compatibility alias.
- Keep all response structures unchanged except removing `SearchRegexResult.path_glob`.
- Preserve unrelated user changes already present in the worktree.

---

### Task 1: Shared request path filter

**Files:**
- Create: `src/query/path_filter.rs`
- Modify: `src/query.rs`
- Test: `src/query/tests/edge_cases.rs`

**Interfaces:**
- Produces: `pub(super) struct PathFilter` with `pub(super) fn new(include: &[String], exclude: &[String]) -> anyhow::Result<Self>` and `pub(super) fn is_match(&self, relative_path: &str) -> bool`.
- Produces: `pub(super) fn filter_notes(&self, notes: Vec<NoteFile>) -> Vec<NoteFile>` if this avoids duplicated iterator code in later tasks.

- [ ] **Step 1: Write failing unit tests for include union, exclude precedence, empty filters, and contextual invalid-pattern errors**

Add tests in the private query test module that construct `PathFilter` directly. Use paths `正文/001.md`, `资料/设定.md`, and `正文/草稿/002.md`. Assert two includes match the first two paths, `**/草稿/**` rejects the third, empty arrays accept every path, and invalid `include: ["["]` / `exclude: ["["]` errors contain both the field name and `[`.

- [ ] **Step 2: Run tests and verify RED**

Run: `cargo test query::tests::edge_cases -- --nocapture`

Expected: compilation fails because `query::path_filter::PathFilter` does not exist.

- [ ] **Step 3: Implement the minimal shared filter**

Create `src/query/path_filter.rs`. Compile each pattern individually with `Glob::new`, attaching context such as `invalid include glob "[": ...`, add valid globs to a `GlobSetBuilder`, build both sets, store whether include was empty, and implement:

```rust
pub(super) fn is_match(&self, path: &str) -> bool {
    (self.include_is_empty || self.include.is_match(path))
        && !self.exclude.is_match(path)
}
```

Declare `mod path_filter;` in `src/query.rs`. Keep this type internal to the query layer.

- [ ] **Step 4: Run focused tests and verify GREEN**

Run: `cargo test query::tests::edge_cases -- --nocapture`

Expected: all edge-case tests pass.

- [ ] **Step 5: Commit the task**

```bash
git add src/query.rs src/query/path_filter.rs src/query/tests/edge_cases.rs
git commit -m "feat: add request path filter"
```

### Task 2: Apply filters in tag, category, and search queries

**Files:**
- Modify: `src/query/links.rs`
- Modify: `src/query/categories.rs`
- Modify: `src/query/search.rs`
- Modify: `src/query.rs`
- Test: `src/query/tests/mod.rs`
- Test: `src/query/tests/search_and_section.rs`

**Interfaces:**
- Consumes: `PathFilter::new(include, exclude)` and `PathFilter::is_match(path)` from Task 1.
- Produces these query signatures:

```rust
list_tags(scope: TagScope, include: &[String], exclude: &[String])
get_tags(tags: &[String], scope: TagScope, verbose: bool, include: &[String], exclude: &[String])
list_categories(include: &[String], exclude: &[String])
get_categories(categories: &[String], include: &[String], exclude: &[String])
search_text(query: &str, case_sensitive: bool, context_lines: usize, include: &[String], exclude: &[String])
search_regex(pattern: &str, case_sensitive: bool, context_lines: usize, include: &[String], exclude: &[String])
```

- [ ] **Step 1: Write failing query behavior tests**

Extend fixtures with notes under at least two directories and a nested excluded directory. Add separate assertions that tag lists/locations, category lists/files, literal search matches, and regex search matches contain only paths selected by two include patterns and then remove paths selected by exclude. Add one test setting `queries.vault.config.exclude` and asserting request include cannot restore that note. Replace the invalid `path_glob` test with invalid include and exclude tests that check contextual errors.

- [ ] **Step 2: Run focused tests and verify RED**

Run: `cargo test query::tests -- --nocapture`

Expected: compilation fails because the query methods do not accept include/exclude arrays and `search_regex` still accepts `path_glob`.

- [ ] **Step 3: Implement filtering before work**

In each public query entry point, compile `PathFilter` before collecting results. For tags, filter the `index_notes()` iterator before examining parsed tags. For categories, filter indexed notes before deriving categories. For searches, remove the local single-glob implementation, filter `vault.list_notes()?` before `into_par_iter()`, and share the same file-selection path between literal and regex collection where practical without unrelated refactoring.

Remove `path_glob` from `SearchRegexResult` in `src/query.rs` and its construction in `src/query/search.rs`.

- [ ] **Step 4: Migrate all existing query call sites**

Update existing tests and internal calls to pass `&[]` for both arrays when no request-level filtering is intended. Do not change their assertions except where they explicitly covered `path_glob`.

- [ ] **Step 5: Run query tests and verify GREEN**

Run: `cargo test query::tests -- --nocapture`

Expected: all query tests pass.

- [ ] **Step 6: Commit the task**

```bash
git add src/query.rs src/query/links.rs src/query/categories.rs src/query/search.rs src/query/tests/mod.rs src/query/tests/search_and_section.rs
git commit -m "feat: filter tag category and search queries"
```

### Task 3: Expose filters through MCP and CLI

**Files:**
- Modify: `src/server.rs`
- Modify: `src/server/tests.rs`
- Modify: `src/server/tests/dispatch_more.rs`
- Modify: `src/main.rs`
- Modify: `tests/cli.rs`

**Interfaces:**
- Consumes: all six filtered query signatures from Task 2.
- Produces MCP request fields `#[serde(default)] pub include: Vec<String>` and `#[serde(default)] pub exclude: Vec<String>` on list-tags, get-tags, list-categories, get-categories, search-text, and search-regex request types.
- Produces repeatable Clap `#[arg(long)] include: Vec<String>` and `exclude: Vec<String>` arguments on the six corresponding subcommands.

- [ ] **Step 1: Write failing MCP schema and dispatch tests**

Inspect `ObsidianVaultMcp::tool_definitions()` by tool name. Assert each of the six input schemas has `include` and `exclude` array properties, and `search_regex` lacks `path_glob`. Add dispatch tests proving an include/exclude request changes returned paths for at least tags and search, which exercises both aggregation and match output handler plumbing.

- [ ] **Step 2: Write failing CLI integration tests**

Create a temporary vault with matching content/tags in `正文/keep.md`, `资料/keep.md`, and `正文/草稿/drop.md`. Invoke all six commands with repeatable `--include` values and an `--exclude` value; assert JSON output contains both kept directories and omits the draft path. Add a `search-regex --path-glob` invocation and assert Clap rejects the unknown argument.

- [ ] **Step 3: Run tests and verify RED**

Run: `cargo test server::tests -- --nocapture`

Run: `cargo test --test cli -- --nocapture`

Expected: failures show missing schema fields/CLI arguments or old `path_glob` support.

- [ ] **Step 4: Implement MCP request and handler plumbing**

Add a dedicated `ListCategoriesRequest` rather than using `EmptyRequest`. Add include/exclude arrays with serde defaults to the six request structs, destructure them in handlers, and pass references to the query layer. Remove `path_glob` from `SearchRegexRequest`. Update descriptions to say filters use vault-relative glob patterns.

- [ ] **Step 5: Implement CLI plumbing**

Add repeatable include/exclude vectors to the six Clap command variants, destructure them in command dispatch, and pass them to query methods. Remove `path_glob` from `SearchRegex`.

- [ ] **Step 6: Migrate existing server and CLI call sites**

Add empty vectors to existing request literals and command expectations. Confirm deserializing omitted MCP fields still yields empty arrays via `#[serde(default)]`.

- [ ] **Step 7: Run MCP and CLI tests and verify GREEN**

Run: `cargo test server::tests -- --nocapture`

Run: `cargo test --test cli -- --nocapture`

Expected: all server and CLI tests pass.

- [ ] **Step 8: Commit the task**

```bash
git add src/server.rs src/server/tests.rs src/server/tests/dispatch_more.rs src/main.rs tests/cli.rs
git commit -m "feat: expose query path filters"
```

### Task 4: Documentation and full verification

**Files:**
- Modify: `README.md`
- Modify: `README.zh-CN.md`
- Modify: `docs/tools.md`

**Interfaces:**
- Documents the MCP and CLI interfaces produced by Task 3.

- [ ] **Step 1: Update user documentation**

Replace every `path_glob` example and field with `include`/`exclude`. For all six tools, document array types, empty-array behavior, include union semantics, exclude precedence, and vault-relative matching. Update CLI examples to use repeatable `--include` and `--exclude`, including the Chinese README example corresponding to the English one.

- [ ] **Step 2: Verify stale API text is gone**

Run: `rg -n "path_glob" README.md README.zh-CN.md docs/tools.md src tests`

Expected: no matches.

- [ ] **Step 3: Format and run the full suite**

Run: `cargo fmt --check`

Expected: success with no output. If it fails, run `cargo fmt`, then re-run `cargo fmt --check`.

Run: `cargo test --all-targets`

Expected: all tests pass with no failures.

Run: `cargo clippy --all-targets -- -D warnings`

Expected: success with no warnings.

- [ ] **Step 4: Review the final diff for scope and user changes**

Run: `git diff --check` and `git status --short`. Confirm only files required by this plan are included in task commits and pre-existing unrelated worktree changes remain untouched.

- [ ] **Step 5: Commit documentation or formatting changes**

```bash
git add README.md README.zh-CN.md docs/tools.md
git commit -m "docs: describe query path filters"
```
