# Blueprint MCP v0.1 Design

## Goal

Implement the complete Blueprint v0.1 protocol as MCP tools for a local Markdown workspace. Blueprint files are the only source of truth; Git preserves history outside the protocol.

## Scope

The implementation covers the workspace layout, Blueprint lifecycle, Definition of Done updates, Todo graph lifecycle, derived execution state, structural validation, ETag concurrency control, per-file locking, atomic writes, and all 21 tools named in the v0.1 design draft.

All Blueprint-specific production code lives under `src/blueprint`. The existing MCP server only defines request schemas and delegates calls to the Blueprint service. The existing `blueprint` CLI placeholder becomes a safe workspace-initialization command.

## Architecture

### Source-preserving document model

`src/blueprint/document.rs` scans Markdown headings and list structure into a source-span document model. It locates fixed H2 sections, tasks, Block IDs, Todo field blocks, and `Children` nesting. The model records exact byte ranges and indentation, then applies a minimal replacement only to the node or section being changed.

This is deliberately not a parse-and-reformat pipeline. It preserves unknown sections and fields, HTML comments, wiki links, callouts, ordinary prose, and existing layout. The scanner recognizes the Markdown structures required by Blueprint while keeping all unrelated source text opaque.

### Domain model and validation

`src/blueprint/model.rs` defines serializable request-facing models: workspace and Blueprint states, parsed Blueprint fields, DoD entries, Todo entries, Todo status, readiness, lifecycle results, and resume/full views.

`src/blueprint/validate.rs` validates every read and every candidate write. Validation requires the eight fixed sections, a legal `bp-*.md` filename and directory lifecycle state, `Created By`, unique correctly prefixed DoD and Todo Block IDs, valid dependency references, no self-dependency or dependency cycle, valid `Children` containment, and each status-specific invariant.

The service derives execution state at read time. Pending Todos are ready only when every dependency is completed. Dependencies that are blocked or cancelled keep dependents pending and not ready. DoD and Todo state remain independent.

### Storage and concurrency

`src/blueprint/store.rs` owns all filesystem behavior. It creates and discovers `.blueprint` workspaces, reads Blueprint files, calculates a file-content ETag, serializes writes with a per-Blueprint lock in `.blueprint/.locks`, re-reads under the lock, checks `expected_etag` when supplied, writes through `.blueprint/.tmp`, and atomically replaces the target file.

Lifecycle transitions use an atomic rename between `active`, `closed`, and `cancelled` after the document update has been validated and written. Different Blueprint IDs use separate locks.

### Service and MCP boundary

`src/blueprint/service.rs` exposes domain operations for all tools:

- Workspace: `blueprint_init`, `blueprint_discover`.
- Blueprint: `blueprint_create`, `blueprint_get`, `blueprint_list`, `blueprint_update`, `blueprint_status`, `blueprint_close`, `blueprint_cancel`.
- DoD: `dod_update`.
- Todo: `todo_create`, `todo_get`, `todo_list`, `todo_update`, `todo_assign`, `todo_start`, `todo_complete`, `todo_block`, `todo_cancel`.

`src/server.rs` adds `schemars` request types and thin tool handlers. It has no Markdown editing or graph logic. `src/main.rs` constructs the service and invokes `blueprint_init` for the existing `blueprint` command.

## Behavioral details

### IDs and files

Generated Blueprint, Todo, and DoD IDs use time-sortable ULID-compatible identifiers with `bp-`, `todo-`, and `dod-` prefixes. IDs are unique within a `.blueprint` workspace. A Blueprint is stored only as `.blueprint/{active,closed,cancelled}/bp-<id>.md`; the directory is its lifecycle state.

`blueprint_init` creates `manifest.md`, `active`, `closed`, `cancelled`, `.locks`, and `.tmp`. The manifest contains only the specified `blueprint/v1` frontmatter and workspace H1. `blueprint_discover` searches from a supplied vault-relative directory upward until it finds a valid workspace.

### Mutations

Normal Blueprint updates can change title, Intent, Constraints, Plan, Results, and Notes. They cannot change Record fields or Todo status. Only dedicated lifecycle methods modify `Closed By`, `Cancelled By`, `Completed By`, checkbox statuses, or lifecycle directories.

Todo mutations enforce the state machine from the draft. Starting requires an owner and completed dependencies. Completing requires an in-progress Todo, every completion criterion checked, all non-cancelled children completed, and a non-empty result summary. Blocking requires both a reason and a handoff. Cancelling requires a reason. Reassigning an in-progress or blocked Todo requires an existing handoff.

Closing always succeeds once supplied with `closed_by`; an incomplete close additionally requires a reason and records open DoD/Todos in `Results`. Cancelling records the reason and actor, updates Record, appends cancellation output, and moves the file to `cancelled`.

### Returned views

Every file read returns an ETag and derived execution state. `blueprint_get(view: resume)` limits the document payload to Intent, Constraints, open DoD, Plan, in-progress/ready/blocked Todos with handoffs, and current Results. Status returns counts, ready and not-ready Todos with unsatisfied dependencies, blocked/unassigned Todos, and open Todos.

## Error handling

Malformed files produce actionable validation errors and are never partially rewritten. A stale `expected_etag` is rejected after lock acquisition and fresh re-read. Invalid lifecycle, state transition, field mutation, dependency, or parent relationship is rejected before writing. I/O and lock errors are surfaced through existing MCP error handling.

## Test strategy

Tests under `src/blueprint/tests/` will cover:

- initialization, discovery, manifest validation, and Blueprint creation;
- complete parsing and source-preserving targeted edits with unknown Markdown retained;
- every structural and status invariant, duplicate IDs, illegal Block ID placement, parent cycles, dependency cycles, and missing references;
- readiness and resume/status derivation, including blocked and cancelled dependencies;
- every lifecycle tool and required Record/Results output;
- optimistic ETag conflict, independent Blueprint lock paths, and atomic storage behavior;
- MCP schemas/dispatch and the CLI initialization command.

The docs generator test will ensure `docs/tools.md` accurately reflects the expanded tool set.

## Explicit non-goals

Blueprint does not maintain revision history, persist execution state or readiness, automatically synchronize DoD and Todo checkboxes, interpret arbitrary user Markdown beyond the protocol structures, or rewrite unrelated Markdown formatting.
