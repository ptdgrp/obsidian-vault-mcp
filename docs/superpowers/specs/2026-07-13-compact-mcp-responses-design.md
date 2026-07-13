# Compact MCP Responses Design

## Status and scope

This document records the response contracts confirmed during the public API
noise audit. It is intentionally separate from
`2026-07-13-task-oriented-query-tools-design.md`: that design reduces eight
overlapping query tools to three task-oriented tools, while this design makes
the remaining public tools smaller and more predictable for LLM callers.

The contracts below cover every remaining public tool reviewed in the API noise
audit. Tools removed or replaced by the task-oriented query-tools design are not
repeated here.

## Common response rules

1. Do not echo request fields unless the resolved, normalized value is needed by
   a subsequent call.
2. Use actionable vault-relative locators such as `note.md#L12` and normalized
   references such as `note.md#Heading#Child` instead of nested source or parser
   objects.
3. Omit optional fields and arrays when they have no value. Do not serialize
   `null` placeholders.
4. Remove `verbose` modes when the additional evidence can be retrieved with a
   targeted `read_note` call.
5. Use fixed-size numeric paging for ordered collections. Page numbers start at
   1; an out-of-range page succeeds with an empty collection and retains the
   actual totals.
6. Keep full paths and references intact. Display text may be bounded, but
   actionable identifiers are never truncated.
7. Preserve complete multi-heading paths. A nested heading reference must not be
   reduced to its final heading component.

## Input naming and lookup contract

Input field names communicate lookup semantics:

- `note`, `target`, and `reference` use Obsidian reference resolution. Callers may
  use a stem, alias, wikilink, heading reference, or block reference as permitted
  by the individual tool. They do not need a `.md` suffix or vault-relative
  directory, so examples prefer `林动`, `林动#身体`, and `林动#^profile`.
- A complete vault-relative note path remains valid in a reference field when it
  is needed to disambiguate duplicate stems.
- `path` and `new_path` use exact file-path syntax. They require a safe
  vault-relative path with a `.md` suffix, such as `人物/林动.md`; absolute paths
  and paths that escape the vault are errors.
- A failed reference lookup compares the normalized unresolved note target with
  visible resolver names and paths. A failed exact-path lookup compares the
  requested path with visible vault-relative paths.
- Suggestions are ordered by edit distance and then natural path order. Only
  candidates with Levenshtein distance at most 3 are eligible, so the server does
  not make distant guesses.
- The reference's heading or block suffix is preserved when suggesting a corrected
  note target.
- Suggestions apply when locating an existing `path`. A destination `new_path`
  is not a lookup and therefore receives validation errors rather than a similar
  existing-path suggestion.

## Reference containment

Backlink scopes use syntactic reference containment:

```text
Note
├── Note#Heading
│   └── Note#Heading#Child
│       └── Note#Heading#Child#Grandchild
└── Note#^block-id
```

- A note scope contains direct note references, all heading references, and all
  block references to that note.
- A heading scope contains itself and descendant heading paths. It does not
  contain sibling headings, parent headings, or block references physically
  located in the section.
- A block scope contains only the exact block reference and has no child scope.
- Block IDs and headings are parallel addressing systems.

## `get_outlinks`

### Input

```json
{
  "note": "林动",
  "page": 1
}
```

- `page` defaults to 1 and must be at least 1.
- The fixed page size is 50 link occurrences.
- `verbose` is removed.

### Output

```json
{
  "note": "人物/林动.md",
  "targets": [
    {
      "source": "人物/林动.md#L42",
      "target": "设定/境界体系.md#淬体境"
    }
  ],
  "ambiguous_targets": [
    {
      "source": "人物/林动.md#L63",
      "reference": "林氏宗族",
      "candidates": [
        "势力/林氏宗族.md",
        "旧稿/林氏宗族.md"
      ]
    }
  ],
  "unresolved_targets": [
    {
      "source": "人物/林动.md#L51",
      "reference": "缺失设定"
    }
  ],
  "pagination": {
    "page": 1,
    "total_pages": 1,
    "total_links": 3
  }
}
```

- Link occurrences are sorted by source location and paged before the selected
  page is partitioned by resolution outcome.
- `targets` is always present and contains uniquely resolved normalized targets.
- `ambiguous_targets` and `unresolved_targets` are present only when the selected
  page contains those outcomes.
- Ambiguous candidates preserve the original heading or block selector.
- Alias, section, snippet, nested resolver output, candidate match kind, and
  status strings are omitted.

## `get_backlinks`

### Input

```json
{
  "target": "人物/林动#身体",
  "include": ["剧情/**"],
  "exclude": ["**/草稿/**"],
  "page": 1
}
```

- The input target must resolve uniquely. Unresolved targets fail with a path
  suggestion when available; ambiguous targets fail with candidate paths.
- `include` and `exclude` filter source-note paths. Excludes take precedence.
- The fixed page size is 50 backlink occurrences.
- `verbose` is removed.

### Output

```json
{
  "scope": "人物/林动.md#身体",
  "references": [
    {
      "target": "人物/林动.md#身体",
      "sources": [
        "剧情/第一章.md#L12",
        "剧情/第二章.md#L18"
      ]
    },
    {
      "target": "人物/林动.md#身体#伤势",
      "sources": [
        "剧情/第三章.md#L27"
      ]
    }
  ],
  "pagination": {
    "page": 1,
    "total_pages": 1,
    "total_backlinks": 3
  }
}
```

- `scope` is the uniquely resolved, normalized query reference.
- Matching backlink occurrences are paged and then grouped by their actual
  normalized target reference.
- Duplicate links from the same source line to the same target are returned once.
- Backlinks whose links are ambiguous or unresolved cannot safely belong to a
  resolved scope and are excluded; `audit_links` owns those outcomes.
- Per-occurrence alias, status, section, source object, and snippet are omitted.

## `resolve_ref`

The existing `result` wrapper, status string, raw input echo, internal
`ReferenceInfo`, candidate match kind, and null fields are removed.

Resolved note, heading, or block:

```json
{
  "target": "人物/林动.md#身体#伤势"
}
```

Ambiguous target:

```json
{
  "ambiguous_targets": [
    "人物/发动机.md#原理",
    "设定/发动机.md#原理"
  ]
}
```

Unresolved target with a unique suggestion:

```json
{
  "unresolved_target": "错误目录/林动#身体",
  "suggested_target": "人物/林动.md#身体"
}
```

Unresolved target without a suggestion:

```json
{
  "unresolved_target": "不存在的人物"
}
```

Exactly one of `target`, `ambiguous_targets`, or `unresolved_target` is present.
`suggested_target` is present only alongside `unresolved_target` when the
suggestion is unique and reliable.

## `get_note_structure`

`get_note_structure` is a bounded structural overview, not a complete parser
dump.

### Input

```json
{
  "note": "林动"
}
```

### Output

```json
{
  "note": "人物/林动.md",
  "frontmatter_fields": [
    "aliases",
    "status"
  ],
  "headings": [
    {
      "heading": "身体",
      "line": 12
    },
    {
      "heading": "身体/伤势",
      "line": 20
    }
  ],
  "link_count": 14,
  "embeds": [
    "附件/林动.png",
    "设定/境界体系.md#淬体境"
  ],
  "tags": [
    "人物",
    "状态/活跃"
  ],
  "blocks": [
    "profile",
    "injury"
  ]
}
```

- `note` is the resolved vault-relative path.
- `frontmatter_fields` contains naturally sorted top-level field names, not
  arbitrary YAML values.
- `headings` contains selectable non-H1 slash-separated heading paths and lines.
- `link_count` counts local-link occurrences; link detail belongs to
  `get_outlinks`.
- Embeds, tags, and block IDs are normalized and deduplicated.
- Tags combine body and frontmatter tags.
- Empty arrays are omitted. `note` and `link_count` are always present.
- Each array is limited to 50 entries. When a limit omits entries, an `omitted`
  object is included with only the affected category counts:

```json
{
  "omitted": {
    "headings": 12,
    "tags": 3
  }
}
```

## `get_note_outline`

### Input

```json
{
  "note": "林动",
  "page": 1
}
```

- The optional heading/ancestor-chain mode is removed.
- The fixed page size is 100 non-H1 headings.

### Output

```json
{
  "note": "人物/林动.md",
  "headings": [
    {
      "heading": "身体",
      "line": 12
    },
    {
      "heading": "身体/伤势",
      "line": 20
    },
    {
      "heading": "关系/绫清竹",
      "line": 46
    }
  ],
  "pagination": {
    "page": 1,
    "total_pages": 1,
    "total_headings": 3
  }
}
```

- Headings retain document order.
- Slash-separated paths encode hierarchy and can be passed directly to
  `read_note`.
- Level, duplicated heading path, recursive children, source, and section are
  omitted.

## `read_note`

Input selection semantics remain unchanged: callers may use a bare reference or
exactly one explicit heading, block ID, or line selector. `max_chars` remains an
optional request-level character budget.

Complete result:

```json
{
  "source": "人物/林动.md#L12-L38",
  "content": "## 身体\n\n……"
}
```

Truncated result:

```json
{
  "source": "人物/林动.md#L12-L38",
  "content": "## 身体\n\n……",
  "truncated": true
}
```

- `source` describes the full selected source span.
- When `truncated` is present, content is a character prefix of that span.
- The duplicated top-level path, nested section metadata, selector echo,
  returned-character count, and static `next_step` are omitted.
- `truncated` is present only when true.

## `get_note_stats`

### Input

```json
{
  "note": "林动#身体",
  "word_count_mode": "visible"
}
```

### Output

```json
{
  "scope": "人物/林动.md#身体",
  "word_count": 824,
  "character_count": 2150,
  "line_count": 47,
  "backlink_count": 6
}
```

- `scope` is the resolved normalized note, heading, or block reference.
- The requested word-count mode is not echoed.
- Heading backlink counts use descendant-heading containment.
- Block backlink counts use exact block matching.
- Whole-note backlink counts include direct note, heading, and block references.
- Line ranges remain unsupported because they are not link-addressable semantic
  scopes.

## `search_text`

### Input

```json
{
  "query": "祖符",
  "case_sensitive": false,
  "include": ["正文/**"],
  "exclude": ["**/草稿/**"],
  "page": 1
}
```

- `context_lines` is removed.
- The fixed page size is 50 matching lines.

### Output

```json
{
  "matches": [
    {
      "source": "正文/第一章.md#L42",
      "preview": "……林动在石池中感应到了祖符的气息……"
    }
  ],
  "pagination": {
    "page": 1,
    "total_pages": 1,
    "total_matches": 1
  }
}
```

- A matching line is returned once even if the literal occurs multiple times.
- Preview is at most 240 characters and is centered on the first match.
- Ellipses appear only where preview text was omitted.
- The input query, nested section metadata, and truncation boolean are omitted.
- Results are ordered by natural note path and line number.

## `search_regex`

`search_regex` uses the same result DTO, ordering, fixed page size, filtering, and
preview rules as `search_text`. Preview is centered on the first regex match on
the line. The regex pattern is not echoed. An invalid regex fails explicitly.

Example input:

```json
{
  "pattern": "祖符.{0,20}气息",
  "case_sensitive": false,
  "include": ["正文/**"],
  "exclude": ["**/草稿/**"],
  "page": 1
}
```

## `list_tags`

### Input

```json
{
  "scope": "note",
  "include": ["人物/**"],
  "exclude": ["**/草稿/**"],
  "page": 1
}
```

### Output

```json
{
  "tags": [
    "人物",
    "状态/活跃",
    "阵营/道宗"
  ],
  "pagination": {
    "page": 1,
    "total_pages": 1,
    "total_tags": 3
  }
}
```

- The fixed page size is 100 unique tags.
- Tags are normalized without a leading `#` and naturally sorted.
- Scope and path filters are not echoed.

## `get_tag`

The existing plural `get_tags` tool is renamed to `get_tag` and accepts one tag
per call. The existing `tags` array and `verbose` mode are removed.

### Input

```json
{
  "tag": "状态/活跃",
  "scope": "section",
  "include": ["人物/**"],
  "exclude": ["**/草稿/**"],
  "page": 1
}
```

### Output

```json
{
  "matches": [
    "人物/林动.md#身体",
    "人物/绫清竹.md#状态"
  ],
  "pagination": {
    "page": 1,
    "total_pages": 1,
    "total_matches": 2
  }
}
```

The requested scope determines locator granularity:

- `note`: note paths containing the tag in body or frontmatter.
- `frontmatter`: note paths containing the tag in frontmatter.
- `body`: note paths containing the tag in body content.
- `section`: normalized heading references containing the tag; deduplicated per
  section.
- `line`: line references containing the tag; deduplicated per line.

The fixed page size is 100 locators. Source-kind fields, compact/detailed dual
occurrences, tag/scope echoes, and section objects are omitted.

## `list_categories`

Categories use folder-name tag semantics, not full directory-path identity. A
folder name appearing at different levels or under different volumes represents
the same category.

### Input

```json
{
  "include": ["第一卷/**", "第二卷/**", "设定集/**"],
  "exclude": ["**/草稿/**"],
  "page": 1
}
```

### Output

```json
{
  "categories": [
    "人物",
    "关系",
    "设定"
  ],
  "pagination": {
    "page": 1,
    "total_pages": 1,
    "total_categories": 3
  }
}
```

- Every parent-directory segment of a visible note contributes a category name.
- Equal segment names are merged across volumes, subtrees, and global setting
  collections.
- Root-level notes contribute no empty category.
- The fixed page size is 100 unique naturally sorted names.
- Scope filters are not echoed.

## `get_category`

The existing plural `get_categories` tool and `categories` array input are
replaced by a single-category query.

### Input

```json
{
  "category": "关系",
  "include": ["第一卷/**", "第二卷/**", "设定集/**"],
  "exclude": ["**/草稿/**"],
  "page": 1
}
```

### Output

```json
{
  "notes": [
    "第一卷/人物/关系/林动.md",
    "第二卷/人物/关系/绫清竹.md",
    "设定集/人物/关系/关系总览.md"
  ],
  "pagination": {
    "page": 1,
    "total_pages": 1,
    "total_notes": 3
  }
}
```

- A note matches when any parent-directory segment equals the normalized category
  name.
- Matching the same name at different locations is intentional union behavior,
  not ambiguity.
- Leading/trailing whitespace and slashes are removed. A remaining slash is an
  error because category inputs are folder-name tags, not paths.
- Include/exclude filters further restrict matching notes.
- The fixed page size is 100 naturally sorted note paths.
- Category, filters, titles, sizes, and bucket wrappers are not echoed.

## `query_frontmatter`

### Input

```json
{
  "field": "phase",
  "mode": "equals",
  "value": "active",
  "include": ["第一卷/**", "第二卷/**"],
  "exclude": ["**/草稿/**"],
  "page": 1
}
```

- `mode` remains the explicit `exists`, `equals`, or `regex` discriminator.
- `exists` rejects a value. `equals` and `regex` require one.
- Include/exclude filtering is added and follows the common path-filter rules.
- The fixed page size is 100 notes.

### Output

```json
{
  "notes": [
    "第一卷/人物/林动.md",
    "第二卷/人物/绫清竹.md"
  ],
  "pagination": {
    "page": 1,
    "total_pages": 1,
    "total_notes": 2
  }
}
```

- Only naturally sorted matching note paths are returned.
- Field, mode, value, and filters are not echoed.
- Matched frontmatter values are not returned because arbitrary YAML arrays and
  objects can dominate the MCP context. Callers use targeted `read_note` access
  when the actual value is needed.
- Invalid regular expressions fail explicitly.

## Section mutation results

`append_section`, `replace_section`, and `delete_section` remain separate tools
but share one result shape.

Append or replace result:

```json
{
  "changed": "人物/林动.md#L20-L24"
}
```

Delete result:

```json
{
  "changed": "人物/林动.md#L20"
}
```

- Append points to the inserted content.
- Replace points to the replacement content.
- Delete points to the nearest valid post-deletion line that should be re-read;
  the line is clamped to the edited document's valid line range, using line 1 for
  an empty document.
- Note, line-start, and line-end fields are replaced by the single actionable
  locator.
- Selector, content, operation, and redundant success booleans are not echoed.

## Rename mutation results

`rename_note`, `rename_heading`, and `rename_block_id` retain their existing
result contract:

```json
{
  "dry_run": true,
  "updated_references": 6,
  "changed_notes": [
    "人物/林动.md",
    "剧情/第一章.md",
    "剧情/第二章.md"
  ]
}
```

- `dry_run` is retained without renaming.
- `updated_references` distinguishes target edits from repaired references.
- `changed_notes` remains complete and is neither truncated nor paginated;
  mutation preview safety requires one coherent change set.
- Old/new names and selectors are not echoed.

## Public API changes

The response redesign also makes these breaking request-surface changes:

- Remove `verbose` from `get_outlinks`, `get_backlinks`, and `get_tag`.
- Rename `get_tags` to `get_tag` and replace `tags[]` with `tag`.
- Rename `get_categories` to `get_category` and replace `categories[]` with
  `category`.
- Remove `context_lines` from `search_text` and `search_regex`.
- Remove the optional heading/ancestor-chain input from `get_note_outline`.
- Add fixed numeric `page` inputs to the collection tools described above.
- Add include/exclude path filters to `query_frontmatter`.

No compatibility aliases are required. MCP and CLI names, arguments, errors, and
serialized results change together.

## Internal architecture

Public DTOs are task-shaped and remain separate from parser, resolver, and edit
types. Internal data retains full evidence; public serialization selects only the
fields required by each tool.

Shared internal components are:

- `ResolvedReference`: retains the resolved note path and the full heading path
  or block ID. It can format a normalized reference string and evaluate syntactic
  containment.
- `Locator`: formats vault paths, line spans, heading references, and block
  references consistently without exposing byte offsets or nested section data.
- `PageSlice<T>`: validates 1-based pages, computes totals, slices a naturally or
  document-ordered collection using the tool's fixed page size, and supplies the
  common page metadata.
- `PathFilter`: continues to implement include-union and exclude-precedence rules
  for all scoped queries.
- Display helpers: normalize tags and category labels, truncate search previews
  on Unicode character boundaries, and cap structural overview groups.

Collection queries follow one data flow:

```text
visible notes
  -> request filtering
  -> collect complete lightweight matches
  -> deterministic ordering and deduplication
  -> compute totals
  -> select fixed numeric page
  -> materialize compact public DTO
```

This is live filesystem paging rather than snapshot paging. Concurrent vault
changes may move later page boundaries; the API makes no cross-call snapshot
guarantee.

## Error and output-boundary policy

- Page zero is an error. A page beyond the final page is an empty successful
  result with actual totals.
- Invalid globs and regular expressions are errors that identify the offending
  field.
- Exact path inputs reject missing `.md`, absolute paths, and vault escapes.
- Reference inputs report ambiguity with naturally sorted candidate paths.
- Missing reference and exact-path targets offer suggestions only when edit
  distance is at most 3, preserving reference suffixes where applicable.
- `get_backlinks` requires a uniquely resolved scope. Ambiguous and unresolved
  links found while scanning the vault are not assigned to that scope.
- A public collection is never silently cut at the global output-byte boundary.
  Callers receive a defined page/omission result or an explicit error.
- Before applying a rename, the server computes and validates the complete
  mutation result. If the complete changed-note list cannot fit the configured
  output boundary, both preview and apply fail before any write.
- Section mutations write atomically and return the compact post-edit locator only
  after the write succeeds.

## Verification

### Shared contract tests

- MCP schemas and CLI help expose the same renamed tools, removed arguments, and
  added page/filter fields.
- Optional fields and arrays are absent rather than serialized as null or empty
  placeholders, except arrays explicitly documented as always present.
- All locators and normalized references round-trip through the corresponding
  `read_note` or resolver input.
- Multi-heading references retain every heading component.
- Note/ref and exact-path lookup failures honor the edit-distance-3 suggestion
  boundary.
- Fixed numeric pages cover empty, first, final partial, and out-of-range pages.

### Link and reference tests

- `get_outlinks` partitions a single ordered page into resolved, ambiguous, and
  unresolved arrays without status strings or verbose evidence.
- `get_backlinks` covers whole-note, heading-descendant, exact-block, sibling
  exclusion, parent exclusion, and heading/block independence.
- Backlink source filtering applies before totals and paging.
- Duplicate same-line links to the same normalized target are deduplicated where
  documented.
- `resolve_ref` serializes each of resolved, ambiguous, unresolved-with-suggestion,
  and unresolved-without-suggestion shapes without its old wrapper.

### Note inspection tests

- `get_note_structure` returns a bounded overview, omits empty groups, and reports
  exact per-group omission counts.
- `get_note_outline` preserves document order across page boundaries and emits
  directly selectable slash paths.
- `read_note` emits one compact source locator and includes `truncated` only when
  content is shortened.
- `get_note_stats` emits the normalized scope and applies reference containment to
  backlink counts.

### Search and metadata tests

- Literal and regex search share the exact match DTO and center a Unicode-safe
  240-character preview on the first match.
- Multiple matches on one line produce one result.
- Tag locators use the requested note, frontmatter, body, section, or line
  granularity and deduplicate at that granularity.
- Category names intentionally union equal folder segments across volumes and
  global setting directories.
- Frontmatter queries never serialize matched arbitrary values and apply path
  filters before paging.

### Mutation tests

- Append and replace return the exact post-edit changed range; delete returns the
  post-delete verification boundary.
- Rename previews and applied results retain `dry_run`, exact updated-reference
  counts, and complete changed-note lists.

## Out of scope

- Compatibility aliases or a deprecation period.
- Cursor or snapshot-consistent pagination.
- Returning attachment contents.
- Persisted search or reference indexes.
- Reintroducing verbose evidence through another boolean mode.
