---
slug: semantic-mcp-coverage
status: planned
intent: clear
pending-action: write .omo/plans/semantic-mcp-coverage.md
approach: Add black-box Rust tests in priority order: MCP mutation tools, CLI, MCP request boundary, then generated documentation; remeasure with cargo llvm-cov.
---

# Draft: semantic-mcp-coverage

## Components (topology ledger)
<!-- Lock the SHAPE before depth. One row per top-level component that can succeed or fail independently. -->
<!-- id | outcome (one line) | status: active|deferred | evidence path -->
| mutation-tools | Users can safely preview and apply renames across real Markdown vaults | active | src/mutation/rename.rs, src/mutation/tests/mod.rs |
| cli | Users can invoke supported vault operations through parsed command-line input | active | src/main.rs:27-706 |
| mcp-service | MCP clients receive valid responses and meaningful errors at the tool boundary | active | src/server.rs:1-790 |
| generated-docs | Tool documentation describes the public schema accurately | active | src/docs.rs:1-331 |

## Open assumptions (announced defaults)
<!-- Record any default you adopt instead of asking, so the user can veto it at the gate. -->
<!-- assumption | adopted default | rationale | reversible? -->
| Coverage target | Improve coverage through behavioral contracts, with no arbitrary percentage threshold | A numeric threshold would incentivize implementation-coupled tests | yes |
| Test level | Use real temporary vaults and public tool/request types before private helper tests | These test user-visible behavior and survive refactors | yes |

## Findings (cited - path:lines)
 - `cargo llvm-cov --workspace --all-features` reports 62.03% line coverage; the largest uncovered user-facing surfaces are `src/server.rs` (17.05%), `src/mutation/rename.rs` (41.09%), `src/main.rs` (0%), and `src/docs.rs` (0%).
 - Existing mutation tests in `src/mutation/tests/mod.rs:25-79` only prove one heading rename reference form and do not exercise distinct link spellings or no-op/error outcomes.
 - Existing service tests in `src/server.rs:786-847` validate schemas plus line-selector parsing, but do not drive tool responses or error translation.

## Decisions (with rationale)
 - Test in user priority order: MCP mutation tools, CLI, MCP service, then documentation.
 - Treat each test name and assertion as a semantic contract: preview means no write, applying a rename updates only resolvable targets, invalid client input receives a usable error, and generated docs retain public schema meaning.
 - Use TDD: introduce each behavior test, observe the meaningful failure where a behavior is absent or wrong, then retain production behavior unchanged unless the test reveals a defect.

## Scope IN
 - New Rust tests and any minimal production correction uncovered by those tests.
 - Coverage measurement, formatting, clippy, and black-box command/request smoke checks.

## Scope OUT (Must NOT have)
 - Artificial coverage-only assertions against private implementation details.
 - Changing MCP tool schemas, CLI flags, or document format solely to make tests easier.
 - Adding a coverage threshold gate without an agreed project policy.

## Open questions
None. The user set the test priority and semantic testing constraint.

## Approval gate
 status: approved-scope
<!-- When exploration is exhausted and unknowns are answered, set status: awaiting-approval. -->
<!-- That durable record is the loop guard: on a later turn read it and resume at the gate instead of re-running exploration. -->
