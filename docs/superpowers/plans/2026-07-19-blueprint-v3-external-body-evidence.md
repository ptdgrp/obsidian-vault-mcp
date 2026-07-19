# Blueprint v3 External Body and Evidence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the unused Blueprint v2 protocol with v3 fenced external bodies, short aggregate-scoped IDs, first-class Evidence submit/list operations, structured Results references, and actionable completion errors.

**Architecture:** Add a small `ExternalBody` value type that owns line-array input and tilde-fence parsing/rendering. Keep protocol Markdown generation inside the Blueprint service, allocate non-Blueprint IDs by scanning the locked aggregate, and derive canonical Evidence links/indexes from typed Evidence records rather than caller-authored Markdown.

**Tech Stack:** Rust 2024, `serde`, `schemars`, `markdown`, `rmcp`, existing Blueprint store and parser, built-in Rust tests.

## Global Constraints

- Use `schema: blueprint/v3` and `schema: blueprint/todo/v3`; reject v2 without migration.
- Preserve Blueprint ULIDs; generate `todo-N`, `dod-N`, `evidence-N`, and `revision-N` within one Blueprint.
- Store every external body in a tilde fence; protocol syntax remains outside fences.
- External body MCP inputs use `{ "lines": string[] }`; protocol text remains a single string.
- Replace `evidence_add` with `evidence_submit`; add `evidence_list`; provide no compatibility alias.
- Results callers provide Evidence IDs, never Markdown paths or fragments.
- Do not weaken Todo completion or complete Blueprint close Evidence requirements.

---

### Task 1: External Body Value Type and v3 Parsing

**Files:**
- Create: `src/blueprint/body.rs`
- Modify: `src/blueprint.rs`
- Modify: `src/blueprint/model.rs`
- Modify: `src/blueprint/source.rs`
- Modify: `src/blueprint/todo.rs`
- Test: `src/blueprint/tests/body.rs`
- Modify: `src/blueprint/tests/mod.rs`

**Interfaces:**
- Produces: `ExternalBody { lines: Vec<String> }`, `ExternalBody::render() -> String`, and `ExternalBody::parse(field, source) -> anyhow::Result<Self>`.
- Produces: `ResultsInput { body: ExternalBody, evidence_ids: Vec<String> }`.
- Consumes: existing `ParsedDocument::section()` byte-preserving section lookup.

- [ ] **Step 1: Write failing body tests**

```rust
#[test]
fn external_body_round_trips_protocol_looking_markdown() {
    let body = ExternalBody { lines: vec!["## Notes".into(), "[[x]] ^evidence-9".into()] };
    let rendered = body.render().unwrap();
    assert_eq!(ExternalBody::parse("Plan", &rendered).unwrap(), body);
}

#[test]
fn external_body_uses_longer_fence_when_needed() {
    let body = ExternalBody { lines: vec!["~~~".into()] };
    assert!(body.render().unwrap().starts_with("~~~~\n"));
}
```

- [ ] **Step 2: Run the focused test and verify RED**

Run: `cargo test blueprint::tests::body -- --nocapture`

Expected: compilation fails because `ExternalBody` and `body` module do not exist.

- [ ] **Step 3: Implement the body type and parser**

Implement a serde/schemars value type whose renderer selects a fence longer than every leading tilde run in the input, joins lines with LF, and whose parser accepts exactly one tilde fence with no info string. Reject required bodies without a non-whitespace character and reject non-fenced section content.

- [ ] **Step 4: Convert typed source fields and schemas to v3**

Change Blueprint and Todo body fields from `String` to `ExternalBody`; change create/update DTO body fields to `ExternalBody`; introduce:

```rust
pub struct ResultsInput {
    pub body: ExternalBody,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
}
```

Parse Results as one fence plus service-owned trailing Evidence list. Update both document schemas to v3 and reject v2.

- [ ] **Step 5: Verify GREEN**

Run: `cargo test blueprint::tests::body blueprint::tests::source blueprint::tests::todo -- --nocapture`

Expected: all selected v3 parsing tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/blueprint.rs src/blueprint/body.rs src/blueprint/model.rs src/blueprint/source.rs src/blueprint/todo.rs src/blueprint/tests
git commit -m "feat: isolate blueprint external bodies"
```

### Task 2: v3 Rendering and Sequential IDs

**Files:**
- Modify: `src/blueprint/service.rs`
- Modify: `src/blueprint/source.rs`
- Test: `src/blueprint/tests/service.rs`
- Test: `src/blueprint/tests/source.rs`

**Interfaces:**
- Produces: `next_scoped_id(prefix: &str, sources: &[&str]) -> anyhow::Result<String>`.
- Consumes: `ExternalBody::render()` from Task 1.
- Produces: v3 Blueprint/Todo renderers with `dod-N`, `todo-N`, and `revision-N` IDs.

- [ ] **Step 1: Write failing sequential-ID service tests**

```rust
#[test]
fn v3_allocates_short_ids_per_blueprint() {
    let (_, service, bp) = create_blueprint();
    assert!(bp.source.contains("schema: blueprint/v3"));
    assert!(bp.source.contains("^dod-1"));
    let one = create_todo(&service, &bp.id, "one");
    let two = create_todo(&service, &bp.id, "two");
    assert_eq!(one.graph.id, "todo-1");
    assert_eq!(two.graph.id, "todo-2");
}
```

Add a parser test rejecting duplicate, zero, negative, and non-decimal protocol IDs.

- [ ] **Step 2: Run tests and verify RED**

Run: `cargo test blueprint::tests::service::v3_allocates_short_ids_per_blueprint blueprint::tests::source -- --nocapture`

Expected: generated documents still contain v2 and ULID-based IDs.

- [ ] **Step 3: Implement locked aggregate allocation**

Scan active protocol IDs in the Blueprint and Todo sources, parse only `prefix-N` with positive decimal `N`, reject duplicates/malformed values, and return maximum plus one. Do not persist counters. Allocate while holding the existing Blueprint lock.

- [ ] **Step 4: Render all external body fields through `ExternalBody`**

Update `render_blueprint`, `render_todo`, section replacement, Revision rendering, and closure Results handling. Protocol text validation rejects CR/LF. Empty optional bodies render as an empty fence only where the typed model permits an empty body.

- [ ] **Step 5: Verify GREEN**

Run: `cargo test blueprint::tests -- --nocapture`

Expected: all migrated Blueprint v3 tests pass at this checkpoint.

- [ ] **Step 6: Commit**

```bash
git add src/blueprint/service.rs src/blueprint/source.rs src/blueprint/tests
git commit -m "feat: render blueprint v3 with short ids"
```

### Task 3: Evidence Submit, List, and Central Index

**Files:**
- Modify: `src/blueprint/model.rs`
- Modify: `src/blueprint/service.rs`
- Modify: `src/blueprint/source.rs`
- Modify: `src/blueprint/mcp.rs`
- Modify: `src/blueprint.rs`
- Test: `src/blueprint/tests/service.rs`
- Test: `src/blueprint/tests/mcp.rs`

**Interfaces:**
- Produces: `EvidenceSubmitInput { blueprint_id, todo_id, title, body, expected_etag }`.
- Produces: `EvidenceListInput { blueprint_id, todo_id, page }` and `EvidenceListOutput { evidence, pagination, index_status }`.
- Produces: `EvidenceSummary { id, title, scope, todo_id, document, body_preview, blueprint_reference, local_reference }`.

- [ ] **Step 1: Write failing Evidence operation tests**

```rust
#[test]
fn evidence_submit_lists_todo_evidence_and_updates_central_index() {
    let scenario = started_todo();
    let submitted = scenario.service.evidence_submit(EvidenceSubmitInput {
        blueprint_id: scenario.blueprint_id.clone(),
        todo_id: Some(scenario.todo_id.clone()),
        title: "Tests passed".into(),
        body: ExternalBody { lines: vec!["cargo test passed".into()] },
        expected_etag: None,
    }).unwrap();
    assert_eq!(submitted.id, "evidence-1");
    let listed = scenario.service.evidence_list(&scenario.blueprint_id, None, 1).unwrap();
    assert_eq!(listed.evidence[0].blueprint_reference,
        "[Tests passed](todos/todo-1.md#^evidence-1)");
    assert!(scenario.service.blueprint_get(&scenario.blueprint_id).unwrap().source
        .contains("[Tests passed](todos/todo-1.md#^evidence-1)"));
}
```

- [ ] **Step 2: Run tests and verify RED**

Run: `cargo test blueprint::tests::service::evidence_submit_lists_todo_evidence_and_updates_central_index -- --nocapture`

Expected: `evidence_submit` and `evidence_list` do not exist.

- [ ] **Step 3: Implement typed Evidence parsing and summaries**

Parse each Evidence H3 title, `^evidence-N`, and following fenced body. Build canonical local and Blueprint-relative references from the owning document. Sort numerically and paginate without returning full bodies.

- [ ] **Step 4: Implement safe submit and derived index refresh**

For Todo Evidence, build and validate both candidates, write Todo first and Blueprint index second, and restore the previous Todo on an in-process second-write failure. Detect stale indexes on queries; rebuild a stale derived index before the next locked mutation. Never duplicate Todo Evidence bodies in `blueprint.md`.

- [ ] **Step 5: Expose MCP tools and remove `evidence_add`**

Register `evidence_submit` and `evidence_list` in `BlueprintMcp`; remove the old DTO, service method, MCP route, exports, and tool-schema expectations.

- [ ] **Step 6: Verify GREEN**

Run: `cargo test blueprint::tests::service blueprint::tests::mcp -- --nocapture`

Expected: Evidence submit/list/index and MCP schema tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/blueprint.rs src/blueprint/model.rs src/blueprint/service.rs src/blueprint/source.rs src/blueprint/mcp.rs src/blueprint/tests
git commit -m "feat: add blueprint evidence operations"
```

### Task 4: Structured Results and Completion Errors

**Files:**
- Modify: `src/blueprint/model.rs`
- Modify: `src/blueprint/service.rs`
- Modify: `src/blueprint/source.rs`
- Modify: `src/blueprint/mcp.rs`
- Test: `src/blueprint/tests/service.rs`
- Test: `src/blueprint/tests/source.rs`

**Interfaces:**
- Consumes: `ResultsInput` from Task 1 and Evidence lookup from Task 3.
- Produces: canonical Results rendering for Blueprint and Todo updates.
- Produces: actionable complete-close errors with one legal link example.

- [ ] **Step 1: Write failing Results and close tests**

```rust
#[test]
fn results_ids_render_links_and_complete_close_error_is_actionable() {
    let scenario = complete_blueprint_without_results_reference();
    let error = scenario.service.blueprint_close(&scenario.id, "agent", None, None).unwrap_err();
    assert!(error.to_string().contains("evidence_submit"));
    assert!(error.to_string().contains("[Acceptance evidence](#^evidence-1)"));
}
```

Add central, local Todo, and sibling Todo link rendering cases; ensure a link inside a body fence does not satisfy completion.

- [ ] **Step 2: Run tests and verify RED**

Run: `cargo test blueprint::tests::service::results_ids_render_links_and_complete_close_error_is_actionable -- --nocapture`

Expected: old generic error and free-form Results behavior fail the assertions.

- [ ] **Step 3: Resolve Evidence IDs and render protocol links**

Reject duplicate/unknown IDs before writing. Resolve ID ownership across the aggregate and render correct relative paths after the body fence. Store only generated links; typed reads return `ResultsInput` data rather than raw structural Markdown.

- [ ] **Step 4: Split completion failures**

Return separate messages for empty Results, absent Evidence IDs, unknown IDs, malformed stored links, and stale indexes. Use exactly this guidance for missing complete-close references:

```text
complete Blueprint close requires at least one Results Evidence reference.
Submit Evidence with evidence_submit, then add its ID to results.evidence_ids.
Stored Markdown example: [Acceptance evidence](#^evidence-1)
```

- [ ] **Step 5: Verify GREEN**

Run: `cargo test blueprint::tests -- --nocapture`

Expected: all Blueprint lifecycle, Results, and Evidence tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/blueprint/model.rs src/blueprint/service.rs src/blueprint/source.rs src/blueprint/mcp.rs src/blueprint/tests
git commit -m "feat: structure blueprint result evidence"
```

### Task 5: CLI, Generated Documentation, and Full Verification

**Files:**
- Modify: `src/cli/commands.rs`
- Modify: `src/docs.rs`
- Modify: `src/server/tests/dispatch_more.rs`
- Modify: `docs/tools.md` (generated)
- Modify: `README.md`
- Modify: `README.zh-CN.md`
- Test: `tests/cli.rs`
- Test: `src/docs/tests.rs`

**Interfaces:**
- Consumes: final v3 DTOs and service operations.
- Produces: direct CLI parity and generated MCP documentation.

- [ ] **Step 1: Write failing CLI and docs contract tests**

Assert that `evidence-submit` requires `--title`, body lines are accepted without literal `\n`, `evidence-list` supports Blueprint/Todo scope, generated tools contain the v3 schemas, and `evidence_add` is absent.

- [ ] **Step 2: Run tests and verify RED**

Run: `cargo test --test cli && cargo test docs::tests -- --nocapture`

Expected: CLI and generated docs still expose v2 names and string bodies.

- [ ] **Step 3: Update CLI and documentation**

Map repeated `--body-line` arguments to `ExternalBody.lines`; add `evidence-submit` and `evidence-list`; update README examples and lifecycle guidance; regenerate `docs/tools.md` using the existing generator.

- [ ] **Step 4: Run focused GREEN verification**

Run: `cargo fmt --all && cargo test blueprint::tests -- --nocapture && cargo test --test cli && cargo test docs::tests -- --nocapture`

Expected: all focused tests pass with no warnings or failures.

- [ ] **Step 5: Run full repository verification**

Run: `cargo test --all-targets && cargo clippy --all-targets -- -D warnings && cargo run -- generate-docs --check`

Expected: every command exits 0; no test, Clippy, or generated-document differences remain.

- [ ] **Step 6: Commit**

```bash
git add src/cli/commands.rs src/docs.rs src/server/tests/dispatch_more.rs tests/cli.rs README.md README.zh-CN.md docs/tools.md
git commit -m "docs: publish blueprint v3 tools"
```
