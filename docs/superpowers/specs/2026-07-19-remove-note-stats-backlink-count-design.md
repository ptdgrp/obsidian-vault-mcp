# Remove Note Stats Backlink Count

**Date:** 2026-07-19

## Problem

`get_note_stats` calculates a whole-vault `backlink_count` while the other fields describe one
selected note, heading, or block. On a vault with thousands of Markdown files, this hidden full
index can exceed the MCP request deadline even when the selected note is small.

## Design

Remove `backlink_count` from `NoteStatsResult` and from the `get_note_stats` implementation. The
tool will return only:

- `scope`;
- `word_count`;
- `character_count`;
- `line_count`.

Do not add a compatibility field, optional flag, or replacement scan. Backlink discovery remains
the responsibility of the dedicated backlink tools.

Delete backlink-count helpers that become unused. Update query tests, MCP server tests, generated
tool documentation, and schema assertions so they explicitly verify that `backlink_count` is not
part of note statistics.

## Verification

- focused query and MCP tests pass;
- generated tool documentation is current;
- the direct `get-note-stats` command for a note in a large vault completes without indexing the
  full vault;
- the complete test suite and Clippy pass.

## Out of Scope

- changing dedicated backlink tools;
- adding a persistent link index;
- changing note word, character, line, or scope semantics.
