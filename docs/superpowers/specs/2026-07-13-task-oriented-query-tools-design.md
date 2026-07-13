# Task-Oriented Query Tools Design

## Summary

Replace eight overlapping vault-query tools with three task-oriented tools:

| New public tool | Replaces or removes |
| --- | --- |
| `audit_links` | Replaces `find_unresolved_links` and `find_ambiguous_links` |
| `get_note_neighborhood` | Replaces `get_graph_neighborhood`, `collect_note_context`, and `collect_reference_context` |
| `list_notes` | Replaces the existing `list_notes` contract and removes `list_vault_files` |

Delete `get_vault_graph` because there is no current full-graph use case. The MCP
and CLI surfaces must expose the same names and semantics. Compatibility aliases
are explicitly out of scope.

The resulting discovery model is:

- `list_notes`: discover which notes exist.
- `get_note_structure`: inspect what exists inside one note.
- `get_note_neighborhood`: inspect how one note connects to other notes.
- `audit_links`: inspect where local-link integrity is broken.

## Design principles

1. Prefer one clear task per tool over a smaller number of mode-heavy tools.
2. Remove fields that do not change the agent's likely next action.
3. Use task-shaped results rather than exposing internal parser or resolver types.
4. Keep stable vault-relative paths intact so every result remains actionable.
5. Make incomplete relationship results explicit; never silently claim a partial
   vault index is complete.
6. Keep MCP and CLI vocabulary, filtering, pagination, errors, and output aligned.

## Public API changes

The following tools and CLI commands are removed without aliases:

- `find_unresolved_links`
- `find_ambiguous_links`
- `get_vault_graph`
- `get_graph_neighborhood`
- `collect_note_context`
- `collect_reference_context`
- `list_vault_files`

The existing `list_notes` name remains, but its input and output contracts change.

The following tools are added:

- `audit_links`
- `get_note_neighborhood`

## `list_notes`

### Intent

Return a small, navigable page of visible Markdown notes. It does not enumerate
attachments, summarize directories, report modification times, or return note
outlines.

### Input

```json
{
  "include": ["人物/**"],
  "exclude": ["**/草稿/**"],
  "page": 1
}
```

- `include` defaults to an empty array. Patterns are vault-relative globs and are
  unioned. An empty array adds no restriction.
- `exclude` defaults to an empty array and takes precedence over `include`.
- Request filters can only narrow the notes visible under the vault's configured
  include/exclude rules.
- `page` defaults to 1 and must be at least 1.
- The page size is fixed at 100 and is not an input or output field.
- Notes are filtered, naturally sorted by vault-relative path, and then paged.

### Output

```json
{
  "notes": [
    {
      "path": "人物/林动.md",
      "title": "林动"
    }
  ],
  "pagination": {
    "page": 1,
    "total_pages": 3,
    "total_notes": 243
  }
}
```

- Each note contains only `path` and, when available, `title`.
- `title` follows the existing priority: first H1, frontmatter title, then
  pathname. If title extraction fails while the note remains enumerable, the
  field is omitted.
- Display titles are truncated to 200 characters with an ellipsis. Paths are
  never truncated.
- An empty result has `total_notes: 0` and `total_pages: 0`.
- A page beyond the last page succeeds with an empty `notes` array while retaining
  the requested `page` and the actual totals.
- `size`, `modified`, directory summaries, attachment summaries, and page-size
  metadata are not returned.

The CLI exposes the same contract, for example:

```text
list_notes --include '人物/**' --exclude '**/草稿/**' --page 2
```

## `audit_links`

### Intent

Perform one vault-wide local-link integrity check and return both unresolved and
ambiguous links. Callers should not need two full-vault scans to answer the single
question "which local links are unsafe?"

### Input

```json
{
  "page": 1
}
```

- `page` defaults to 1 and must be at least 1.
- Unresolved and ambiguous results are independently sorted by source using
  natural path order followed by line range.
- Each category has a fixed page size of 50. One `page` value is applied to both
  categories.

### Output

```json
{
  "unresolved": [
    {
      "source": "剧情/第一章.md#L42",
      "target": "不存在的人物"
    }
  ],
  "ambiguous": [
    {
      "source": "剧情/第二章.md#L18",
      "target": "林动",
      "candidates": [
        "人物/林动.md",
        "旧稿/林动.md"
      ]
    }
  ],
  "totals": {
    "unresolved": 31,
    "ambiguous": 4
  },
  "pagination": {
    "page": 1,
    "total_pages": 2
  }
}
```

- `source` uses the compact actionable form `path#Lx` or `path#Lx-Ly`.
- Unresolved items contain only `source` and `target`.
- Ambiguous items additionally contain candidate paths.
- A single ambiguous link returns at most 20 candidate paths. When more exist,
  it includes `omitted_candidates`; the field is absent otherwise.
- `total_pages` is the larger of the unresolved and ambiguous page counts.
- A category that has no entries on the requested page returns an empty array.
- A page beyond both categories succeeds with two empty arrays and retains the
  actual totals.
- Alias, snippet, nested resolution objects, raw reference structures, and
  candidate match kinds are not returned. Callers can use `read_note` with the
  compact line reference when source text is needed.

## `get_note_neighborhood`

### Intent

Return a bounded, reliable relationship neighborhood around one note. The tool
accepts both note identifiers and Obsidian references and replaces the previous
compact context collectors as well as the graph-neighborhood entry point.

### Input

```json
{
  "target": "[[林动#身体]]",
  "depth": 1,
  "direction": "both"
}
```

- `target` accepts a vault-relative path, stem, alias, wikilink, heading
  reference, or block reference.
- `depth` defaults to 1 and must be in the inclusive range 1 through 3.
- `direction` is `out`, `in`, or `both` and defaults to `both`.
- Only uniquely resolved local links participate in traversal.
- Multiple resolved link occurrences with the same source-note and target-note
  paths form one directed neighborhood link. Occurrence-level evidence remains a
  responsibility of `get_outlinks` and `get_backlinks`.
- Problem links are not an optional neighborhood mode; `audit_links` owns that
  task.
- The result contains at most 50 neighboring notes and 100 links. These limits
  are fixed and are not request parameters.
- Neighborhood results are not paginated because paging a relationship graph
  would make each page structurally incomplete.

### Output

```json
{
  "center": {
    "path": "人物/林动.md",
    "title": "林动",
    "heading": "身体"
  },
  "notes": [
    {
      "path": "设定/境界体系.md",
      "title": "境界体系",
      "distance": 1
    },
    {
      "path": "剧情/第一章.md",
      "title": "第一章",
      "distance": 1
    }
  ],
  "links": [
    {
      "from": "人物/林动.md",
      "to": "设定/境界体系.md"
    },
    {
      "from": "剧情/第一章.md",
      "to": "人物/林动.md"
    }
  ]
}
```

- `center` is not repeated in `notes`.
- `center.heading` or `center.block_id` is included only when the input selected
  that kind of reference.
- Neighboring notes contain only `path`, optional `title`, and shortest traversal
  `distance` from the center.
- Neighboring notes are ordered by shortest distance and then natural path order.
- Titles use the same 200-character display limit as `list_notes`.
- Links contain only their resolved `from` and `to` paths.
- Links are unique directed note pairs ordered naturally by `from` and then `to`.
- `direction` controls which links may expand the breadth-first traversal. The
  returned `links` contain all resolved links whose endpoints are both in the
  returned center-plus-neighbor set, preserving cross-links among discovered
  notes.
- Tags, sizes, resolution status, aliases, raw targets, snippets, and source spans
  are omitted. Precise one-direction evidence remains available from
  `get_outlinks` and `get_backlinks`.
- `truncated`, `omitted_notes`, and `omitted_links` are present only when the
  corresponding fixed limit caused omission.
- The full requested-depth neighborhood is computed before output limiting.
  `omitted_notes` is the number of eligible non-center notes beyond the first 50.
  After retaining those notes, eligible unique links are those whose endpoints
  are both in the retained center-plus-neighbor set; `omitted_links` is the number
  of those links beyond the first 100. Links touching an omitted note are already
  represented by `omitted_notes` and are not counted again as omitted links.

## Internal architecture

Introduce one internal, non-persistent `LinkIndex` used by relationship queries:

```text
visible Markdown notes
        -> read and parse
        -> resolve every local link
        -> LinkIndex {
             notes,
             resolved_links,
             unresolved_links,
             ambiguous_links
           }
        -> audit_links / get_note_neighborhood
```

`LinkIndex` is a per-query view over the current vault, not a database, watcher,
or on-disk index. It reuses the existing parse cache. Both public tools therefore
share exactly the same visibility and resolution rules without duplicating their
own traversal logic.

Public result DTOs remain task-specific. Internal parser and resolver types must
not leak into MCP or CLI output merely because they contain more data.

## Error and partial-result policy

### `list_notes`

- Invalid globs and `page < 1` are errors.
- A page beyond the available range is an empty successful result.
- Failure to extract a title omits `title`; it does not hide an otherwise visible
  path.

### `audit_links`

- `page < 1` is an error; an out-of-range page is an empty successful result.
- If any visible note cannot be read or parsed, the entire call fails and names
  the note path. Returning a partial health audit would produce false confidence.

### `get_note_neighborhood`

- An unresolved target is an error and includes the nearest-path suggestion when
  available.
- An ambiguous target is an error and includes candidate paths.
- An invalid depth is an error.
- Because correct backlinks require the complete visible note set, any visible
  note read or parse failure fails the whole call.
- Reaching a fixed node or link limit returns a successful partial neighborhood
  with explicit omission fields.

No tool silently truncates serialized JSON at the global output-byte boundary.
Count-based omission is represented by the contracts above; otherwise the call
fails explicitly.

## Documentation and discovery

- MCP descriptions and CLI help use the same one-sentence intent for each tool.
- README, README.zh-CN, `docs/tools.md`, examples, and recommended workflows must
  remove all deleted names.
- Documentation presents the four-level model: enumerate notes, inspect internal
  structure, inspect external relationships, audit broken relationships.
- `list_notes` documents fixed numeric paging rather than cursor pagination.

## Verification

### `list_notes`

- Include patterns are unioned and excludes take precedence.
- Filtering occurs before natural sorting and fixed-size paging.
- Empty vaults, last pages, and out-of-range pages match the contract.
- Output contains no size, page-size, modified-time, directory, or attachment
  fields.
- MCP and CLI serialize the same result structure.

### `audit_links`

- One call returns both unresolved and ambiguous results.
- Each category pages independently and top-level `total_pages` uses the larger
  page count.
- Compact sources cover single-line and multiline spans.
- Candidate omission above 20 is reported exactly.
- A visible-note parse failure prevents a partial audit result.

### `get_note_neighborhood`

- Paths, stems, aliases, heading references, and block references resolve.
- All three directions and depths 1 through 3 are covered.
- Cycles and duplicate links do not duplicate notes.
- Traversal uses resolved links only.
- Node and link caps report exact omission counts.
- Output contains no tags, sizes, status, snippets, aliases, raw targets, or
  source spans.
- Unresolved errors include suggestions when possible and ambiguous errors list
  candidates.

### Removal and consistency

- MCP schemas and CLI help expose none of the seven removed names.
- Repository documentation and examples contain no stale calls.
- Tests assert the exact public JSON shapes rather than only inspecting selected
  fields.

## Out of scope

- Compatibility aliases or a deprecation period.
- Attachment enumeration.
- Full-vault graph export or visualization.
- Persistent indexing, database storage, or filesystem watching.
- Snapshot-consistent pagination across concurrent filesystem mutations.
- Combining `get_note_structure` with relationship queries.
