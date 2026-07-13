# Unified note reading design

## Goal

Replace the separate `read_note` and `read_section` read interfaces with one
bounded `read_note` interface whose bare reference selects a whole note,
heading section, block, or line range.

## Interface

`read_note` remains the only read-content MCP tool and CLI command. It accepts
`note` and optional `max_chars`; callers use these bare forms without `[[...]]`
wrappers:

- `Project/Plan` reads the full note.
- `Project/Plan#Progress` reads a heading section.
- `Project/Plan#^milestone` reads a block.
- `Project/Plan#L1` reads one line.
- `Project/Plan#L1-L20` reads an inclusive line range.

`#^` takes precedence for blocks, and a fragment exactly matching `L<number>`
or `L<number>-L<number>` takes precedence for line ranges. All other
fragments retain existing heading and nested-heading interpretation.

## Behavior

The query parses the bare reference, resolves the containing note, converts
the fragment into the existing private `SectionSelector`, then reuses
`section_source` for every selected scope. A whole-note request receives a
source span covering the complete file. `ReadNoteResult` always includes that
source span along with `path`, `content`, and `truncated`.

Every response, including selected heading, block, and line content, truncates
at `max_chars` Unicode characters. An omitted `max_chars` uses the existing
`max_read_note_chars` configuration. Truncation does not alter the reported
source range; the span identifies the selected source, while `truncated`
identifies incomplete returned content.

## Removal and retained internals

Remove `read_section` from MCP, CLI, public result/request types, generated
tool documentation, tests, README guidance, and internal read methods. Remove
the read-only `max_output_bytes` configuration and CLI flag because it is no
longer used.

Keep `SectionSelector` and line-range selection private to the crate for
structural edit operations and scoped stats. Their editing API, including
`--line`, remains unchanged.

## Errors and guidance

Heading and block failures use the existing selector diagnostics. Invalid line
references use the existing line-range diagnostics. The truncation next-step
message directs callers to retry `read_note` with a bare heading, block, or
line ref rather than naming `read_section`.

## Verification

Add tests for full note, heading, block, single-line, and line-range reads;
Unicode `max_chars` truncation for a selected scope; server schema and dispatch
coverage; removal of `read_section`; CLI behavior; and generated docs. Run
formatting, query tests, CLI tests, docs check, and the full Cargo test suite.
