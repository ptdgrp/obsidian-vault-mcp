# Read-only Bases and Canvas

The default binary includes `query_base_views`, `read_base`, `read_canvas`, and `list_files` and their
hyphenated CLI equivalents. No Obsidian process, browser, JavaScript runtime,
network connection, or optional attachment feature is required. `read_note`
continues to return Markdown source and source selectors; rendered results never
claim source-file line numbers.

## Discovery, paths, and context

`list_files` returns paths, extensions, and byte sizes without parsing files.
Its `extensions` filter takes names without dots, for example `["base", "canvas"]`;
omitting it discovers all visible files. `include`/`exclude` are the same relative
path globs as note discovery. Pages contain at most 100 files. `list_notes`, text
searches, and the existing Markdown graph retain their Markdown-only contracts.

Discovery respects hidden paths, git ignores, and Obsidian `userIgnoreFilters`.
Explicit reader paths can address ignored files without adding them to discovery.
All paths remain relative to the configured root; absolute paths, traversal outside
the root, and symlinks escaping it are rejected. Source files and lazily accessed
note content obey `max_note_bytes`. The metadata scan does not read binary content.

A Base queries all visible files, including attachments and the Base itself.
Properties, tags, links, and note content are loaded only when expressions require
them. Properties from non-Markdown files are empty. `.obsidian/types.json` supplies
configured date/date-time property types; otherwise YAML values retain their
parsed types. Invalid required YAML or property types produce contextual errors.

`query_base_views` reads an exact `.base` path and lists each configured view's
1-based `position`, nullable `name`, `type` and `is_default` in file order. It
validates YAML structure without executing formulas, filters, datasets or plugin
views. Unknown view kinds/functions do not block discovery. Names are not
invented or deduplicated, and position is informational, not a selector.
`read_base` chooses the first view unless `view` names an exact unique configured
name (CLI `--view`); missing or duplicate selected names are errors. `this` points
to the Base by default. Set `context_note` to the exact host path to evaluate an
embedded Base in that host's context. Explicit contextual files may be ignored;
they are available for evaluation without entering the candidate set. Local link
resolution checks exact and source-relative paths, then visible suffix/short-name
matches; equally close ambiguous targets produce an error. Unresolved links retain
their reference and yield null from `asFile()`.

Without a discovered vault, project mode exposes `list_files` and `read_canvas`
alongside the existing file-local Markdown tools, rooted at the working directory.
`query_base_views` and `read_base` require vault mode; the latter evaluates
vault-wide data and context.

## Bases data semantics

The YAML reader executes global filters AND view filters. Nested `and`, `or`, and
`not` groups are supported; a `not` list succeeds when none of its entries do.
Formula dependencies are evaluated lazily, with explicit unknown-reference and
cycle diagnostics. Formulas can participate in filters, sorting, grouping,
columns, and summaries. `order` selects and orders columns; `sort` orders rows.
Property `displayName` changes headings. Missing properties are null; known
methods on null propagate null (except universal inspection/conversion methods).
Unknown methods and incompatible non-null argument types produce errors.

All public function families from the [official functions reference](https://obsidian.md/help/bases/functions)
are implemented: global date/duration/file/link/list/number/image/icon/html,
conversion and predicates, number rounding, Moment-style date fields/format/time/
relative, string matching/case/slice/split/replacement, regex matching, object
keys/values, file predicates, link resolution, and list contains/map/filter/reduce/
flat/join/sort/slice/unique. Lambda `value`, `index`, and `acc` scopes are restored
across nested evaluation. The engine supports typed equality, arithmetic,
comparisons, logical operators, literals, indexing, and formula references.
Unsupported syntax, names, arity, and unsupported regex features
are diagnosed rather than silently converted to false or null. All formula syntax
and function names are validated even when filters match no rows.

JavaScript rounding and exact binary-floating-point `toFixed` are implemented,
including `round(-1.5) == -1`, `2.5.toFixed(0) == "3"`, and
`2.675.toFixed(2) == "2.67"`. String length and slicing use UTF-16 positions;
a slice that would create an unpaired surrogate is diagnosed because Rust output
strings must remain valid Unicode. ECMAScript regex matching uses `regress`, with
JavaScript replacement captures. Expressions are immutable: global/sticky regex
state is not persisted between calls.

Dates use the process timezone and English formatting/relative text. Supported
input forms are `YYYY-MM-DD`, local ISO-style date-times, and RFC 3339 date-times.
Duration months/years and days/weeks are calendar units; hours/minutes/seconds are
elapsed units, so `1d` preserves the local hour across DST while `24h` need not.
Nonexistent local times are errors; ambiguous local times choose the earlier
offset. Date subtraction returns milliseconds as documented. `now()` is frozen
within a query; `random()` is nondeterministic. File creation time requires OS
support. `file.name` includes the extension and `file.basename` omits it. Some
Obsidian versions/oracle fixtures differ in these representations; this engine
follows the documented millisecond subtraction and explicit file-name contract.

Table, cards, list, and kanban views use Markdown tables. Card image fields and
Maps coordinate/icon/color fields are included as columns. The official Maps
view's center expression is validated, and configured tile sources are preserved
as references; no remote tile loading or map viewport is needed. Recognized
visual settings (such as column widths, card sizes, and map zoom) do not affect
rows. Unknown view types or settings fail explicitly to avoid discarding unknown
plugin data semantics. Image, icon, HTML, and link values retain meaningful text
or references; HTML is escaped in tables.

`groupBy` partitions rows and names the grouping property in each heading.
`groupOrder` controls visible groups and their order, retaining explicit empty
groups unless kanban `hideEmptyColumns` is true. Filtering and group visibility
precede sorting and the global view `limit`; grouping then partitions those
limited rows. Built-in summaries include Average, Min, Max, Sum, Range, Median,
Stddev, Earliest, Latest, Checked, Unchecked, Empty, Filled, and Unique. Custom
summary formulas evaluate the complete `values` list. Group summaries cover all
limited rows in the group; overall summaries cover all limited rows across groups.
Both are labeled with their scope and remain independent of the current page.

Markdown cells escape pipes, line breaks, HTML, and link aliases. Every page
returns `row_paths` in data-row order, independently of selected columns, so the
caller can follow up with an exact source read. Empty group markers do not add
row paths. This is a local compatibility implementation, not an embedded Obsidian
runtime: plugin-defined expression functions, locale customization, or future
undocumented syntax require explicit implementation. Expression and formula
nesting is capped at 128, with a per-record evaluation budget of 1,000,000 steps.

## Canvas semantics

The [JSON Canvas 1.0 specification](https://jsoncanvas.org/spec/1.0/) is read as
text, file, link, and group nodes. Mermaid includes full node text, URLs, file type,
path and optional subpath, plus labeled connections with all arrow combinations.
Images are file references; not opening their pixels is not conversion loss.
Coordinates are used only to infer containment: a node's full rectangle must fit
inside a group. Nested and overlapping groups retain all memberships through
explicit dotted `contains` edges. Empty groups remain nodes. Coincident group
bounds are reported as ambiguous in warnings rather than creating mutual nesting.

Generated node IDs cannot inject Mermaid syntax. Labels encode source punctuation
and HTML characters, and Mermaid HTML labels are disabled so literal HTML remains
text. Reverse edges are rendered by swapping endpoints and using a forward arrow,
which keeps labeled connections valid in Mermaid. Self-loops and isolated nodes
are retained. Unknown node types, duplicate IDs, invalid rectangles, unknown arrow
ends, invalid subpaths, and missing edge endpoints produce clear errors. Empty
Canvas files and pages beyond the end still return complete Mermaid syntax.

## Rendered pagination

Both readers accept one-based `page` and a Unicode-character `max_chars` budget
(defaulting to the server's normal read budget). A page packs complete semantic
units: Base rows (plus explicit empty-group markers), or Canvas nodes, connections,
and memberships. Necessary table headings/summaries and Canvas endpoint
declarations are repeated. One unit plus that context can exceed the budget; text
and references are never truncated for presentation. `pagination` returns the
requested page, total pages/units, returned units, and optional `next_page`.
Use the same budget and unchanged files when following pages. Beyond-end pages
have zero returned units and a complete empty rendering. These are result-unit
positions, never Markdown source-line selectors. `total_rows` counts Base data
rows after filters/visibility/limit, while `total_units` also counts empty groups.

## Implementation and verification

YAML uses `serde_yaml_ng`; typed evaluation is implemented locally so unsupported
operations cannot disappear behind a dependency's null/false fallbacks. `regress`
provides ECMAScript regex syntax, `chrono` supplies local calendar arithmetic,
`num-bigint` supplies exact decimal rounding, and `ryu-js` supplies JavaScript
number formatting. There is no dynamic script execution.

Repository tests cover the public function families, null/mixed records, formula
cycles/depth, JS rounding, typed dates, links/embeds/backlinks, summaries and
pagination, Maps/kanban semantics, full Canvas text/arrows/groups, and both CLI
and vault/project MCP contracts. CLI DST tests run in an isolated timezone
subprocess. See also [Bases syntax](https://obsidian.md/help/bases/syntax),
[views](https://obsidian.md/help/bases/views),
[Maps](https://obsidian.md/help/bases/views/map),
[Moment calendar arithmetic](https://momentjs.com/docs/#/manipulating/add/), and
[Mermaid flowcharts](https://mermaid.js.org/syntax/flowchart.html).
