# Query edit-distance extraction design

## Goal

Remove the duplicate character-based Levenshtein implementations used by query
suggestion paths without changing their output.

## Design

Add a private `query::edit_distance` module exposing
`pub(super) fn levenshtein_distance(left: &str, right: &str) -> usize`.
Both unresolved-note and unresolved-heading suggestion paths call that
function. The implementation retains character-based comparison, including
Unicode input, and uses the existing row-based dynamic-programming algorithm.

The module stays below `query` rather than in a crate-wide `utils` module
because its only consumers are query-specific suggestion flows.

## Non-goals

- No changes to suggestion ranking, distance thresholds, deduplication, or
  error text.
- No public API or dependency changes.

## Verification

Add focused module tests for empty strings, replacement/insertion/deletion,
and Unicode characters. Run the query test suite and the full project tests.
