# Scoped note stats design

## Goal

Allow `get_note_stats` to calculate statistics for a heading section or block
when the existing `note` input includes a bare Obsidian reference fragment.

## Interface

`note` continues to be the only selection input. Callers use bare references:

- `Project/Plan` calculates the whole note.
- `Project/Plan#Progress` calculates that heading section.
- `Project/Plan#Parent#Child` calculates the matching nested heading section.
- `Project/Plan#^milestone` calculates that block.

The API neither requires nor documents `[[...]]` wrappers. Existing resolver
compatibility with wrapped input is retained. No `heading`, `block_id`, or
`selector` request fields are added.

## Behavior

The query parses the input reference, resolves the containing note, and maps a
heading or block fragment to the existing private `SectionSelector` and
`section_source` implementation. For a selected range, word count, character
count, and line count use only the sliced source. The result adds an optional
`source` span to make the selected range explicit; it is absent for whole-note
statistics.

`backlink_count` is scoped too. It counts inbound links that resolve to the
selected note and explicitly target the selected heading or block. Links to the
note with no fragment do not count for a selected range. Whole-note statistics
retain the current count of all inbound links to that note.

Missing headings and blocks use the same failures and suggestions as the
unified `read_note` selector flow. Strict `resolve_ref` behavior remains
unchanged.

## Implementation boundaries

- `src/query/notes.rs` owns stats calculation and reference-to-selector
  orchestration.
- `src/query/section.rs` continues to own private section range selection.
- `src/query/links.rs` exposes scoped backlink counting over parsed link
  references.
- `src/server.rs`, `src/main.rs`, and generated tool documentation describe
  bare reference fragments without changing request shape.

## Verification

Add query and server-dispatch tests for whole-note compatibility, heading
selection, nested-heading selection, block selection, scoped backlink counts,
and missing selector errors. Run formatting, the query suite, and the full
Cargo test suite.
