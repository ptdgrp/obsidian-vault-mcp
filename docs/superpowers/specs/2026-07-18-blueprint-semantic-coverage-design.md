# Blueprint Semantic Coverage Design

## Goal

Increase Blueprint protocol coverage with tests that prove behavior from the original v0.1 design. Coverage percentage is a diagnostic signal, not the acceptance criterion. Tests must exercise protocol rules, state transitions, derived state, preservation guarantees, or error handling.

## Scope

The work targets `src/blueprint/` and its existing test modules. Production behavior is not intentionally changed. If a semantic test exposes a mismatch between the implementation and the v0.1 design, stop on that case, document the discrepancy, and decide whether the implementation or the expected behavior should change before proceeding.

The unrelated untracked `src/blueprint/todo.rs` file is outside this work.

## Test Strategy

Use service-level tests as the primary surface because they exercise parsing, validation, storage, lifecycle movement, and ETag-protected writes together. Use focused unit tests for validation and parsing rules whose failure should be isolated precisely. Add MCP tests only where they prove the public tool contract or actual routing behavior; do not invoke every thin forwarding handler solely to color coverage lines.

Prefer reusable scenario builders for valid Blueprint and Todo setup. Each test name must state a protocol rule, and each assertion must observe persisted Markdown, returned protocol state, or a meaningful rejection.

## Semantic Matrix

### Blueprint lifecycle and views

- Full and resume views return the correct source or recovery context.
- Status derives ready, not-ready, blocked, unassigned, open Todo, and open DoD groups from Markdown.
- Complete close records a complete closure and moves the file to `closed/`.
- Incomplete close requires a reason and records remaining DoD and Todo IDs.
- Cancel records actor and reason and moves the file to `cancelled/`.
- Blueprint updates validate non-empty semantic fields and preserve unrelated Markdown.

### Todo lifecycle

- Assignment trims and persists owners; completed or cancelled Todos cannot be assigned.
- Reassignment of in-progress or blocked work requires Handoff when the owner changes.
- Start requires `pending`, an Owner, and completed dependencies.
- Block requires `in_progress`, a non-empty reason, and non-empty Handoff.
- Cancel accepts only pending, in-progress, or blocked Todos and requires a reason.
- Complete requires checked criteria, completed-or-cancelled children, a non-empty summary, and an eligible state.
- Todo update changes only allowed fields and validates a changed dependency graph.
- Todo list combines status, owner, and derived readiness filters.
- DoD update changes only the selected item and preserves or writes its note.

### Validation and parsing

- Reject self-dependencies, unknown dependencies, cycles, and duplicate Todo IDs.
- Enforce state invariants for Owner, Completed By, Result Summary, Completion Criteria, open children, Block Reason, Handoff, and Cancel Reason.
- Reject invalid Blueprint filenames, missing or duplicated required sections, and malformed protocol task placement.
- Continue excluding Completion Criteria tasks from the executable Todo graph.

### Storage and concurrency

- Reject invalid Blueprint IDs and lifecycle states.
- Reject duplicate creation and missing reads.
- Enforce stale ETags for writes and moves.
- Move files between lifecycle directories without leaving the active source behind.
- Reject an invalid existing workspace manifest.

## Coverage Use

After the semantic matrix passes, regenerate the LLVM coverage report and compare Blueprint module function and line coverage. Remaining uncovered lines are acceptable when they are defensive filesystem failures, process-level stdin/stdout server startup, logging-only branches, or trivial forwarding code with no independent contract.

## Verification

Run focused tests while adding each behavior, then run:

```text
cargo fmt --all -- --check
cargo check --all-targets
cargo clippy --all-targets -- -D warnings
cargo test --no-fail-fast
cargo llvm-cov --html
```

Success means the semantic matrix is represented by passing tests, no low-value assertions were introduced, the full suite remains green, and the refreshed report shows the expected Blueprint coverage improvement.
