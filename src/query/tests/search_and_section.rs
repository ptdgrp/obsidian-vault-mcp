use std::fs;

use super::fixture;
use crate::query::SectionSelector;

#[test]
fn read_note_accepts_heading_block_and_line_ref_scopes() {
    let (dir, queries) = fixture();
    fs::write(
        dir.path().join("发动机.md"),
        "# 发动机\n\n## 原理\n\n链接到 [[林动]] ^state\n",
    )
    .expect("write scoped fixture");

    let heading = queries
        .read_note("发动机#原理", None, None)
        .expect("read heading ref");
    assert_eq!(heading.content, "## 原理\n\n链接到 [[林动]] ^state\n");
    assert_eq!(heading.source, "发动机.md#L3-L5");

    let block = queries
        .read_note("发动机#^state", None, None)
        .expect("read block ref");
    assert_eq!(block.source, "发动机.md#L5");

    let single_line = queries
        .read_note("发动机#L3", None, None)
        .expect("read single line ref");
    assert_eq!(single_line.content, "## 原理\n");

    let bounded = queries
        .read_note("发动机#L3-L5", None, None)
        .expect("read bounded line ref");
    assert_eq!(bounded.source, "发动机.md#L3-L5");

    let to_end = queries
        .read_note("发动机#L1-", None, None)
        .expect("read open-ended line ref");
    assert_eq!(to_end.source, "发动机.md#L1-L5");

    let error = queries
        .read_note(
            "发动机#原理",
            None,
            Some(SectionSelector::Block {
                block_id: "state".to_string(),
            }),
        )
        .expect_err("conflicting selectors");
    assert!(error.to_string().contains("selector"));
}

#[test]
fn read_note_truncates_selected_scope_by_unicode_characters() {
    let (dir, queries) = fixture();
    fs::write(
        dir.path().join("字符范围.md"),
        "# 字符范围\n\n## 部分\n\n甲乙丙\n",
    )
    .expect("write unicode scoped fixture");

    let result = queries
        .read_note("字符范围#部分", Some(2), None)
        .expect("read selected scope");

    assert_eq!(result.content, "##");
    assert!(result.truncated);
    assert_eq!(result.source, "字符范围.md#L3-L5");
}

#[test]
fn search_text_truncates_to_max_results_and_marks_truncated() {
    let (_dir, mut queries) = fixture();
    queries.vault.config.max_results = 1;

    let result = queries
        .search_text("林动", false, 0, &[], &[])
        .expect("search text");

    assert_eq!(result.matches.len(), 1);
    assert!(result.truncated);
    assert!(result.matches[0].snippet.contains("林动"));
}

#[test]
fn searches_report_invalid_include_and_exclude_globs() {
    let (_dir, queries) = fixture();

    let include_error = queries
        .search_regex("林动", false, 0, &["[".to_string()], &[])
        .expect_err("invalid include glob should fail");
    assert!(include_error.to_string().contains("include"));
    assert!(include_error.to_string().contains('['));

    let exclude_error = queries
        .search_text("林动", false, 0, &[], &["[".to_string()])
        .expect_err("invalid exclude glob should fail");
    assert!(exclude_error.to_string().contains("exclude"));
    assert!(exclude_error.to_string().contains('['));
}

#[test]
fn searches_honor_case_sensitivity() {
    let (dir, queries) = fixture();
    fs::write(dir.path().join("大小写.md"), "# Case\n\nNeedle\nneedle\n")
        .expect("write case fixture");

    let insensitive_text = queries
        .search_text("needle", false, 0, &[], &[])
        .expect("case-insensitive text search");
    let sensitive_text = queries
        .search_text("needle", true, 0, &[], &[])
        .expect("case-sensitive text search");
    let insensitive_regex = queries
        .search_regex("needle", false, 0, &[], &[])
        .expect("case-insensitive regex search");
    let sensitive_regex = queries
        .search_regex("needle", true, 0, &[], &[])
        .expect("case-sensitive regex search");

    assert_eq!(insensitive_text.matches.len(), 2);
    assert_eq!(sensitive_text.matches.len(), 1);
    assert_eq!(insensitive_regex.matches.len(), 2);
    assert_eq!(sensitive_regex.matches.len(), 1);
    assert_eq!(sensitive_text.matches[0].source.line_start, 4);
    assert_eq!(sensitive_regex.matches[0].source.line_start, 4);
}

#[test]
fn search_context_lines_clamp_at_file_boundaries() {
    let (dir, queries) = fixture();
    fs::write(
        dir.path().join("上下文.md"),
        "first needle\nsecond\nthird\nlast needle\n",
    )
    .expect("write context fixture");

    let result = queries
        .search_text("needle", true, 2, &[], &[])
        .expect("search with context");

    let matches = result
        .matches
        .iter()
        .filter(|matched| matched.source.path == "上下文.md")
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), 2);
    assert_eq!(
        (matches[0].source.line_start, matches[0].source.line_end),
        (1, 3)
    );
    assert_eq!(matches[0].snippet, "first needle\nsecond\nthird\n");
    assert_eq!(
        (matches[1].source.line_start, matches[1].source.line_end),
        (2, 4)
    );
    assert_eq!(matches[1].snippet, "second\nthird\nlast needle\n");
}

#[test]
fn search_results_are_stably_sorted_before_global_truncation() {
    let (dir, mut queries) = fixture();
    fs::write(dir.path().join("10.md"), "needle\nneedle\n").expect("write 10");
    fs::write(dir.path().join("2.md"), "needle\nneedle\n").expect("write 2");
    queries.vault.config.max_results = 3;

    let result = queries
        .search_text("needle", true, 0, &[], &[])
        .expect("sorted truncated search");
    let locations = result
        .matches
        .iter()
        .map(|matched| (matched.source.path.as_str(), matched.source.line_start))
        .collect::<Vec<_>>();

    assert_eq!(locations, vec![("2.md", 1), ("2.md", 2), ("10.md", 1)]);
    assert!(result.truncated);
}

#[test]
fn search_snippets_truncate_by_unicode_characters() {
    let (dir, queries) = fixture();
    let content = format!("needle{}\n", "界".repeat(300));
    fs::write(dir.path().join("长片段.md"), content).expect("write long snippet");

    let result = queries
        .search_text("needle", true, 0, &[], &[])
        .expect("search long Unicode line");
    let matched = result
        .matches
        .iter()
        .find(|matched| matched.source.path == "长片段.md")
        .expect("long snippet match");

    assert_eq!(matched.snippet.chars().count(), 243);
    assert!(matched.snippet.starts_with("needle"));
    assert!(matched.snippet.ends_with("..."));
}

#[test]
fn regex_search_is_line_oriented_and_supports_line_anchors() {
    let (dir, queries) = fixture();
    fs::write(
        dir.path().join("逐行.md"),
        "alpha start\nend omega\nalpha omega\n",
    )
    .expect("write line-oriented fixture");

    let across_lines = queries
        .search_regex("start.*end", true, 0, &[], &[])
        .expect("line-oriented regex search");
    let anchored = queries
        .search_regex("^alpha omega$", true, 0, &[], &[])
        .expect("anchored regex search");

    assert!(
        !across_lines
            .matches
            .iter()
            .any(|matched| matched.source.path == "逐行.md")
    );
    let anchored_match = anchored
        .matches
        .iter()
        .find(|matched| matched.source.path == "逐行.md")
        .expect("anchored line match");
    assert_eq!(anchored_match.source.line_start, 3);
    assert_eq!(anchored_match.source.line_end, 3);
}

#[test]
fn empty_text_and_regex_queries_match_each_line() {
    let (dir, queries) = fixture();
    fs::write(dir.path().join("空查询.md"), "first\nsecond\n").expect("write empty query fixture");

    let text = queries
        .search_text("", true, 0, &[], &[])
        .expect("empty text search");
    let regex = queries
        .search_regex("", true, 0, &[], &[])
        .expect("empty regex search");

    assert_eq!(
        text.matches
            .iter()
            .filter(|matched| matched.source.path == "空查询.md")
            .count(),
        2
    );
    assert_eq!(
        regex
            .matches
            .iter()
            .filter(|matched| matched.source.path == "空查询.md")
            .count(),
        2
    );
}

#[test]
fn read_note_supports_explicit_selectors_and_selected_truncation() {
    let (dir, queries) = fixture();
    fs::write(dir.path().join("块.md"), "# 块\n\n段落\n^state\n").expect("write block note");

    let result = queries
        .read_note(
            "块",
            None,
            Some(SectionSelector::Block {
                block_id: "state".to_string(),
            }),
        )
        .expect("read block");

    assert_eq!(result.source, "块.md#L3-L4");
    assert_eq!(result.content, "段落\n^state\n");
    assert!(!result.truncated);

    let truncated = queries
        .read_note(
            "发动机",
            Some(5),
            Some(SectionSelector::Heading {
                heading: "原理".to_string(),
            }),
        )
        .expect("read heading");
    assert!(truncated.truncated);
    assert_eq!(truncated.content, "## 原理");
}

#[test]
fn read_note_clamps_line_ranges_to_existing_lines() {
    let (_dir, queries) = fixture();

    let result = queries
        .read_note(
            "发动机",
            None,
            Some(SectionSelector::Lines {
                line_start: 1,
                line_end: 99,
            }),
        )
        .expect("read lines");

    assert_eq!(result.source, "发动机.md#L1-L5");
    assert!(result.content.contains("## 原理"));
}

#[test]
fn read_note_rejects_level_one_heading_without_suggesting_it() {
    let (dir, queries) = fixture();
    fs::write(dir.path().join("根章节.md"), "# 根章节\n\n正文\n").expect("write root note");

    let error = queries
        .read_note(
            "根章节",
            None,
            Some(SectionSelector::Heading {
                heading: "根章节".to_string(),
            }),
        )
        .expect_err("level-one heading should not be selectable");

    assert_eq!(
        error.to_string(),
        "heading not found: \"根章节\". Note has no selectable headings; level-one headings are note titles. Use a lower-level heading, block id, or line selector."
    );
}

#[test]
fn read_note_accepts_markdown_heading_syntax() {
    let (_dir, queries) = fixture();

    let result = queries
        .read_note(
            "发动机",
            None,
            Some(SectionSelector::Heading {
                heading: "## 原理".to_string(),
            }),
        )
        .expect("read heading with markdown marker");

    assert_eq!(result.source, "发动机.md#L3-L5");
    assert!(result.content.contains("链接到 [[林动]]"));
}

#[test]
fn read_note_markdown_heading_syntax_requires_matching_level() {
    let (dir, queries) = fixture();
    fs::write(
        dir.path().join("同名标题.md"),
        "# Root\n\n## Target\n\nWrong level\n\n### Target\n\nRight level\n",
    )
    .expect("write note");

    let result = queries
        .read_note(
            "同名标题",
            None,
            Some(SectionSelector::Heading {
                heading: "### Target".to_string(),
            }),
        )
        .expect("read level-constrained heading");

    assert_eq!(result.source, "同名标题.md#L7-L9");
    assert!(result.content.contains("Right level"));
    assert!(!result.content.contains("Wrong level"));
}

#[test]
fn read_note_treats_trailing_heading_colons_as_optional() {
    let (dir, queries) = fixture();
    fs::write(
        dir.path().join("冒号.md"),
        "# Root\n\n### Heading:\n\nASCII colon\n\n### 中文：\n\nFullwidth colon\n\n## Heading\n\nWrong level\n",
    )
    .expect("write note");

    let ascii = queries
        .read_note(
            "冒号",
            None,
            Some(SectionSelector::Heading {
                heading: "### Heading".to_string(),
            }),
        )
        .expect("read ascii colon heading");
    assert!(ascii.content.contains("ASCII colon"));
    assert!(!ascii.content.contains("Wrong level"));

    let fullwidth = queries
        .read_note(
            "冒号",
            None,
            Some(SectionSelector::Heading {
                heading: "### 中文".to_string(),
            }),
        )
        .expect("read fullwidth colon heading");
    assert!(fullwidth.content.contains("Fullwidth colon"));
}

#[test]
fn read_note_suggests_available_headings_when_heading_is_missing() {
    let (dir, queries) = fixture();
    fs::write(
        dir.path().join("建议.md"),
        "# Engine\n\n## Principle\n\n## Character\n",
    )
    .expect("write note");

    let error = queries
        .read_note(
            "建议",
            None,
            Some(SectionSelector::Heading {
                heading: "Principl".to_string(),
            }),
        )
        .expect_err("missing heading should fail");

    assert_eq!(
        error.to_string(),
        "heading not found: \"Principl\". Did you mean: \"Principle\"?"
    );
}
