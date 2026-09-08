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

Exclusions control automatic indexing and discovery, not file access. Scanning prunes
excluded directories: `archive/**` excludes the whole subtree, while
`archive/**/*.md` filters matching files. Known paths remain usable for content,
section, structure, stats, and outlink queries, and as backlink targets from visible
notes. For example, `read_note("archive/note.md")` and `[[archive/note#Summary]]`
can address excluded notes. Lists, searches, tags, categories, and graph discovery
remain limited to visible notes; short-name and alias lookup never scans excluded
folders. On-demand reads do not add files to the discovery index, and per-file size
limits still apply.

## Build

```sh
cargo check
cargo test
```

Install the CLI from this repository for the examples below:

```sh
cargo install --path .
```

## CLI quick start

Run the installed CLI from an Obsidian vault or any directory inside it.
`--vault` is optional: discovery starts at the process working directory and
walks upward to the nearest directory containing `.obsidian/`.
Start with the first page of notes, then move to precise reads and relation checks:

```sh
obsidian-vault-mcp list-notes --page 1
obsidian-vault-mcp get-note-outline "人物/林动.md" --page 1
obsidian-vault-mcp read-note "人物/林动.md#身体" --max-chars 4096
obsidian-vault-mcp get-note-structure "人物/林动.md"
obsidian-vault-mcp get-note-stats "人物/林动.md"
obsidian-vault-mcp resolve-ref '[[林动#身体]]'
obsidian-vault-mcp get-outlinks "人物/林动.md" --page 1
obsidian-vault-mcp get-backlinks '[[林动]]' --page 1
obsidian-vault-mcp get-note-neighborhood "林动" --depth 1 --direction both
obsidian-vault-mcp audit-links --page 1
obsidian-vault-mcp search-text "求生本能" --include "正文/**/*.md" --include "资料/**/*.md" --exclude "**/草稿/**" --page 1
obsidian-vault-mcp search-regex "林动.{0,20}代偿" --include "正文/**/*.md" --exclude "**/草稿/**" --page 1
obsidian-vault-mcp list-tags --page 1
obsidian-vault-mcp get-tag "状态/身体" --page 1
obsidian-vault-mcp list-categories --page 1
obsidian-vault-mcp get-category "人物" --page 1
obsidian-vault-mcp query-frontmatter phase --mode equals --value active --page 1
obsidian-vault-mcp append-section "人物/林动.md" "新增内容" --heading "身体"
obsidian-vault-mcp replace-section "人物/林动.md" "替换内容" --heading "身体"
obsidian-vault-mcp delete-section "人物/林动.md" --heading "旧设定"
obsidian-vault-mcp rename-heading "人物/林动.md" --old-heading "身体" --new-heading "身体状态"
```

Run the MCP server:

```sh
obsidian-vault-mcp serve
```

To explicitly select a vault from another directory, use
`obsidian-vault-mcp --vault /path/to/vault serve` or set
`OBSIDIAN_VAULT_MCP_ROOT`. Explicit configuration overrides discovery;
`--vault` takes precedence over the environment variable and accepts `~` and
`~/...` paths. Discovery searches ancestors only, not child directories or the
whole computer. If `serve` cannot discover a vault, it exposes file-local Markdown tools: `read_note`,
`get_note_outline`, `get_note_structure`, `get_note_stats`, `audit_links`, and
the structural section edit tools. Their `note` values are paths relative to
the current project directory; absolute paths and paths outside it are rejected.
In this mode, `audit_links` checks only standard Markdown relative links such
as `[text](../target.md)`, reporting targets whose files do not exist.
Vault-wide search, metadata, and link-graph tools remain unavailable. Other
commands still report an error, as does an invalid explicitly configured vault
path.

Common cache settings:

```sh
obsidian-vault-mcp \
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
`set_block_id`.
Section edits accept only heading and block-id selectors; line selectors remain
available for `read_note` only.

Prefer `get_note_outline` before `read_note`, then select a heading or line range.
`max_chars` is a Unicode-character budget: reading continues through the boundary
line, so the response may exceed it. Use `next_line` to continue without gaps.
Whole-note and line reads skip AST parsing; outlines and heading reads parse only
heading inline content after scanning block structure. Block-ID reads retain full
parsing. These paths still read the source file and enforce `max_note_bytes`.

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
- `set_block_id`

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

No vault argument is needed when the MCP client starts the server with its
working directory inside the target vault:

```json
{
  "context_servers": {
    "obsidian-vault": {
      "command": "/path/to/obsidian-vault-mcp",
      "args": ["serve"]
    }
  }
}
```

Discovery uses the server process working directory, not the executable's
location. Configure the client to launch it in the target vault or a subdirectory.
If the client launches it elsewhere, explicitly add `--vault` and the vault path
before `serve`, or set `OBSIDIAN_VAULT_MCP_ROOT`.

The server writes protocol data to stdout only. Logs stay on stderr.

## Observability

The server emits `tracing` logs to stderr and keeps stdout reserved for MCP
protocol data or CLI JSON. `--log-level` defaults to `debug`:

```sh
obsidian-vault-mcp --log-level debug serve
```

Lifecycle events let you correlate CLI and MCP work. `logging.initialized`
marks logging setup; each CLI command emits `cli.command.start` followed by
`cli.command.ok` or `cli.command.error`. Each MCP tool call retains its
`mcp.tool` span and `tool.call.*` events. Start events record an `input.preview`
of CLI and MCP arguments, capped at 1 KiB; `input.truncated=true` marks a clipped
value. Error events retain the complete error chain. Successful command and tool
output bodies are not copied into logs.

## Safety boundaries

This server has no database, vector index, file watcher, or persistent on-disk
cache. Default operation reads the vault; writes happen only through explicit
structural edit tools. Each tool call checks file metadata, and changed files
are reparsed before use.

Hidden paths and gitignored paths are excluded from visible notes by default, so
Obsidian settings, git data, agent skill files, and generated artifacts do not
enter normal note results.
