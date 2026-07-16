# obsidian-vault-mcp

[中文 README](./README.zh-CN.md)

MCP server for Obsidian-style Markdown vaults with compact, task-oriented tools
and structural section edits.

The server is deliberately mechanical:

- reads the current vault from disk for each tool call
- keeps only an in-memory Markdown parse cache keyed by path, file size, and
  modified time
- expires parse cache entries after 10 minutes by default and caps the cache at
  1024 parsed notes
- edits notes only through explicit section operations; each write is atomic
- ignores hidden dot paths by default, such as `.obsidian/`, `.git/`,
  `.agents/`, and `.hidden.md`
- respects `.gitignore`, `.git/info/exclude`, and parent gitignore rules while
  scanning visible Markdown notes
- respects Obsidian's `.obsidian/app.json` `userIgnoreFilters` while scanning
  visible Markdown notes; exact-path reads remain available
- returns vault-relative paths, Obsidian-style line references, and nearest
  headings so an LLM can cite evidence

It does not infer story domains such as "character", "organization", or
"chapter". File paths, Markdown headings, local links, tags, and frontmatter are
the source of meaning. The server exposes those facts so an LLM can reason from
evidence instead of guessing.

## Build

```sh
cargo check
cargo test
```

## CLI quick start

Start with the first page of notes, then move from broad navigation to precise
reads and relation checks:

```sh
cargo run -- --vault /path/to/vault list-notes --page 1
cargo run -- --vault /path/to/vault get-note-outline "人物/林动.md" --page 1
cargo run -- --vault /path/to/vault read-note "人物/林动.md#身体" --max-chars 4096
cargo run -- --vault /path/to/vault get-note-structure "人物/林动.md"
cargo run -- --vault /path/to/vault get-note-stats "人物/林动.md"
cargo run -- --vault /path/to/vault resolve-ref '[[林动#身体]]'
cargo run -- --vault /path/to/vault get-outlinks "人物/林动.md" --page 1
cargo run -- --vault /path/to/vault get-backlinks '[[林动]]' --page 1
cargo run -- --vault /path/to/vault get-note-neighborhood "林动" --depth 1 --direction both
cargo run -- --vault /path/to/vault audit-links --page 1
cargo run -- --vault /path/to/vault search-text "求生本能" --include "正文/**/*.md" --include "资料/**/*.md" --exclude "**/草稿/**" --page 1
cargo run -- --vault /path/to/vault search-regex "林动.{0,20}代偿" --include "正文/**/*.md" --exclude "**/草稿/**" --page 1
cargo run -- --vault /path/to/vault list-tags --page 1
cargo run -- --vault /path/to/vault get-tag "状态/身体" --page 1
cargo run -- --vault /path/to/vault list-categories --page 1
cargo run -- --vault /path/to/vault get-category "人物" --page 1
cargo run -- --vault /path/to/vault query-frontmatter phase --mode equals --value active --page 1
cargo run -- --vault /path/to/vault append-section "人物/林动.md" "新增内容" --heading "身体"
cargo run -- --vault /path/to/vault replace-section "人物/林动.md" "替换内容" --heading "身体"
cargo run -- --vault /path/to/vault delete-section "人物/林动.md" --heading "旧设定"
cargo run -- --vault /path/to/vault rename-heading "人物/林动.md" --old-heading "身体" --new-heading "身体状态"
```

Run the MCP server:

```sh
cargo run -- --vault /path/to/vault serve
```

Common cache settings:

```sh
cargo run -- --vault /path/to/vault \
  --parse-cache-ttl-secs 600 \
  --parse-cache-max-entries 1024 \
  serve
```

## Four-layer tool model

The public tools are organized around the question an agent is trying to answer:

1. Notes enumeration: `list_notes` pages through visible Markdown notes. Use it
   first when you need candidate paths, titles, and sizes.
2. Internal note structure: `get_note_outline`, `read_note`,
   `get_note_structure`, and `get_note_stats` inspect one note or one selected
   heading, block, or line range.
3. External note relations: `resolve_ref`, `get_outlinks`, `get_backlinks`,
   `get_note_neighborhood`, `list_tags`, `get_tag`, `list_categories`,
   `get_category`, `query_frontmatter`, `search_text`, and `search_regex` answer
   focused cross-note questions.
4. Relation audit: `audit_links` reports unresolved and ambiguous local links
   across visible notes so broad work can start from a known link-health state.

Editing tools use the same structural selectors: `append_section`,
`replace_section`, `delete_section`, `rename_heading`, `rename_note`, and
`rename_block_id`.

## Pagination and filters

Paged tools accept a fixed numeric `page` value. Page numbers are one-based;
`page: 1` is the first page. There are no cursors or opaque tokens. If `page`
is greater than the available page count, the response keeps the requested page
number and returns an empty result list with pagination totals.

Request filters use vault-relative Markdown note paths. An empty or omitted
`include` array leaves the request unrestricted; otherwise a note must match at
least one include pattern. Multiple includes are a union, multiple excludes are
a union, and an exclude match always wins.

`get_note_neighborhood` is intentionally not paged. It returns a bounded
resolved-link neighborhood controlled by `depth` and `direction`, so narrow the
target or depth when the result is too broad. Use `audit_links --page 1` when
you need a paged relation-health sweep.

## References and paths

A path is a vault-relative Markdown note path such as `人物/林动.md`. It names a
file directly and is the clearest choice once `list_notes` or another tool has
returned a path.

A reference is an Obsidian-style note target such as `[[林动#身体]]`,
`林动#身体`, or `人物/林动.md#L1-L20`. References may point at a note, heading,
block id, or line range. Use `resolve_ref` when a human-facing reference must be
checked before reading or following links.

Ambiguous references are not guessed. The tools either require a uniquely
resolved target or report the ambiguity so the caller can pick a concrete path
or selector.

## Tool contracts

See [docs/tools.md](docs/tools.md) for the generated MCP input and output
contracts.

Primary read/query tools:

- `list_notes`
- `get_note_outline`
- `read_note`
- `get_note_structure`
- `get_note_stats`
- `resolve_ref`
- `get_outlinks`
- `get_backlinks`
- `get_note_neighborhood`
- `audit_links`
- `search_text`
- `search_regex`
- `list_tags`
- `get_tag`
- `list_categories`
- `get_category`
- `query_frontmatter`

Primary edit tools:

- `append_section`
- `replace_section`
- `delete_section`
- `rename_heading`
- `rename_note`
- `rename_block_id`

## Recommended workflow

1. Start with `list_notes --page 1`.
2. Use path filters when the question belongs to a known folder or subset.
3. Use `get_note_outline` to choose a section before reading.
4. Use `read_note` with a path, heading, block id, or line range selector for
   the smallest useful evidence span.
5. Use `resolve_ref`, `get_outlinks`, `get_backlinks`, or
   `get_note_neighborhood` for explicit local-link questions.
6. Use `search_text`, `search_regex`, tags, categories, and frontmatter queries
   for recall questions that are not already anchored to one note.
7. Use `audit_links` before large refactors or audits that depend on link
   reliability.

## MCP configuration

```json
{
  "context_servers": {
    "obsidian-vault": {
      "command": "/path/to/obsidian-vault-mcp",
      "args": ["--vault", "/path/to/vault", "serve"]
    }
  }
}
```

The server writes protocol data to stdout only. Logs stay on stderr.

## Observability

The server emits `tracing` logs to stderr and keeps stdout reserved for MCP
protocol data or CLI JSON. `--log-level` defaults to `debug` and controls the
level for stderr logs, OTEL traces, and OTEL logs together:

```sh
cargo run -- --vault /path/to/vault --log-level debug serve
```

OpenTelemetry export is opt-in. Set an OTLP HTTP endpoint with either
`--otel-endpoint` or `OTEL_EXPORTER_OTLP_ENDPOINT` to export both traces and
logs. The endpoint is a base URL: the server sends traces to `/v1/traces` and
logs to `/v1/logs`:

```sh
cargo run -- --vault /path/to/vault \
  --log-level debug \
  --otel-endpoint http://10.5.11.4:11418 \
  serve
```

The service name defaults to `obsidian-vault-mcp`; override it with
`--otel-service-name` or `OTEL_SERVICE_NAME`.

Lifecycle events let you correlate CLI and MCP work. `telemetry.initialized`
marks telemetry setup; each CLI command emits `cli.command.start` followed by
`cli.command.ok` or `cli.command.error`. Each MCP tool call retains its
`mcp.tool` span and `tool.call.*` events. The same tracing events are also
exported as OTEL logs. Tool arguments, note contents, query text, regex
patterns, and endpoint values are not recorded in lifecycle fields; a
`cli.command.error` retains the full error text for diagnosis.

CLI completion and MCP shutdown force-flush pending OTEL logs and traces before
the process exits. In Grafana Loki, query this service with:

```logql
{service_name="obsidian-vault-mcp"}
```

## Safety boundaries

This server has no database, vector index, file watcher, or persistent on-disk
cache. Default operation reads the vault; writes happen only through explicit
structural edit tools. Each tool call checks file metadata, and changed files
are reparsed before use.

Hidden paths and gitignored paths are excluded from visible notes by default, so
Obsidian settings, git data, agent skill files, and generated artifacts do not
enter normal note results.
