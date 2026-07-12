# Note Stats Line Count Design

## Goal

Return a line count from `get-note-stats` alongside its existing word,
character, and backlink counts.

## API

Add a required `line_count: usize` field to `NoteStatsResult`. The field is
serialized by the existing MCP and CLI response paths, so no request schema or
new tool is needed.

## Counting Semantics

`line_count` is calculated from the raw Markdown source with
`content.lines().count()`.

- Blank lines count as lines.
- A final line terminator does not create an additional empty line.
- The count is independent of `word_count_mode` and includes frontmatter and
  Markdown syntax, just as `character_count` does.

## Implementation and Tests

The query implementation computes the count while it already has the source
text loaded. Query and server tests assert the returned count, including the
fixture note's trailing newline behavior. Existing result consumers receive the
new required JSON field automatically.

## Non-goals

This change does not add visible-text line counting, a separate stats endpoint,
or configurable line-count semantics.
