# Query edit-distance extraction design

## Goal

Remove the duplicate character-based Levenshtein implementations used by query
suggestion paths and improve the fallback suggestion shown when a note lookup
cannot find a file.

## Design

Add a private `query::edit_distance` module exposing
`pub(super) fn levenshtein_distance(left: &str, right: &str) -> usize`.
Both unresolved-note and unresolved-heading suggestion paths call that
function. The implementation retains character-based comparison, including
Unicode input, and uses the existing row-based dynamic-programming algorithm.

The module stays below `query` rather than in a crate-wide `utils` module
because its only consumers are query-specific suggestion flows.

The unresolved-note fallback remains in `find_indexed_note`, which is reached
by `read_note` after direct path lookup fails (and by other query operations
that first locate a note). It does not change `resolve_ref`, whose contract is
to report unresolved references without guessing.

For an Obsidian reference, candidate scoring uses `ObsidianRef::target`, not
the raw input. This excludes the `[[...]]` wrapper, heading/block fragment, and
display alias. A uniquely matching filename stem is suggested with its actual
vault-relative path even when the directory portion in the requested target is
wrong. Otherwise the existing bounded edit-distance suggestion compares the
target to vault-relative paths without `.md`.

## Non-goals

- No changes to `resolve_ref` semantics or public API.
- No arbitrary selection when multiple files share an exact filename stem.
- No public API or dependency changes.

## Verification

Add focused module tests for empty strings, replacement/insertion/deletion,
and Unicode characters. Add `read_note` regression tests for an unresolved
Obsidian reference whose filename exists under a different directory, including
a heading and display alias. Run the query test suite and the full project
tests.
