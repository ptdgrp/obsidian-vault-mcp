# Backlinks Path Filters Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let `get_backlinks` restrict results to source notes matching include/exclude vault-relative path globs.

**Architecture:** Reuse `PathFilter`, compile it once per request, and filter indexed source notes before their links are examined. Target resolution, result ordering, verbose transformation, and post-filter truncation stay unchanged.

**Tech Stack:** Rust 2024, `globset`, RMCP schemas, Clap, Cargo tests.

## Global Constraints

- Filters match backlink source-note paths only; they never alter target resolution.
- Empty include accepts all source paths; exclude takes precedence.
- Invalid glob errors identify include or exclude.
- Compact and verbose outputs share the same filtered result.

---

### Task 1: Add failing source-filter regressions

**Files:**
- Modify: `src/query/tests/files_and_context.rs`
- Modify: `src/server/tests/dispatch_more.rs`
- Modify: `tests/cli.rs`

**Interfaces:**
- Consumes proposed `get_backlinks(target, include, exclude)`.
- Produces query, MCP, and CLI coverage.

- [ ] **Step 1: Write failing tests**

Create `来源/保留.md` and `来源/排除.md`, both linking to `发动机`. Add:

```rust
let result = queries.get_backlinks(
    "发动机",
    &["来源/**/*.md".to_string()],
    &["**/排除.md".to_string()],
).expect("filtered backlinks");
assert_eq!(result.backlinks.len(), 1);
assert_eq!(result.backlinks[0].source.path, "来源/保留.md#L1-L1");
```

Also assert an invalid `[` include produces an error containing `include`; add a tool-schema assertion for array `include` and `exclude`; add a CLI invocation with repeatable `--include 来源/**/*.md --exclude **/排除.md`.

- [ ] **Step 2: Verify RED**

Run: `cargo test get_backlinks_honors_source_path_filters`

Expected: compilation failure because current backlink methods take no filter arguments.

- [ ] **Step 3: Commit tests**

```bash
git add src/query/tests/files_and_context.rs src/server/tests/dispatch_more.rs tests/cli.rs
git commit -m "test: cover backlinks path filters"
```

### Task 2: Filter indexed source notes and expose parameters

**Files:**
- Modify: `src/query/links.rs:43-90`
- Modify: `src/server.rs:121-130,548-560`
- Modify: `src/main.rs:155-175,480-490`
- Modify: `docs/tools.md`

**Interfaces:**
- Produces `get_backlinks(&self, target: &str, include: &[String], exclude: &[String])`.
- Produces `get_backlinks_output(&self, target: &str, verbose: bool, include: &[String], exclude: &[String])`.
- Produces defaulted `include` and `exclude` vectors on `BacklinksRequest`.

- [ ] **Step 1: Implement query filtering**

At the start of `get_backlinks`, create the existing filter type:

```rust
let filter = PathFilter::new(include, exclude)?;
```

Change only the source-note loop:

```rust
for note in notes.iter().filter(|note| filter.is_match(&note.file.relative_path)) {
    for link in &note.parsed.links {
        let matches = wanted_path
            .as_ref()
            .is_some_and(|path| RefResolver::link_matches(&link.target, path, &notes))
            || link.target == target;
        if matches {
            backlinks.push(LinkEvidence {
                source: link.source.clone().into(),
                target: link.target.clone(),
                alias: link.alias.clone(),
                resolved: RefResolver::resolve(&link.target, &notes).into(),
                snippet: read_snippet(&note.file, &link.source),
            });
        }
    }
}
```

Thread both slices through `get_backlinks_output`.

- [ ] **Step 2: Add MCP and CLI fields**

Add the fields to `BacklinksRequest`:

```rust
#[serde(default)]
pub include: Vec<String>,
#[serde(default)]
pub exclude: Vec<String>,
```

Destructure and forward them in the server tool. Add repeatable vectors with the standard path-glob help text to `Command::GetBacklinks`, and forward them to `get_backlinks_output`. Regenerate docs:

```bash
cargo run -- generate-docs
```

- [ ] **Step 3: Verify GREEN**

Run: `cargo test get_backlinks_honors_source_path_filters && cargo test backlinks_tool && cargo test --test cli && cargo run -- generate-docs --check`

Expected: PASS. Only allowed source notes remain in results and docs/schema show both arrays.

- [ ] **Step 4: Full verification and commit**

Run: `cargo fmt --check && cargo test`

Expected: PASS.

```bash
git add src/query/links.rs src/server.rs src/main.rs src/query/tests src/server/tests tests/cli.rs docs/tools.md
git commit -m "feat: filter backlinks by source path"
```
