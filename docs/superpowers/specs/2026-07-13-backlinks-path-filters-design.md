# Backlinks path filters design

## Goal

Allow `get_backlinks` callers to restrict returned backlinks by the
vault-relative paths of the source notes containing those links.

## Interface

Add optional `include: string[]` and `exclude: string[]` fields to the MCP
request and repeatable `--include` / `--exclude` flags to the CLI command.
The names, glob syntax, defaults, validation errors, and precedence match the
existing query path-filter interface.

## Behavior

Resolve the requested backlink target exactly as today. Construct a
`PathFilter` from the request values, then apply it to each indexed source note
before examining that note's links. Thus filters never change which target is
resolved and only control which source-note backlink occurrences can appear in
the result. An empty include list permits every source path; matching excludes
win over includes; invalid glob errors retain their include/exclude context.

Both compact and verbose backlink outputs use the same filtered result. The
existing sort order and post-filter `max_results` truncation remain unchanged.

## Verification

Add query, MCP-dispatch, CLI, and generated-schema tests that show include
selects matching source paths, exclude overrides include, invalid globs are
reported, and truncation is evaluated after filtering. Run formatting, query
tests, CLI tests, docs check, and the full Cargo test suite.
