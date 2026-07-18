# Blueprint Semantic Coverage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add behavior-focused tests that cover the Blueprint v0.1 lifecycle, Todo graph, validation, parsing, storage, and public contract semantics.

**Architecture:** Extend the existing test modules under `src/blueprint/tests/`. Exercise workflows through `BlueprintService` when several layers participate, and use focused tests for `validate`, `source`, and `store` error contracts. Production code changes are out of scope unless a new semantic test proves a discrepancy that the user explicitly approves fixing.

**Tech Stack:** Rust 2024, built-in test harness, `tempfile`, `camino`, `cargo-llvm-cov`, existing Blueprint service/store/parser APIs.

## Global Constraints

- Every test must correspond to a protocol rule, state transition, derived state, preservation guarantee, or meaningful error.
- Do not add tests that merely reconstruct a value and assert its fields.
- Prefer returned protocol models and persisted Markdown over implementation-detail assertions.
- Do not test process-level stdin/stdout startup, logging-only branches, or every thin MCP forwarding handler solely for coverage.
- Do not modify or stage the unrelated `src/blueprint/todo.rs` file.
- Stop and report before changing production behavior if a semantic test contradicts the implementation.

---

### Task 1: Blueprint Views, Derived Status, and Lifecycle

**Files:**
- Modify: `src/blueprint/tests/service.rs`

**Interfaces:**
- Consumes: `BlueprintService::{blueprint_create, blueprint_view, blueprint_status, blueprint_close, blueprint_cancel, dod_update, todo_create, todo_assign, todo_start, todo_block}`.
- Produces: Service-level characterization tests for full/resume views, all derived status groups, closure outcomes, and cancellation persistence.

- [x] **Step 1: Add a reusable valid Blueprint scenario helper**

Add a helper that keeps the temporary directory alive and returns a freshly created protocol document:

```rust
use tempfile::{TempDir, tempdir};

fn create_blueprint() -> (TempDir, BlueprintService, super::super::service::BlueprintCreated) {
    let directory = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).expect("UTF-8 path");
    let service = BlueprintService::new(root);
    let blueprint = service
        .blueprint_create(BlueprintCreateRequest {
            title: "协议测试".into(),
            created_by: "creator".into(),
            intent: "验证 Blueprint 协议".into(),
            constraints: vec!["保留 Markdown".into()],
            definition_of_done: vec!["完成协议验证".into()],
            plan: "按状态转换验证".into(),
        })
        .expect("create Blueprint");
    (directory, service, blueprint)
}
```

- [x] **Step 2: Add full and resume view tests**

Create a ready Todo, an in-progress Todo, and a blocked Todo. Assert that `view = full` returns `source` and no `resume`; assert that `view = resume` omits source and contains Intent, Constraints, Plan, open DoD, ready IDs, and active Todo objects for ready/in-progress/blocked work. Also assert that an unsupported view reports `view must be full or resume`.

- [x] **Step 3: Run the focused view tests**

Run:

```bash
cargo test blueprint::tests::service::blueprint_views_return_full_source_or_resume_context -- --exact
```

Expected: PASS, or a semantic discrepancy reported before any production edit.

- [x] **Step 4: Add a derived status classification test**

Construct these states through service calls:

```text
ready-unassigned: pending, no dependencies, no owner
prerequisite: pending
dependent: pending, Depends On prerequisite
blocked: assigned -> in_progress -> blocked
cancelled: pending -> cancelled
```

Assert exact membership of `ready_todos`, `not_ready_todos`, `blocked_todos`, `unassigned_todos`, `open_todos`, and `open_definition_of_done`. Confirm cancelled work is not open and a cancelled dependency remains unsatisfied.

- [x] **Step 5: Add complete and incomplete close tests**

For incomplete close, assert that omitting `reason` fails and that a supplied reason persists `Outcome: incomplete`, actor, reason, open DoD IDs, and open Todo IDs before moving to `closed/`. For complete close, complete or cancel all Todos, update the DoD to checked, close without a reason, and assert `Outcome: complete` with no open lists.

- [x] **Step 6: Add cancellation lifecycle test**

Call `blueprint_cancel` with whitespace-padded actor/reason. Assert trimmed `Cancelled By` in Record, a `### Cancellation` result with actor/reason, state `cancelled`, absence from `active`, and presence in `cancelled`.

- [x] **Step 7: Run service lifecycle tests**

Run:

```bash
cargo test blueprint::tests::service -- --nocapture
```

Expected: all service tests pass.

---

### Task 2: Todo Mutations, Eligibility, and Filters

**Files:**
- Modify: `src/blueprint/tests/service.rs`

**Interfaces:**
- Consumes: `BlueprintService::{todo_create, todo_get, todo_list, todo_update, todo_assign, todo_start, todo_complete, todo_block, todo_cancel, dod_update}` and `CheckUpdate`.
- Produces: Tests for every Todo state transition and mutation restriction from the v0.1 protocol.

- [x] **Step 1: Test assignment and reassignment rules**

Assert owner whitespace is trimmed. Start the Todo and verify reassignment to a different owner fails without Handoff. Add Handoff using:

```rust
service.todo_update(
    &blueprint.id,
    &todo.id,
    None,
    None,
    None,
    Some(&["continue from checkpoint".into()]),
    None,
    None,
)?;
```

Then assert reassignment succeeds. Complete or cancel representative Todos and assert assignment is rejected for both terminal states.

- [x] **Step 2: Test start eligibility**

Create one Todo without Owner and assert start fails. Create a prerequisite and dependent Todo, both assigned, and assert the dependent cannot start until the prerequisite is started and completed. Assert a non-pending Todo cannot start again.

- [x] **Step 3: Test block and cancel transitions**

Assert block rejects pending work, empty reason, and empty Handoff. Assert an in-progress Todo becomes blocked with trimmed Block Reason and Handoff. Assert pending, in-progress, and blocked Todos can be cancelled with Cancel Reason, while completed and already-cancelled Todos cannot.

- [x] **Step 4: Test completion eligibility including children**

Assert completion rejects a pending Todo and an in-progress Todo with unchecked criteria. Create a parent with a pending child and assert the in-progress parent cannot complete. Cancel or complete the child, check all criteria through `todo_update`, then assert completion writes `Completed By` and `Result Summary` and preserves Owner.

- [x] **Step 5: Test allowed Todo updates and dependency validation**

Update title, Depends On, Completion Criteria, repeated Handoff values, and Result Summary together. Reload the Todo and assert every allowed field. Then attempt a self-dependency and an unknown dependency and assert that neither invalid graph is persisted.

- [x] **Step 6: Test combined Todo list filters**

Create Todos with different status, owners, and readiness. Assert exact IDs for each of:

```rust
service.todo_list(&id, Some(TodoStatus::Pending), Some("agent-a"), Some(true))
service.todo_list(&id, None, None, Some(false))
service.todo_list(&id, Some(TodoStatus::Blocked), None, None)
```

- [x] **Step 7: Test DoD updates**

Extract the generated `dod-*` ID from the created source, mark it complete with a note, and assert only that DoD marker changed and the note is persisted. Reopen it and assert `[x]` returns to `[ ]`. Assert an unknown DoD ID is rejected without changing the ETag.

- [x] **Step 8: Run focused Todo tests**

Run:

```bash
cargo test blueprint::tests::service -- --nocapture
```

Expected: all Todo lifecycle tests pass.

---

### Task 3: Dependency Graph and State Invariants

**Files:**
- Modify: `src/blueprint/tests/validate.rs`

**Interfaces:**
- Consumes: `validate_dependency_graph(&[Todo])` and `derive_readiness(&[Todo])`.
- Produces: Direct, isolated tests for graph and state invariant errors.

- [x] **Step 1: Extend the Todo fixture for semantic mutations**

Keep the existing `todo` constructor and clone/mutate its returned value in each scenario. Add a helper that asserts a diagnostic fragment:

```rust
fn assert_invalid(todos: &[Todo], expected: &str) {
    let error = validate_dependency_graph(todos).expect_err("graph must be rejected");
    assert!(error.to_string().contains(expected), "{error:#}");
}
```

- [x] **Step 2: Test dependency identity errors**

Add one test covering self-dependency, unknown dependency, and duplicate Todo IDs. Each case must assert its specific error fragment (`cannot depend on itself`, `unknown Todo`, `duplicate Todo ID`). Keep the existing cycle test as the multi-node graph case.

- [x] **Step 3: Test required state fields**

Create otherwise-valid Todos and assert rejection for:

```text
missing Created By
in_progress without Owner
completed without Completed By
completed without Result Summary
completed with an unchecked Completion Criterion
blocked without Block Reason
blocked without Handoff
cancelled without Cancel Reason
```

For each error, set all unrelated required fields so the intended invariant is the first failure.

- [x] **Step 4: Test completed parent child constraints**

Attach an open child to a fully valid completed parent and assert `open child Todos`. Change the child to cancelled with a reason and assert validation succeeds; repeat with a valid completed child.

- [x] **Step 5: Test readiness includes only pending work**

Build completed, in-progress, blocked, cancelled, ready pending, and not-ready pending Todos. Assert only the two pending Todos appear in readiness, with exact dependency status for the not-ready item.

- [x] **Step 6: Run validation tests**

Run:

```bash
cargo test blueprint::tests::validate -- --nocapture
```

Expected: all graph and invariant tests pass.

---

### Task 4: Blueprint Source and Store Error Contracts

**Files:**
- Modify: `src/blueprint/tests/source.rs`
- Modify: `src/blueprint/tests/store.rs`

**Interfaces:**
- Consumes: `ParsedBlueprintSource::parse`, `BlueprintStore::{ensure_workspace, create, read, read_in, list, write, move_to}`.
- Produces: Tests for structural Markdown recognition, workspace integrity, identifiers, states, lifecycle movement, and optimistic concurrency.

- [x] **Step 1: Test required source structure**

Using the existing `blueprint` fixture, assert parsing rejects a non-`.md` path, each missing required H2 represented by removing `## Notes`, and a duplicate required section represented by appending a second `## Plan`. Assert exact error fragments.

- [x] **Step 2: Test executable Todo placement and field parsing**

Create Markdown containing a root Todo, a child under `Children`, a task under `Completion Criteria`, and a task outside `Todos`. Assert only protocol Todos are in the executable graph, CSV dependencies are trimmed, Handoff and Reference values retain order, and marker characters map to the expected statuses.

- [x] **Step 3: Test invalid workspace manifest**

Create `.blueprint/manifest.md` with a different schema before `ensure_workspace()`. Assert `invalid Blueprint workspace manifest` and confirm the file is not overwritten.

- [x] **Step 4: Test ID, state, create, and read errors**

Assert invalid IDs (`bp-`, path separators, missing prefix) are rejected. Assert invalid states are rejected by `list` and `read_in`. Create `bp-01` twice and assert `already exists`; read `bp-missing` and assert `does not exist`.

- [x] **Step 5: Test move concurrency and filesystem lifecycle**

Create an active Blueprint, assert a stale ETag prevents `move_to`, and assert destination `active` is rejected. Move with the current ETag to `closed`; assert returned state/source, absence of `.blueprint/active/bp-01.md`, presence of `.blueprint/closed/bp-01.md`, and rejection of subsequent active writes.

- [x] **Step 6: Run source and store tests**

Run:

```bash
cargo test blueprint::tests::source -- --nocapture
cargo test blueprint::tests::store -- --nocapture
```

Expected: all source and store tests pass.

---

### Task 5: Review, Full Verification, and Coverage Comparison

**Files:**
- Modify only if review finds a test-quality issue: `src/blueprint/tests/{service,validate,source,store,mcp}.rs`
- Generate: `target/llvm-cov/html/index.html`

**Interfaces:**
- Consumes: all semantic tests from Tasks 1-4 and the existing MCP schema tests.
- Produces: reviewed tests, a clean full suite, and a refreshed coverage report.

- [x] **Step 1: Review tests for semantic value**

Inspect the diff using the rust-git-review checklist. Remove assertions that only prove Rust field assignment, merge redundant setup into helpers, and keep every error assertion tied to a protocol rule.

- [x] **Step 2: Run formatting, build, lint, and all tests**

Run:

```bash
cargo fmt --all -- --check
cargo check --all-targets
cargo clippy --all-targets -- -D warnings
cargo test --no-fail-fast
git diff --check
```

Expected: every command exits 0, with no warnings or failed tests.

- [x] **Step 3: Regenerate LLVM coverage**

Run:

```bash
cargo llvm-cov --html
```

Expected: `target/llvm-cov/html/index.html` is regenerated successfully.

- [x] **Step 4: Compare Blueprint coverage**

Record before/after function and line coverage for `blueprint/service.rs`, `validate.rs`, `source.rs`, `store.rs`, and `mcp.rs`. Explain any intentionally uncovered defensive, logging, startup, or forwarding lines rather than adding low-value tests.

- [x] **Step 5: Leave changes uncommitted for user review**

Report modified files, verification evidence, coverage deltas, and any implementation/design discrepancies. Do not stage or commit the test changes unless the user asks.
