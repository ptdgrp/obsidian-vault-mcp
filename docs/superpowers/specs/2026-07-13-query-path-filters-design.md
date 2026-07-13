# Query Path Filters Design

## Goal

Add request-level path filtering to `list_tags`, `get_tags`,
`list_categories`, `get_categories`, `search_text`, and `search_regex`.
Each tool accepts multiple include and exclude glob patterns so callers can
limit work to selected areas of a vault without changing server-wide vault
configuration.

## Public API

All six MCP tools gain two optional array fields:

- `include: string[]`: when non-empty, a note must match at least one pattern.
- `exclude: string[]`: a note matching any pattern is omitted.

Missing fields and empty arrays are equivalent and add no request-level
restriction. Patterns match vault-relative Markdown note paths using the
project's existing `globset` syntax.

The corresponding CLI commands gain repeatable `--include <GLOB>` and
`--exclude <GLOB>` arguments. This keeps CLI and MCP behavior aligned.

`search_regex.path_glob` is removed. Callers migrate a previous
`path_glob: "正文/**/*.md"` value to `include: ["正文/**/*.md"]`. No compatibility
alias is retained; this is an intentional breaking API change.

Response structures remain unchanged. The filters affect only which notes
contribute results.

## Filtering Semantics

Request-level filters operate only on notes already visible through the
server's vault configuration, gitignore rules, and default ignored-path rules.
They cannot re-include a globally hidden note.

For every visible note path:

1. If `include` is empty, the note passes the include phase.
2. Otherwise, it passes only if it matches at least one include pattern.
3. A note that matches any exclude pattern is rejected, regardless of include
   matches.

Thus multiple include patterns form a union, multiple exclude patterns form a
union, exclude takes precedence, and request-level rules intersect with the
global visible-note set.

## Architecture

Introduce a reusable query-layer `PathFilter`. It owns compiled include and
exclude `GlobSet` values and exposes a predicate over vault-relative paths.
Construction compiles every pattern once per request.

The query entry points accept include and exclude slices and construct the
filter before doing query-specific work. They obtain the globally visible note
list from `Vault::list_notes()`, then apply `PathFilter` before reading or
parsing files:

- Tag queries filter before parsing tag nodes and frontmatter tags.
- Category queries filter before deriving folder categories and building
  buckets.
- Text and regex searches filter before parallel file reads and line matching.

This keeps filtering semantics in one unit, avoids conflating per-request
filters with mutable vault configuration, and prevents work on files that
cannot contribute to the response.

## Errors

An invalid include or exclude glob fails the whole request. The error identifies
whether the invalid pattern came from `include` or `exclude` and includes the
offending pattern. Patterns are never silently ignored.

Other query errors preserve existing behavior.

## Testing

Implementation follows test-driven development. Tests first demonstrate the
new public behavior and fail because filtering is not yet supported.

Query-layer coverage includes:

- empty or omitted filters preserve current results;
- multiple include patterns use union semantics;
- excludes remove matching notes;
- excludes override includes;
- request filters cannot restore notes removed by global vault filters;
- invalid include and exclude patterns return contextual errors;
- tags, categories, literal search, and regex search all filter before
  aggregation or matching.

Server tests verify that all six MCP request schemas expose both arrays and
that handlers pass them to the query layer. CLI tests verify repeatable
`--include` and `--exclude` arguments for all corresponding commands.

Existing `path_glob` tests and documentation are migrated to `include`, and a
schema assertion confirms that `search_regex` no longer advertises
`path_glob`.

## Documentation

Update the English and Chinese READMEs and generated-style tool documentation
to describe the new arrays, matching precedence, vault-relative path syntax,
and the `search_regex.path_glob` migration.

## Scope

This change applies only to the six requested query tools and their CLI
counterparts. It does not add request-level filtering to unrelated tools,
change response schemas, or alter global vault visibility rules.
