# Query Note Suggestion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Share the query edit-distance implementation and offer a useful file-path suggestion whenever a query cannot locate the note referenced by an Obsidian ref.

**Architecture:** Keep a private `query::edit_distance` module for character-based Levenshtein distance. Keep note-candidate policy at the existing `find_indexed_note` boundary, so every operation that resolves a single input note through `find_indexed_note` or `resolve_note_path` returns the same error suggestion while strict `resolve_ref` remains unchanged.

**Tech Stack:** Rust 2024, existing `tempfile`-backed query tests, Cargo test runner.

## Global Constraints

- Compare `ObsidianRef::target`, never `ObsidianRef::raw`, when finding a file candidate.
- Prefer exactly one same-stem file even when the requested folder is wrong; never choose arbitrarily when multiple files have that stem.
- Keep the `resolve_ref` unresolved contract unchanged.
- Preserve character-based Unicode semantics and introduce no dependency or public API changes.

---

### Task 1: Add end-to-end regressions for failed note lookup

**Files:**
- Modify: `src/query/tests/edge_cases.rs`

**Interfaces:**
- Consumes: `VaultQueries::read_note(&str, Option<usize>) -> anyhow::Result<ReadNoteResult>` and `VaultQueries::read_section(&str, SectionSelector) -> anyhow::Result<ReadSectionResult>`.
- Produces: regression coverage for all callers that share `resolve_note_path`.

- [ ] **Step 1: Write the failing test**

Add this test after `read_note_reports_unresolved_reference_for_missing_note`:

```rust
#[test]
fn note_lookups_suggest_unique_filename_when_obsidian_ref_has_wrong_directory() {
    let (dir, queries) = fixture();
    fs::create_dir(dir.path().join("正确目录")).expect("create note directory");
    fs::write(
        dir.path().join("正确目录/唯一笔记.md"),
        "# 唯一笔记\n\n## 章节\n\n内容\n",
    )
    .expect("write note");

    let reference = "[[错误目录/唯一笔记#章节|显示名]]";
    let read_note_error = queries
        .read_note(reference, None)
        .expect_err("missing reference should suggest the actual file");
    let read_section_error = queries
        .read_section(
            reference,
            SectionSelector::Heading {
                heading: "章节".to_string(),
            },
        )
        .expect_err("section lookup should share the file suggestion");

    for error in [read_note_error, read_section_error] {
        let message = error.to_string();
        assert!(message.contains("unresolved note reference"));
        assert!(message.contains(reference));
        assert!(message.contains("正确目录/唯一笔记"));
    }
}
```

- [ ] **Step 2: Run the test to verify it fails for the intended reason**

Run: `cargo test query::tests::edge_cases::note_lookups_suggest_unique_filename_when_obsidian_ref_has_wrong_directory`

Expected: FAIL because the current candidate distance is calculated from the raw `[[...#...|...]]` string and therefore does not suggest `正确目录/唯一笔记`.

- [ ] **Step 3: Commit the regression test**

```bash
git add src/query/tests/edge_cases.rs
git commit -m "test: cover note suggestions for wrong directories"
```

### Task 2: Extract the shared edit-distance helper

**Files:**
- Create: `src/query/edit_distance.rs`
- Modify: `src/query.rs:1-12`
- Modify: `src/query/section.rs:1-5,243-270`

**Interfaces:**
- Produces: `pub(super) fn levenshtein_distance(left: &str, right: &str) -> usize`.
- Consumed by: `find_indexed_note` in `src/query.rs` and `closest_heading_suggestions` in `src/query/section.rs`.

- [ ] **Step 1: Add focused helper tests before its implementation**

Create `src/query/edit_distance.rs` with this test module first:

```rust
#[cfg(test)]
mod tests {
    use super::levenshtein_distance;

    #[test]
    fn measures_empty_ascii_and_unicode_inputs_by_characters() {
        assert_eq!(levenshtein_distance("", "笔记"), 2);
        assert_eq!(levenshtein_distance("kitten", "sitting"), 3);
        assert_eq!(levenshtein_distance("笔记", "笔迹"), 1);
    }
}
```

Add `mod edit_distance;` beside the other `query` child-module declarations in `src/query.rs`.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test query::edit_distance::tests::measures_empty_ascii_and_unicode_inputs_by_characters`

Expected: FAIL to compile because `levenshtein_distance` has not yet been defined.

- [ ] **Step 3: Implement the minimal shared helper and migrate heading suggestions**

Place this definition above the test module in `src/query/edit_distance.rs`:

```rust
pub(super) fn levenshtein_distance(left: &str, right: &str) -> usize {
    let left = left.chars().collect::<Vec<_>>();
    let right = right.chars().collect::<Vec<_>>();
    if left.is_empty() {
        return right.len();
    }
    if right.is_empty() {
        return left.len();
    }

    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    let mut current = vec![0; right.len() + 1];
    for (left_index, left_char) in left.iter().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_char) in right.iter().enumerate() {
            let substitution_cost = usize::from(left_char != right_char);
            current[right_index + 1] = (previous[right_index + 1] + 1)
                .min(current[right_index] + 1)
                .min(previous[right_index] + substitution_cost);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}
```

In `src/query/section.rs`, import it with:

```rust
use super::{ReadSectionResult, SectionSelector, VaultQueries, edit_distance::levenshtein_distance, truncate_utf8};
```

Replace `edit_distance(...)` in `closest_heading_suggestions` with `levenshtein_distance(...)`, then delete the local `fn edit_distance`.

- [ ] **Step 4: Run focused tests to verify they pass**

Run: `cargo test query::edit_distance::tests::measures_empty_ascii_and_unicode_inputs_by_characters && cargo test query::tests::search_and_section`

Expected: PASS; Unicode distance and heading suggestion behavior stay covered.

- [ ] **Step 5: Commit the extraction**

```bash
git add src/query.rs src/query/edit_distance.rs src/query/section.rs
git commit -m "refactor: share query edit distance"
```

### Task 3: Apply shared candidate selection to all note-location boundaries

**Files:**
- Modify: `src/query.rs:927-1027`
- Test: `src/query/tests/edge_cases.rs`

**Interfaces:**
- Consumes: `RefResolver::resolve(note, notes) -> ResolveResult` and `ObsidianRef { raw, target, .. }`.
- Consumes: `edit_distance::levenshtein_distance(&str, &str) -> usize`.
- Produces: unchanged `find_indexed_note` return type, with a more accurate error message for unresolved note input.

- [ ] **Step 1: Implement a private candidate helper**

Add this helper near `find_indexed_note` in `src/query.rs`:

```rust
fn suggested_note_path(target: &str, notes: &[IndexedNote]) -> Option<String> {
    let target = target.trim_end_matches(".md");
    let target_stem = target.rsplit('/').next().unwrap_or(target);
    let exact_stems = notes
        .iter()
        .filter(|note| {
            note.file
                .relative_path
                .trim_end_matches(".md")
                .rsplit('/')
                .next()
                == Some(target_stem)
        })
        .collect::<Vec<_>>();
    if let [note] = exact_stems.as_slice() {
        return Some(note.file.relative_path.trim_end_matches(".md").to_string());
    }

    notes
        .iter()
        .map(|note| {
            let path = note.file.relative_path.trim_end_matches(".md");
            (levenshtein_distance(target, path), path)
        })
        .min_by_key(|(distance, _)| *distance)
        .filter(|(distance, path)| *distance <= 3 && !path.is_empty())
        .map(|(_, path)| path.to_string())
}
```

In the `ResolveResult::Unresolved` branch of `find_indexed_note`, replace the
inline loop with:

```rust
if let Some(closest) = suggested_note_path(&reference.target, notes) {
    return Err(anyhow::anyhow!(
        "unresolved note reference: {:?} (did you mean {:?})",
        reference.raw,
        closest
    ));
}
Err(anyhow::anyhow!("unresolved note reference: {:?}", reference.raw))
```

Delete the old local `levenshtein_distance` from `src/query.rs` and import the
shared helper using `use self::edit_distance::levenshtein_distance;`.

- [ ] **Step 2: Run both regressions and the query test suite**

Run: `cargo test query::tests::edge_cases::note_lookups_suggest_unique_filename_when_obsidian_ref_has_wrong_directory && cargo test query::tests`

Expected: PASS. The first command proves the shared note-location path serves
both `read_note` and `read_section`; the second protects all query callers that
use the same boundaries.

- [ ] **Step 3: Run formatting and the full suite**

Run: `cargo fmt --check && cargo test`

Expected: both commands exit 0.

- [ ] **Step 4: Commit the candidate behavior**

```bash
git add src/query.rs src/query/tests/edge_cases.rs
git commit -m "feat: suggest notes for unresolved refs"
```
