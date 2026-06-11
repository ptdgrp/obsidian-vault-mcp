# obsidian-vault-mcp

[中文 README](./README.zh-CN.md)

Read-only MCP server for Obsidian-style Markdown vaults.

This service is intentionally mechanical:

- reads the current vault from disk for each tool call
- does not maintain a database, vector index, watcher, or persistent cache
- keeps only an in-memory Markdown parse cache keyed by path, file size, and
  modified time
- expires parse cache entries after 10 minutes by default and caps the cache at
  1024 parsed notes
- does not write notes
- ignores hidden dot paths by default, such as `.obsidian/`, `.git/`, `.agents/`,
  and `.hidden.md`
- respects `.gitignore`, `.git/info/exclude`, and parent gitignore rules while
  scanning visible files
- returns source spans and heading sections so an LLM can cite evidence

It does not infer story domains such as "character", "organization", or
"chapter". File paths and Markdown headings are the source of meaning. The
server exposes those paths so an LLM can reason from evidence instead of
guessing.

## Build

```sh
cargo check
cargo test
```

## CLI

```sh
cargo run -- --vault /path/to/vault list-notes --json
cargo run -- --vault /path/to/vault list-vault-files --max-files 100 --json
cargo run -- --vault /path/to/vault --parse-cache-ttl-secs 600 --parse-cache-max-entries 1024 serve
cargo run -- --vault /path/to/vault parse-note "人物/林动.md" --json
cargo run -- --vault /path/to/vault get-note-outline "人物/林动.md" --json
cargo run -- --vault /path/to/vault resolve '[[林动#身体]]' --json
cargo run -- --vault /path/to/vault backlinks '[[林动]]' --json
cargo run -- --vault /path/to/vault search "求生本能" --json
cargo run -- --vault /path/to/vault search-regex "林动.{0,20}代偿" --path-glob "正文/**/*.md" --json
cargo run -- --vault /path/to/vault get-tags --tag "状态/身体" --json
cargo run -- --vault /path/to/vault query-frontmatter phase --mode equals --value active --json
cargo run -- --vault /path/to/vault query-frontmatter arc --mode regex --value "引擎.*" --json
cargo run -- --vault /path/to/vault read-section "人物/林动.md" --heading "身体" --json
cargo run -- --vault /path/to/vault find-unresolved-links --json
cargo run -- --vault /path/to/vault find-ambiguous-links --json
cargo run -- --vault /path/to/vault get-note-graph --json
cargo run -- --vault /path/to/vault get-graph-neighborhood "林动" --depth 1 --direction both --json
```

## Zed MCP

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

The server writes protocol data to stdout only. Logs must stay on stderr.

## Observability

The server emits `tracing` logs to stderr and keeps stdout reserved for MCP
protocol data or CLI JSON. Use `--log-level` to control verbosity:

```sh
cargo run -- --vault /path/to/vault --log-level info serve
```

OpenTelemetry export is opt-in. Set an OTLP HTTP endpoint with either
`--otel-endpoint` or `OTEL_EXPORTER_OTLP_ENDPOINT` to export both traces and
logs:

```sh
cargo run -- --vault /path/to/vault \
  --log-level info \
  --otel-endpoint http://localhost:4318 \
  serve
```

The service name defaults to `obsidian-vault-mcp`; override it with
`--otel-service-name` or `OTEL_SERVICE_NAME`.

Each MCP tool call creates an `mcp.tool` span with `tool.name`, duration, and
success/error events. The same tracing events are also exported as OTEL logs.
Tool arguments, note contents, query text, and regex patterns are not recorded
by default. On normal shutdown, the process force flushes pending spans and
logs, then waits up to 5 seconds for OTEL shutdown.

## Vault File List Semantics

`list_vault_files` is the recommended first tool call when an agent does not yet
know the vault layout.

The response is a flat file list:

- every returned item has a vault-relative `path`
- directories are not returned as standalone nodes
- Markdown files are returned when `include_files` is true
- non-Markdown files are counted as attachments; pass `include_attachments:
  true` to return attachment entries
- empty directories are omitted
- gitignore rules are applied during scanning

For example, this filesystem shape:

```text
资料库/技术设定/
  README.md
  001-发动机.md
```

is returned as file entries for `资料库/技术设定/README.md` and
`资料库/技术设定/001-发动机.md`. The path carries the directory context.

Use `include_readme_outline: true` only when a README's heading list is needed;
otherwise note entries include just their first heading. The default hides
attachment entries to keep tool output small.

## P0 Tools

See [docs/tools.md](docs/tools.md) for detailed input and output contracts.

- `list_notes`
- `list_vault_files`
- `read_note`
- `parse_note`
- `get_note_outline`
- `search_text`
- `search_regex`
- `resolve_ref`
- `get_outlinks`
- `get_backlinks`
- `get_tags` - body tag nodes and frontmatter `tag`/`tags`
- `query_frontmatter` - query a top-level frontmatter field by explicit
  `exists`, `equals`, or `regex` mode
- `collect_note_context`
- `collect_reference_context`
- `read_section`
- `find_unresolved_links`
- `find_ambiguous_links`
- `get_graph_neighborhood` - bounded graph context around one note; prefer this
  for normal agent use
- `get_note_graph` - full local-link graph for audit, visualization, or
  debugging

Every snippet-like result includes a `source` object with path, line range, and
nearest heading `section` when available. Byte offsets are internal only and are
not returned by MCP tools.

## Tool Use Guide

- Start with `list_vault_files` to understand the visible file paths.
- Use `list_notes` when only note paths and titles are needed.
- Use `get_note_outline` before `read_section` to select a heading without
  reading the whole note.
- Use `search_text` for literal recall and `search_regex` for structured phrase
  patterns such as chapter ranges, years, or recurring motifs.
- Use `get_tags` for both body tag nodes and frontmatter tags. Use
  `query_frontmatter` when the condition is a metadata field such as
  `phase: active`; choose `exists`, `equals`, or `regex` explicitly.
- Search tools return lightweight navigation results by default: path, line
  range, nearest section, and a short preview. They omit byte offsets and return
  only the matching line unless `context_lines` is explicitly set.
- Use `resolve_ref` before trusting an Obsidian reference target if ambiguity matters.
- Use `get_outlinks`, `get_backlinks`, and `get_graph_neighborhood` for focused
  local-link context. This includes
  Obsidian wikilinks and Markdown links whose relative path stays inside the
  vault.
- Use `get_note_graph` only when the full graph is explicitly needed for audit,
  visualization, debugging, or global health checks.
- Use `find_unresolved_links` and `find_ambiguous_links` as health checks before
  larger analysis.

## Safety Boundary

The server is read-only. It has no database, no vector index, no file watcher,
and no disk-backed cache. Each tool call checks current file metadata before
reusing a parsed Markdown document from memory, so changed files are reparsed.
Cached parse entries expire after `--parse-cache-ttl-secs` seconds and are also
bounded by `--parse-cache-max-entries`.
Hidden dot paths are ignored by default so Obsidian config, git data, and agent
skill files do not contaminate story context.
