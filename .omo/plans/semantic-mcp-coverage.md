# semantic-mcp-coverage - Work Plan

## TL;DR (For humans)
<!-- Fill this LAST, after the detailed plan below is written, so it summarizes the REAL plan. -->
<!-- Plain English for a non-engineer: NO file paths, NO todo numbers, NO wave/agent/tool names. -->

**What you'll get:** A stronger safety net around the ways people actually use the vault server: renaming notes and headings, invoking commands, making MCP requests, and reading generated tool documentation. The tests will measure meaningful outcomes in real temporary vaults.

**Why this approach:** The most valuable risk is unintended Markdown link changes during mutation, so it comes first. The remaining work follows the order you chose and avoids fragile assertions tied to internal code structure.

**What it will NOT do:** It will not change public tool or command behavior merely to make coverage rise. It will not add arbitrary coverage gates or tests that only know about private implementation details.

**Effort:** Medium
**Risk:** Low - work is isolated to test coverage unless a test discovers a real behavior defect.
**Decisions to sanity-check:** The coverage goal is behavioral confidence, not a fixed percentage.

Your next move: start work, or request a high-accuracy review of this plan. Full execution detail follows below.

---

> TL;DR (machine): Medium effort, low risk; behavioral coverage in user priority order: MCP mutations, CLI, MCP service, documentation.

## Scope
### Must have
 - Behavioral coverage for MCP mutations, especially heading and note renames across user-authored Obsidian link forms.
 - Behavioral CLI coverage for representative valid and invalid invocations.
 - Public MCP request/response boundary coverage, including error cases.
 - Public-schema documentation coverage after the preceding priorities.
### Must NOT have (guardrails, anti-slop, scope boundaries)
 - Do not test private helpers as a proxy for public behavior.
 - Do not alter tool contracts or introduce a coverage threshold simply to increase a percentage.

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: TDD + Rust built-in test framework, real temporary vault fixtures, `cargo llvm-cov`.
- Evidence: command output retained in the Codex run and, if needed, `.omo/evidence/` text summaries.

## Execution strategy
### Parallel execution waves
> Target 5-8 todos per wave. Fewer than 3 (except the final) means you under-split.

Wave 1 establishes the semantic MCP mutation contracts. Wave 2 validates CLI and MCP boundary behavior. Wave 3 covers documentation and runs the full verification set.

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1 | none | 2, 6 | none |
| 2 | 1 | 6 | 3 |
| 3 | none | 6 | 2 |
| 4 | none | 6 | 5 |
| 5 | none | 6 | 4 |
| 6 | 1-5 | final verification | none |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->
- [ ] 1. Lock heading-rename preview and apply contracts
  What to do / Must NOT do: Add real-vault tests proving a dry run preserves every file, an apply updates the target heading and only resolved references, and an absent heading returns an actionable error. Do not assert private edit ordering or helper calls.
  Parallelization: Wave 1 | Blocked by: none | Blocks: 2, 6
  References (executor has NO interview context - be exhaustive): src/mutation/rename.rs:1-305; src/mutation/tests/mod.rs:1-79; src/query/links.rs; src/resolver.rs
  Acceptance criteria (agent-executable): `cargo test mutation::tests` passes; test fixtures cover markdown link target, alias/display text, unresolved link, and dry-run non-mutation.
  QA scenarios (name the exact tool + invocation): happy: run the mutation test module on a vault containing references; failure: rename a missing heading and assert a user-visible error; Evidence `.omo/evidence/task-1-semantic-mcp-coverage.txt`.
  Commit: Y | test(mutation): cover user-visible rename outcomes

- [ ] 2. Lock note-rename reference migration contracts
  What to do / Must NOT do: Add behavioral tests for moving a note and preserving navigable resolved links across root-relative and sibling-relative references, while leaving external/unresolved text untouched. Do not make tests depend on filesystem traversal implementation.
  Parallelization: Wave 1 | Blocked by: 1 | Blocks: 6
  References (executor has NO interview context - be exhaustive): src/mutation/rename.rs:1-305; src/mutation/tests/mod.rs; src/resolver.rs; src/vault.rs
  Acceptance criteria (agent-executable): `cargo test mutation::tests` passes and every assertion is against final Markdown content or public mutation result fields.
  QA scenarios (name the exact tool + invocation): happy: move a note referenced by multiple valid forms; failure: attempt an invalid target path and assert no source files change; Evidence `.omo/evidence/task-2-semantic-mcp-coverage.txt`.
  Commit: Y | test(mutation): cover note rename link migration

- [ ] 3. Cover CLI commands as user invocations
  What to do / Must NOT do: Add tests that exercise CLI parsing and command dispatch for representative read/query and mutation commands, including invalid enum input. Prefer invoking the binary or parsing the public clap type; do not call parsing helpers directly unless no public surface can expose the behavior.
  Parallelization: Wave 2 | Blocked by: none | Blocks: 6
  References (executor has NO interview context - be exhaustive): src/main.rs:27-706; README.md:22-57; src/query.rs; src/mutation.rs
  Acceptance criteria (agent-executable): valid command output is machine-readable and invalid input exits with a clear user-facing diagnostic.
  QA scenarios (name the exact tool + invocation): happy: invoke a listed query command against a temporary vault; failure: supply an invalid direction/tag scope/frontmatter mode; Evidence `.omo/evidence/task-3-semantic-mcp-coverage.txt`.
  Commit: Y | test(cli): cover command contracts

- [ ] 4. Drive MCP tool requests through their service boundary
  What to do / Must NOT do: Add request-level tests for representative tool success, malformed selector input, unknown note, and mutation failure. Assert serialized tool response or error semantics, not service-private closures.
  Parallelization: Wave 2 | Blocked by: none | Blocks: 6
  References (executor has NO interview context - be exhaustive): src/server.rs:1-790; src/query/tests/mod.rs:1-700; src/mutation/tests/mod.rs
  Acceptance criteria (agent-executable): `cargo test server::tests` proves successful request mapping and distinct client error cases.
  QA scenarios (name the exact tool + invocation): happy: request a read/query tool against a real vault; failure: malformed line reference and missing note return clear errors; Evidence `.omo/evidence/task-4-semantic-mcp-coverage.txt`.
  Commit: Y | test(server): cover MCP request outcomes

- [ ] 5. Verify generated tool documentation reflects the public contract
  What to do / Must NOT do: Add tests around the public documentation generation output covering object fields, nested references, enum/union descriptions, and stable escaping. Do not snapshot the entire generated document.
  Parallelization: Wave 2 | Blocked by: none | Blocks: 6
  References (executor has NO interview context - be exhaustive): src/docs.rs:1-331; src/server.rs; Cargo.toml schemars dependency
  Acceptance criteria (agent-executable): `cargo test docs::tests` passes with focused assertions for the documented schema semantics.
  QA scenarios (name the exact tool + invocation): happy: document a schema with nested definitions; failure: unusual Markdown table characters are escaped without changing semantic type text; Evidence `.omo/evidence/task-5-semantic-mcp-coverage.txt`.
  Commit: Y | test(docs): cover generated schema documentation

- [ ] 6. Measure behavior coverage and verify the real surfaces
  What to do / Must NOT do: Format code, run targeted and complete tests, run clippy without warnings, calculate coverage, and manually drive at least one CLI and one mutation scenario. Do not claim success solely because the percentage rises.
  Parallelization: Wave 3 | Blocked by: 1-5 | Blocks: final verification
  References (executor has NO interview context - be exhaustive): Cargo.toml; README.md; all test modules changed by Todos 1-5
  Acceptance criteria (agent-executable): `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`, and `cargo llvm-cov --workspace --all-features` all exit 0; report before/after line coverage.
  QA scenarios (name the exact tool + invocation): happy: run a documented CLI query against a fixture vault; failure: run an invalid/missing-note operation and observe an error; Evidence `.omo/evidence/task-6-semantic-mcp-coverage.txt`.
  Commit: Y | test: expand semantic coverage

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance audit
- [ ] F2. Code quality review
- [ ] F3. Real manual QA
- [ ] F4. Scope fidelity

## Commit strategy
Create one or more focused `test(...)` commits by surface after all validation passes; do not include generated coverage artifacts.

## Success criteria
 - MCP mutation scenarios verify preview, apply, matching references, and failure safety against a real vault.
 - CLI and MCP boundary tests assert observable output/error behavior.
 - Documentation tests assert schema meaning rather than implementation traversal.
 - The complete test, lint, format, coverage, and manual surface checks pass.
