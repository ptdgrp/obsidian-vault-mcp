# Note suggestion prefix matching

## Scope

Improve only the fallback suggestion shown for an unresolved note reference.
The resolver itself remains strict: it resolves only explicit existing paths and
valid Obsidian references. A filename-prefix candidate must never be opened or
otherwise treated as a resolved note.

## Candidate ranking

Given an unresolved reference, derive its requested relative path, parent
directory, extension, and filename stem. Select suggestions in this order:

1. An exact relative-path match.
2. Candidates whose parent directory exactly matches the requested parent.
3. Among those candidates, require an exactly matching extension.
4. Prefer an exactly matching filename stem.
5. If no exact-stem candidate exists, prefer filename stems beginning with the
   requested stem followed by a separator. The separator is `-`.
6. If neither exact nor prefix candidates exist, fall back to the existing
   bounded Levenshtein-distance suggestion.

For example, a request for
`正文/vol01-卷一/章节/ch005.md` may suggest
`正文/vol01-卷一/章节/ch005-归途与北声点火.md`. It must not match a file in
another directory, a non-Markdown extension, or a stem such as `ch005a`.

## Multiple candidates

When more than one candidate has the best prefix rank, expose every candidate
in the unresolved-reference error message instead of selecting arbitrarily.
Candidates are naturally sorted for deterministic output.

The structured `resolve_ref` result remains a single optional
`suggested_target`; it receives a suggestion only when the best candidate is
unique. Multiple candidates remain unresolved without a single suggested
target.

## Tests

Add focused tests for:

- a same-directory, same-extension prefix candidate outranking an edit-distance
  candidate;
- directory, extension, and separator constraints;
- deterministic listing of multiple prefix candidates;
- unchanged strict resolution (the prefix candidate is still unresolved).

## MCP output-schema consistency

Keep output schemas strict for collections that are part of a tool's stable
result shape. In particular, `get_outlinks` must always serialize
`ambiguous_targets` and `unresolved_targets`; an empty group is represented by
an empty array, not by an omitted property.

Audit every output type that conditionally omits serialized properties against
its generated JSON Schema. A property may be omitted only when the schema also
marks it optional. Add a regression test that exercises tool outputs with empty
collections and verifies that every required output property is present in the
serialized structured result.
