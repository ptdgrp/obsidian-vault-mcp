use std::fs;

use super::fixture;
use crate::query::SectionSelector;

#[test]
fn search_text_truncates_to_max_results_and_marks_truncated() {
    let (_dir, mut queries) = fixture();
    queries.vault.config.max_results = 1;

    let result = queries.search_text("林动", false, 0).expect("search text");

    assert_eq!(result.matches.len(), 1);
    assert!(result.truncated);
    assert!(result.matches[0].snippet.contains("林动"));
}

#[test]
fn search_regex_reports_invalid_path_glob() {
    let (_dir, queries) = fixture();

    let error = queries
        .search_regex("林动", false, 0, Some("["))
        .expect_err("invalid glob should fail");

    assert!(error.to_string().contains("unclosed character class"));
}

#[test]
fn read_section_supports_block_selectors_and_truncates_large_output() {
    let (dir, mut queries) = fixture();
    fs::write(dir.path().join("块.md"), "# 块\n\n段落\n^state\n").expect("write block note");

    let result = queries
        .read_section(
            "块",
            SectionSelector::Block {
                block_id: "state".to_string(),
            },
        )
        .expect("read block");

    assert_eq!(result.source.path, "块.md");
    assert_eq!(result.source.line_start, 3);
    assert_eq!(result.source.line_end, 4);
    assert_eq!(result.content, "段落\n^state\n");
    assert!(!result.truncated);

    queries.vault.config.max_output_bytes = 9;
    let truncated = queries
        .read_section(
            "发动机",
            SectionSelector::Heading {
                heading: "原理".to_string(),
            },
        )
        .expect("read heading");
    assert!(truncated.truncated);
    assert_eq!(truncated.content, "## 原理");
}

#[test]
fn read_section_clamps_line_ranges_to_existing_lines() {
    let (_dir, queries) = fixture();

    let result = queries
        .read_section(
            "发动机",
            SectionSelector::Lines {
                line_start: 1,
                line_end: 99,
            },
        )
        .expect("read lines");

    assert_eq!(result.source.path, "发动机.md");
    assert_eq!(result.source.line_start, 1);
    assert_eq!(result.source.line_end, 5);
    assert!(result.content.contains("## 原理"));
}
