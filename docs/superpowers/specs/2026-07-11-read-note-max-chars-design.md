# `read_note` character-limit migration

## Goal

Make the `read_note` limit use the same Unicode-character unit exposed by
`get_note_stats.character_count`. This is a breaking API change.

## Public contract

- Rename the MCP request field from `max_bytes` to `max_chars`.
- Rename the CLI option from `--max-read-note-bytes` to
  `--max-read-note-chars`; it accepts an unsigned integer, with no byte-size
  suffix parsing.
- Rename the configuration field and default constant to
  `max_read_note_chars` and `DEFAULT_MAX_READ_NOTE_CHARS`.
- Do not retain `max_bytes` or the old CLI/configuration names as aliases.
- Preserve the numeric default of `4096`, whose meaning changes from bytes to
  Unicode scalar values.

## Implementation

`read_note` selects its per-request value or configured default, compares it
with `content.chars().count()`, and truncates through `content.chars().take()`.
The resulting `String` is valid UTF-8 and the content prefix remains in source
order. Tool guidance and generated documentation will refer to `max_chars`.

## Testing

Regression coverage will prove that a character budget counts multibyte
characters as one each, that per-request overrides take precedence over the
configured default, and that the old byte-oriented terms do not remain in the
public generated schema or documentation. The full test suite and documentation
freshness check will run after the migration.
