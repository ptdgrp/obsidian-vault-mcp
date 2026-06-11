# MCP Tools

This document describes the public MCP tool contract exposed by
`obsidian-vault-mcp`. All paths are vault-relative unless stated otherwise. The
server is read-only.

## Tool Index

| Symbol | Tool | Use |
| --- | --- | --- |
| 🗒️ | `list_notes` | Lightweight note paths and titles. |
| 🗂️ | `list_vault_files` | Flat visible vault file listing. |
| 📖 | `read_note` | Full note body. |
| 🧩 | `parse_note` | Parsed Markdown and Obsidian structures. |
| 🌲 | `get_note_outline` | Heading tree before section reads. |
| 🔎 | `search_text` | Literal text search. |
| .* | `search_regex` | Regex search with optional path glob. |
| 🧭 | `resolve_ref` | Resolve notes, headings, and blocks. |
| ↗ | `get_outlinks` | Outgoing local links. |
| ↩ | `get_backlinks` | Incoming local links. |
| # | `get_tags` | Body and frontmatter tags. |
| 🧾 | `query_frontmatter` | Query YAML/frontmatter fields. |
| 🧵 | `collect_note_context` | Current note plus link context. |
| 🔗 | `collect_reference_context` | Resolve reference then collect context. |
| ✂ | `read_section` | Exact heading, block, or line-range content. |
| ? | `find_unresolved_links` | Links with no visible target. |
| ! | `find_ambiguous_links` | Links with multiple targets. |
| 🕸️ | `get_graph_neighborhood` | Bounded local-link graph. |
| 🗺️ | `get_vault_graph` | Full local-link graph. |

## Common Types

### Empty Input

Tools that operate on the whole vault accept an empty object:

```json
{}
```

### Source

Snippet-like results include a `source` object:

| Field | Type | Description |
| --- | --- | --- |
| `path` | string | Vault-relative note path. |
| `line_start` | integer | 1-based start line. |
| `line_end` | integer | 1-based end line. |
| `section` | object \| null | Nearest containing heading, when available. |

`section` has:

| Field | Type | Description |
| --- | --- | --- |
| `heading` | string | Heading text. |
| `heading_level` | integer | Markdown heading level. |
| `heading_path` | string[] | Heading breadcrumb from outer to inner heading. |

### Resolve Summary

Many link results include a compact resolution:

```json
{ "status": "resolved", "path": "Note.md", "heading": null, "block_id": null }
```

Variants:

| Status | Fields |
| --- | --- |
| `resolved` | `path`, optional `heading`, optional `block_id` |
| `ambiguous` | `candidates: [{ path, match_kind }]` |
| `unresolved` | no extra fields |

### Link Evidence

| Field | Type | Description |
| --- | --- | --- |
| `source` | Source | Where the link appears. |
| `target` | string | Link target text/path. |
| `alias` | string \| null | Display text when present. |
| `resolved` | Resolve Summary | Resolution status for the target. |
| `snippet` | string | Source snippet; omitted when empty. |

### Text Match

| Field | Type | Description |
| --- | --- | --- |
| `source` | Source | Match location. |
| `snippet` | string | Short preview. Use `read_section` for full evidence. |

## 🗒️ list_notes

List visible Markdown notes with path, first heading title, size, and modified
time.

Input:

```json
{}
```

Output:

| Field | Type | Description |
| --- | --- | --- |
| `notes` | NoteSummary[] | Visible Markdown notes. |

`NoteSummary`:

| Field | Type | Description |
| --- | --- | --- |
| `path` | string | Vault-relative note path. |
| `title` | string \| null | First Markdown heading, when present. |

## 🗂️ list_vault_files

List gitignore-aware visible vault files as flat entries with paths. This is not
a nested directory tree; group by `path` prefix if a tree-shaped view is needed.

Input:

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `include_files` | boolean | `true` | Include Markdown notes. |
| `include_attachments` | boolean | `false` | Include non-Markdown files. |
| `include_readme_outline` | boolean | `false` | Include README heading titles. |
| `max_files` | integer | `100` | Maximum returned file entries. |

Compatibility: `max_children_per_dir` is accepted as an alias for `max_files`.

Output:

| Field | Type | Description |
| --- | --- | --- |
| `summary` | VaultFilesSummary | Whole-vault counts after ignore/exclude filtering. |
| `files` | VaultFile[] | Visible files sorted by natural vault-relative path order. |
| `truncated_files` | integer | Files omitted because `max_files` was reached. |

`VaultFilesSummary`:

| Field | Type | Description |
| --- | --- | --- |
| `notes` | integer | Markdown file count. |
| `directories` | integer | Unique parent directories containing returned files. |
| `attachments` | integer | Non-Markdown file count. |
| `empty_directories` | integer | Always `0` for the flat file list. |

`VaultFile`:

| Field | Type | Description |
| --- | --- | --- |
| `path` | string | Vault-relative file path. |
| `kind` | `"note"` \| `"attachment"` | File kind. |
| `title` | string \| null | First Markdown heading for notes. |
| `outline` | string[] \| null | README heading titles when requested. |
| `size` | string | Human-readable file size. |
| `modified` | string \| null | Last modified time in the current system timezone. |

## 📖 read_note

Read one Markdown note body by path, stem, or alias.

Input:

| Field | Type | Description |
| --- | --- | --- |
| `note` | string | Vault-relative path, note stem, or alias. |

Output:

| Field | Type | Description |
| --- | --- | --- |
| `path` | string | Resolved vault-relative note path. |
| `content` | string | Note body, possibly truncated by server limits. |
| `truncated` | boolean | Whether content was truncated. |

## 🧩 parse_note

Parse one Markdown note into compact headings, local links, embeds, tags, block
ids, and frontmatter.

Input:

| Field | Type | Description |
| --- | --- | --- |
| `note` | string | Vault-relative path, note stem, or alias. |

Output:

| Field | Type | Description |
| --- | --- | --- |
| `path` | string | Resolved note path. |
| `frontmatter` | JSON object \| null | Parsed YAML frontmatter when present. |
| `headings` | CompactHeadingInfo[] | Markdown headings with line numbers. |
| `links` | CompactLinkInfo[] | Obsidian wikilinks and safe in-vault Markdown links. |
| `embeds` | CompactEmbedInfo[] | Obsidian embeds. |
| `tags` | CompactTagInfo[] | Body tags. |
| `blocks` | CompactBlockInfo[] | Block ids. |

Important nested shapes:

| Type | Fields |
| --- | --- |
| `CompactHeadingInfo` | `text`, `level`, `path`, `line` |
| `CompactLinkInfo` | `target`, `alias`, `reference`, `kind`, `line`, `section` |
| `CompactEmbedInfo` | `target`, `reference`, `line`, `section` |
| `CompactTagInfo` | `tag`, `line`, `section` |
| `CompactBlockInfo` | `id`, `line`, `section` |
| `ReferenceInfo` | tagged by `kind`: `heading { value }`, `multi_heading { value: string[] }`, or `block_id { value }` |

`kind` for links is `wikilink` or `markdown`.

## 🌲 get_note_outline

Return one note's heading tree without body text.

Input:

| Field | Type | Description |
| --- | --- | --- |
| `note` | string | Vault-relative path, note stem, or alias. |

Output:

| Field | Type | Description |
| --- | --- | --- |
| `note` | string | Resolved note path. |
| `outline` | OutlineNode[] | Heading tree. |

`OutlineNode`:

| Field | Type | Description |
| --- | --- | --- |
| `heading` | string | Heading text. |
| `level` | integer | Markdown heading level. |
| `heading_path` | string[] | Heading breadcrumb. |
| `source` | Source | Heading source span. |
| `children` | OutlineNode[] | Nested child headings. |

## 🔎 search_text

Search literal text across visible Markdown notes and return section-aware
snippets.

Input:

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `query` | string | required | Literal text to search for. |
| `case_sensitive` | boolean | `false` | Whether matching is case-sensitive. |
| `context_lines` | integer | `0` | Surrounding lines to include in each snippet. |

Output:

| Field | Type | Description |
| --- | --- | --- |
| `query` | string | Original query. |
| `matches` | TextMatch[] | Search results. |
| `truncated` | boolean | Whether results were truncated. |

## .* search_regex

Search visible Markdown notes with a Rust regular expression.

Input:

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `pattern` | string | required | Rust regex pattern matched line by line. |
| `case_sensitive` | boolean | `false` | Whether matching is case-sensitive. |
| `context_lines` | integer | `0` | Surrounding lines to include in each snippet. |
| `path_glob` | string \| null | `null` | Optional note path glob, such as `正文/**/*.md`. |

Output:

| Field | Type | Description |
| --- | --- | --- |
| `pattern` | string | Original regex pattern. |
| `path_glob` | string \| null | Applied path glob. |
| `matches` | TextMatch[] | Search results. |
| `truncated` | boolean | Whether results were truncated. |

## 🧭 resolve_ref

Resolve an Obsidian reference to a note, heading, or block without guessing
ambiguous targets.

Input:

| Field | Type | Description |
| --- | --- | --- |
| `reference` | string | Reference such as `Note`, `[[Note]]`, `[[Note#Heading]]`, or `[[Note#^block]]`. |

Output:

| Field | Type | Description |
| --- | --- | --- |
| `result` | ResolveResult | Full resolution result. |

`ResolveResult` is tagged by `status`:

| Status | Fields |
| --- | --- |
| `resolved` | `reference`, `path`, `heading`, `block_id` |
| `ambiguous` | `reference`, `candidates` |
| `unresolved` | `reference` |

`reference` has `raw`, `target`, and optional `reference`.

## ↗ get_outlinks

Get outgoing local links from one note. Defaults to compact location output;
set `verbose: true` for source spans and snippets.

Input:

| Field | Type | Description |
| --- | --- | --- |
| `note` | string | Vault-relative path, note stem, or alias. |
| `verbose` | boolean | `false` | Return detailed source spans and snippets. |

Output:

| Field | Type | Description |
| --- | --- | --- |
| `note` | string | Resolved note path. |
| `links` | CompactLinkEvidence[] \| LinkEvidence[] | Outgoing links. |

## ↩ get_backlinks

Get backlinks to a note or Obsidian reference. Defaults to compact location
output; set `verbose: true` for source spans and snippets.

Input:

| Field | Type | Description |
| --- | --- | --- |
| `target` | string | Note path, stem, alias, or Obsidian reference. |
| `verbose` | boolean | `false` | Return detailed source spans and snippets. |

Output:

| Field | Type | Description |
| --- | --- | --- |
| `target` | string | Original target. |
| `resolution` | Resolve Summary | Resolved target status. |
| `backlinks` | CompactLinkEvidence[] \| LinkEvidence[] | Inbound links. |
| `truncated` | boolean | Whether results were truncated. |

`CompactLinkEvidence` has `location`, `target`, optional `alias`, `resolved`,
and optional `section`. Verbose `LinkEvidence` has `source`, `target`, optional
`alias`, `resolved`, and `snippet`.

## # get_tags

List tags across body tag nodes and frontmatter tags, or list notes under one
exact tag.

Input:

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `tag` | string \| null | `null` | Optional exact tag filter. Both `状态/身体` and `#状态/身体` are accepted. |
| `verbose` | boolean | `false` | Return detailed section metadata without duplicating note paths. |

Default output (`verbose: false`):

| Field | Type | Description |
| --- | --- | --- |
| `tags` | TagBucket[] | Tag buckets. |

`TagBucket`:

| Field | Type | Description |
| --- | --- | --- |
| `tag` | string | Normalized tag. |
| `notes` | CompactTagMatch[] | Compact note matches. Body tags use `path:line` notation. |

`CompactTagMatch`:

| Field | Type | Description |
| --- | --- | --- |
| `note` | string | Vault-relative path, with `:line` suffix for body tags. |
| `source_kind` | `"body"` \| `"frontmatter"` | Tag source. |
| `section` | string \| null | Heading breadcrumb, omitted when unavailable. |

Verbose output (`verbose: true`) uses a detailed but de-duplicated shape:

| Field | Type | Description |
| --- | --- | --- |
| `tag` | string | Normalized tag. |
| `occurrences` | DetailedTagOccurrence[] | Body/frontmatter occurrences. |

`DetailedTagOccurrence`:

| Field | Type | Description |
| --- | --- | --- |
| `location` | string | Vault-relative path, with `:line` or `:start-end` suffix when line data exists. |
| `source_kind` | `"body"` \| `"frontmatter"` | Tag source. |
| `section` | object \| null | Nearest-heading metadata for body tags; redundant breadcrumb/anchor fields are omitted. |

## 🧾 query_frontmatter

Query notes by a top-level frontmatter field.

Input:

| Field | Type | Description |
| --- | --- | --- |
| `field` | string | Top-level frontmatter field name. |
| `mode` | `"exists"` \| `"equals"` \| `"regex"` | Match mode. |
| `value` | string \| null | Value used by `equals` or `regex`. |

Output:

| Field | Type | Description |
| --- | --- | --- |
| `field` | string | Queried field. |
| `mode` | string | Applied match mode. |
| `value` | string \| null | Applied value. |
| `matches` | FrontmatterMatch[] | Matching notes. |
| `truncated` | boolean | Whether results were truncated. |

`FrontmatterMatch`:

| Field | Type | Description |
| --- | --- | --- |
| `note` | string | Note path. |
| `value` | JSON value | Matched frontmatter value. |

## 🧵 collect_note_context

Collect bounded context grouped as current note, outlinks, and backlinks.

Input:

| Field | Type | Description |
| --- | --- | --- |
| `note` | string | Vault-relative path, note stem, or alias. |

Output:

| Field | Type | Description |
| --- | --- | --- |
| `reference` | string | Original note reference. |
| `groups` | ContextGroup[] | Context grouped by relationship. |
| `truncated` | boolean | Whether context was truncated. |
| `omitted_count` | integer | Items omitted because of limits. |

`ContextGroup`:

| Field | Type | Description |
| --- | --- | --- |
| `kind` | string | Group kind, such as current note, outlinks, or backlinks. |
| `items` | ContextItem[] | Context items. |

`ContextItem` has `source: Source` and `content: string`.

## 🔗 collect_reference_context

Resolve a reference, then collect bounded context grouped as current note,
outlinks, and backlinks.

Input:

| Field | Type | Description |
| --- | --- | --- |
| `reference` | string | Reference such as `Note`, `[[Note]]`, or `[[Note#Heading]]`. |

Output: same shape as `collect_note_context`.

## ✂ read_section

Read exactly one heading section, block id, or line range from a note.

Input:

| Field | Type | Description |
| --- | --- | --- |
| `note` | string | Vault-relative path, note stem, or alias. |
| selector | object | Exactly one selector, flattened into the input object. |

Selector variants:

```json
{ "note": "Note.md", "kind": "heading", "heading": "Heading" }
{ "note": "Note.md", "kind": "block", "block_id": "block-id" }
{ "note": "Note.md", "kind": "lines", "line_start": 10, "line_end": 20 }
```

Output:

| Field | Type | Description |
| --- | --- | --- |
| `note` | string | Resolved note path. |
| `selector` | SectionSelector | Applied selector. |
| `source` | SourceSpan | Exact source span. |
| `content` | string | Selected content. |
| `truncated` | boolean | Whether content was truncated. |

`SourceSpan` serializes `path`, `line_start`, `line_end`, and `section`. Byte
offsets are internal and are not returned.

## ? find_unresolved_links

Find local links that do not resolve to any visible note.

Input:

```json
{}
```

Output:

| Field | Type | Description |
| --- | --- | --- |
| `links` | LinkEvidence[] | Unresolved links. |
| `truncated` | boolean | Whether results were truncated. |

## ! find_ambiguous_links

Find local links that resolve to multiple visible notes.

Input:

```json
{}
```

Output:

| Field | Type | Description |
| --- | --- | --- |
| `links` | LinkEvidence[] | Ambiguous links. |
| `truncated` | boolean | Whether results were truncated. |

## 🕸️ get_graph_neighborhood

Return a bounded local-link graph neighborhood around one note or reference.
Prefer this over `get_vault_graph` for normal agent context.

Input:

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `target` | string | required | Note path, stem, alias, or Obsidian reference used as graph center. |
| `depth` | integer | `1` | Number of resolved local-link hops to traverse. |
| `direction` | `"out"` \| `"in"` \| `"both"` | `"both"` | Edge direction to traverse. |
| `include_unresolved` | boolean | `false` | Include unresolved and ambiguous edges touching returned nodes. |

Output:

| Field | Type | Description |
| --- | --- | --- |
| `nodes` | GraphNode[] | Returned notes. |
| `edges` | GraphEdge[] | Local-link edges. |
| `truncated` | boolean | Whether graph output was truncated. |

`GraphNode`:

| Field | Type | Description |
| --- | --- | --- |
| `path` | string | Note path. |
| `title` | string \| null | First heading title. |
| `tags` | string[] | Tags found on the note. |

`GraphEdge`:

| Field | Type | Description |
| --- | --- | --- |
| `source` | Source | Link source. |
| `from` | string | Source note path. |
| `to` | string | Resolved destination path when available. |
| `target` | string | Original link target. |
| `alias` | string \| null | Link alias/display text. |
| `status` | string | Link resolution status. |

## 🗺️ get_vault_graph

Build the full visible-note local-link graph for audit, visualization, or
debugging.

Input:

```json
{}
```

Output: same shape as `get_graph_neighborhood`.
