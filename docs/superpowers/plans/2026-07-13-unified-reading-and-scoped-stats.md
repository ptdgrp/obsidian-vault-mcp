# Unified Reading and Scoped Stats Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `read_section` with ref- and selector-aware `read_note`, and scope `get_note_stats` to a heading or block reference.

**Architecture:** Keep `SectionSelector` crate-private. Convert an input ref fragment or exactly one explicit selector to that type, select through `section_source`, and apply the Unicode `max_chars` budget once to the selected slice. Stats follows the same selection path; whole-note backlinks retain existing behavior while selected stats count only explicit matching fragments.

**Tech Stack:** Rust 2024, Clap, RMCP schemas, existing parser/resolver, Cargo tests.

## Global Constraints

- Bare refs support whole notes, headings, blocks, `#L1`, `#L1-L20`, and `#L1-`.
- Explicit `heading`, `block_id`, or `line` are mutually exclusive with a ref fragment and with each other.
- Remove public `read_section` and the read-only `max_output_bytes`; mutations retain `--line`.
- Selected reads use Unicode-character `max_chars`, retain the complete selected `source`, and mark truncation.
- Scoped stats support heading and block, never a line selector; unfragmented links do not count toward selected backlinks.

---

### Task 1: Write failing unified-read tests

**Files:**
- Modify: `src/query/tests/mod.rs`
- Modify: `src/query/tests/search_and_section.rs`

**Interfaces:**
- Consumes the proposed `VaultQueries::read_note(note, max_chars, selector)`.
- Produces regression coverage for heading, block, line, open-ended line, conflicts, and scoped truncation.

- [ ] **Step 1: Add failing tests**

Add a test that calls:

```rust
let heading = queries.read_note("发动机#原理", None, None)?;
assert_eq!(heading.content, "## 原理\n\n链接到 [[林动]]\n");
assert_eq!(heading.source.line_start, 3);

let to_end = queries.read_note("发动机#L1-", None, None)?;
assert_eq!(to_end.source.line_end, 5);

let error = queries.read_note(
    "发动机#原理",
    None,
    Some(SectionSelector::Block { block_id: "state".to_string() }),
).expect_err("conflicting selectors");
assert!(error.to_string().contains("selector"));
```

Add a selected Unicode content case with `max_chars: Some(2)`; assert two characters, `truncated == true`, and an unshortened selected `source`.

- [ ] **Step 2: Verify RED**

Run: `cargo test read_note_accepts_heading_block_and_line_ref_scopes`

Expected: compilation failure because the current method has two arguments and `ReadNoteResult` has no `source`.

- [ ] **Step 3: Commit tests**

```bash
git add src/query/tests/mod.rs src/query/tests/search_and_section.rs
git commit -m "test: cover unified note reading"
```

### Task 2: Implement private selector conversion and read selection

**Files:**
- Modify: `src/query.rs:49-58,640-710`
- Modify: `src/query/section.rs:1-105`
- Modify: `src/query/notes.rs:1-55`

**Interfaces:**
- Produces `pub(crate) fn selector_from_reference(&Option<ReferenceInfo>) -> anyhow::Result<Option<SectionSelector>>`.
- Produces `read_note(&self, note: &str, max_chars: Option<usize>, selector: Option<SectionSelector>)`.
- Produces `ReadNoteResult { path, source, content, truncated, next_step }`.

- [ ] **Step 1: Make result and selector private to the crate**

Replace the read result with:

```rust
pub struct ReadNoteResult {
    pub path: String,
    pub source: SourceSpan,
    pub content: String,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_step: Option<String>,
}
```

Delete `ReadSectionResult` and all serialization/schema code that only exposed `SectionSelector`; retain `Heading`, `Block`, and `Lines` for internal edits and reads.

- [ ] **Step 2: Convert reference fragments**

Add this in `src/query/section.rs`:

```rust
pub(crate) fn selector_from_reference(
    reference: &Option<ReferenceInfo>,
) -> anyhow::Result<Option<SectionSelector>> {
    match reference {
        None => Ok(None),
        Some(ReferenceInfo::BlockId { value }) => Ok(Some(SectionSelector::Block {
            block_id: value.clone(),
        })),
        Some(ReferenceInfo::Heading { value }) => selector_from_fragment(value),
        Some(ReferenceInfo::MultiHeading { value }) => Ok(Some(SectionSelector::Heading {
            heading: value.join("/"),
        })),
    }
}
```

Implement `selector_from_fragment` so only an exact `L` plus digits, optionally followed by `-` and digits, is a line selector. Thus `L1`, `L1-L20`, and `L1-` become `Lines { 1, 1 }`, `Lines { 1, 20 }`, and `Lines { 1, u64::MAX }`; values such as `Loom` and `L1-appendix` remain heading text. Reject numeric line selectors with zero or reversed bounds using `invalid line range`.

- [ ] **Step 3: Select and truncate once**

At the start of `read_note`, parse with `RefResolver::parse_ref(note)`, reject a parsed fragment plus an explicit selector, and choose the selector. Resolve the file, parse it, and use either `section_source` or `source_for_line(..., 1, total_lines)`. Then use:

```rust
let selected = slice_text(&content, source.byte_start, source.byte_end);
let budget = max_chars.unwrap_or(self.vault.config.max_read_note_chars);
let truncated = selected.chars().count() > budget;
let content = if truncated {
    truncate_chars(&selected, budget)
} else {
    selected
};
```

Change the truncated-message guidance to retry `read_note` with a heading, block, or line ref.

- [ ] **Step 4: Verify GREEN**

Run: `cargo test read_note_ && cargo test query::tests::search_and_section`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/query.rs src/query/section.rs src/query/notes.rs src/query/tests
git commit -m "feat: unify note reads by reference"
```

### Task 3: Remove public read_section and migrate adapters

**Files:**
- Modify: `src/server.rs`, `src/main.rs`, `src/vault.rs`
- Modify: `src/server/tests.rs`, `src/server/tests/dispatch_more.rs`, `tests/cli.rs`
- Modify: `README.md`, `docs/tools.md`

**Interfaces:**
- Produces `ReadNoteRequest { note, max_chars, heading, block_id, line }`.
- Removes `ReadSectionRequest`, `read_section` server/CLI handlers, and `max_output_bytes`.

- [ ] **Step 1: Add failing public-surface tests**

Assert the `read_note` schema has `heading`, `block_id`, `line`, and `max_chars`, and no tool definition is named `read_section`. Add a dispatch request with `note: "发动机"`, `heading: Some("原理")`, `max_chars: Some(4)`; assert line 3 and truncation.

- [ ] **Step 2: Verify RED**

Run: `cargo test read_note_schema_exposes_selectors_and_read_section_is_absent`

Expected: FAIL because current read-note request lacks selector fields and read-section is registered.

- [ ] **Step 3: Migrate MCP and CLI**

Add this adapter beside `section_parts` and call it from the read-note MCP and CLI paths:

```rust
fn read_note_parts(
    note: String,
    heading: Option<String>,
    block_id: Option<String>,
    line: Option<String>,
) -> Result<(String, Option<SectionSelector>), String> {
    if heading.is_none() && block_id.is_none() && line.is_none() {
        return Ok((note, None));
    }
    section_parts(note, heading, block_id, line).map(|(note, selector)| (note, Some(selector)))
}
```

Delete `ReadSectionRequest`, server handler, CLI subcommand, tests, and imports. Add `--heading`, `--block-id`, `--line`, and per-request `--max-chars` to `Command::ReadNote`. Delete `max_output_bytes` from CLI, `VaultConfig`, and config construction. Regenerate docs with `cargo run -- generate-docs` and replace README examples with bare read-note refs.

- [ ] **Step 4: Verify and commit**

Run: `cargo test server::tests && cargo test --test cli && cargo run -- generate-docs --check`

Expected: PASS.

```bash
git add src/server.rs src/main.rs src/vault.rs src/server/tests.rs src/server/tests tests/cli.rs README.md docs/tools.md
git commit -m "feat: replace read section with unified read note"
```

### Task 4: Add scoped get_note_stats

**Files:**
- Modify: `src/query/notes.rs`, `src/query/links.rs`, `src/query.rs`
- Modify: `src/query/tests/mod.rs`, `src/server/tests/dispatch_more.rs`

**Interfaces:**
- Produces optional `source: Option<SourceSpan>` on `NoteStatsResult`.
- Produces a scoped backlink-count helper that receives resolved target path, parsed target, selected selector, and selected source.

- [ ] **Step 1: Write failing stats test**

Create a target with heading `甲`, block `state`, and inbound links `[[目标#甲]]`, `[[目标#^state]]`, and `[[目标]]`. Assert heading and block stats count only their selected text and exactly one inbound link; whole-note stats count all three and omit `source`.

- [ ] **Step 2: Verify RED**

Run: `cargo test get_note_stats_scopes_text_and_backlinks_to_ref`

Expected: FAIL because current stats always count the entire file and all path backlinks.

- [ ] **Step 3: Implement selection-aware stats**

Parse the input ref, derive only heading/block selection through `selector_from_reference`, and calculate counts from `slice_text(content, source.byte_start, source.byte_end)`. For scoped backlinks, first keep the existing `RefResolver::link_matches` path check. A block counts only matching `ReferenceInfo::BlockId`. A heading counts only an inbound heading/multi-heading reference which resolves through `find_selectable_heading` to the same selected heading line. A no-fragment inbound link never matches a selected scope.

- [ ] **Step 4: Verify and commit**

Run: `cargo test get_note_stats_scopes_text_and_backlinks_to_ref && cargo test note_stats_tool_returns_word_character_and_backlink_counts`

Expected: PASS.

```bash
git add src/query.rs src/query/notes.rs src/query/links.rs src/query/tests src/server/tests
git commit -m "feat: scope note stats by reference"
```
