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
fn search_text_returns_compact_fixed_pages_and_one_match_per_line() {
    let (dir, queries) = fixture();
    let lines = (1..=51)
        .map(|index| format!("line {index:03} needle needle"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(dir.path().join("分页搜索.md"), format!("{lines}\n")).expect("write search page");

    let page_1 = queries
        .search_text("needle", true, &[], &[], 1)
        .expect("search page 1");
    let page_2 = queries
        .search_text("needle", true, &[], &[], 2)
        .expect("search page 2");
    let out_of_range = queries
        .search_text("needle", true, &[], &[], 3)
        .expect("search page 3");
    let empty = queries
        .search_text("absent", true, &[], &[], 1)
        .expect("empty search");

    let local_page_1 = page_1
        .matches
        .iter()
        .filter(|matched| matched.source.starts_with("分页搜索.md#L"))
        .collect::<Vec<_>>();
    assert_eq!(local_page_1.len(), 50);
    assert_eq!(local_page_1[0].source, "分页搜索.md#L1");
    assert_eq!(local_page_1[49].source, "分页搜索.md#L50");
    assert_eq!(page_1.pagination.page, 1);
    assert_eq!(page_1.pagination.total_pages, 2);
    assert_eq!(page_1.pagination.total_matches, 51);
    assert_eq!(
        page_2.matches,
        vec![crate::query::TextMatch {
            source: "分页搜索.md#L51".to_string(),
            preview: "line 051 needle needle".to_string(),
        }]
    );
    assert_eq!(out_of_range.matches.len(), 0);
    assert_eq!(out_of_range.pagination.total_pages, 2);
    assert_eq!(empty.pagination.total_matches, 0);
    assert!(queries.search_text("needle", true, &[], &[], 0).is_err());

    let value = serde_json::to_value(&page_1).expect("search json");
    assert!(value.get("query").is_none());
    assert!(value.get("truncated").is_none());
    assert!(value["matches"][0]["source"].is_string());
    assert!(value["matches"][0].get("snippet").is_none());
}

#[test]
fn search_regex_returns_compact_fixed_pages_and_one_match_per_line() {
    let (dir, queries) = fixture();
    fs::write(
        dir.path().join("正则分页.md"),
        "alpha 01 beta 01\nalpha 02 beta 02\n",
    )
    .expect("write regex page");

    let result = queries
        .search_regex("alpha \\d+|beta \\d+", true, &[], &[], 1)
        .expect("regex page");

    let matches = result
        .matches
        .iter()
        .filter(|matched| matched.source.starts_with("正则分页.md#L"))
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].source, "正则分页.md#L1");
    assert_eq!(matches[1].source, "正则分页.md#L2");
    assert_eq!(result.pagination.total_matches, 2);
    assert!(queries.search_regex("(", true, &[], &[], 1).is_err());
    assert!(queries.search_regex("alpha", true, &[], &[], 0).is_err());

    let value = serde_json::to_value(&result).expect("regex json");
    assert!(value.get("pattern").is_none());
    assert!(value.get("truncated").is_none());
}

#[test]
fn search_preview_is_centered_on_first_match_and_unicode_safe() {
    let (dir, queries) = fixture();
    fs::write(
        dir.path().join("预览.md"),
        format!(
            "{}needle{} later needle\nneedle{}\nshort needle line\n",
            "前".repeat(180),
            "后".repeat(180),
            "尾".repeat(300)
        ),
    )
    .expect("write preview fixture");

    let result = queries
        .search_text("needle", true, &[], &["林动.md".to_string()], 1)
        .expect("preview search");
    let previews = result
        .matches
        .iter()
        .filter(|matched| matched.source.starts_with("预览.md#L"))
        .map(|matched| matched.preview.as_str())
        .collect::<Vec<_>>();

    assert_eq!(previews.len(), 3);
    assert!(previews[0].contains("needle"));
    assert!(previews[0].chars().count() <= 240);
    assert!(previews[0].starts_with("..."));
    assert!(previews[0].ends_with("..."));
    assert_eq!(previews[0].matches("needle").count(), 1);
    assert!(!previews[1].starts_with("..."));
    assert!(previews[1].ends_with("..."));
    assert_eq!(previews[2], "short needle line");
}

#[test]
fn searches_report_invalid_include_and_exclude_globs() {
    let (_dir, queries) = fixture();

    let include_error = queries
        .search_regex("林动", false, &["[".to_string()], &[], 1)
        .expect_err("invalid include glob should fail");
    assert!(include_error.to_string().contains("include"));
    assert!(include_error.to_string().contains('['));

    let exclude_error = queries
        .search_text("林动", false, &[], &["[".to_string()], 1)
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
        .search_text("needle", false, &[], &[], 1)
        .expect("case-insensitive text search");
    let sensitive_text = queries
        .search_text("needle", true, &[], &[], 1)
        .expect("case-sensitive text search");
    let insensitive_regex = queries
        .search_regex("needle", false, &[], &[], 1)
        .expect("case-insensitive regex search");
    let sensitive_regex = queries
        .search_regex("needle", true, &[], &[], 1)
        .expect("case-sensitive regex search");

    assert_eq!(
        insensitive_text
            .matches
            .iter()
            .filter(|matched| matched.source.starts_with("大小写.md#L"))
            .count(),
        2
    );
    assert_eq!(
        sensitive_text
            .matches
            .iter()
            .filter(|matched| matched.source.starts_with("大小写.md#L"))
            .count(),
        1
    );
    assert_eq!(
        insensitive_regex
            .matches
            .iter()
            .filter(|matched| matched.source.starts_with("大小写.md#L"))
            .count(),
        2
    );
    assert_eq!(
        sensitive_regex
            .matches
            .iter()
            .filter(|matched| matched.source.starts_with("大小写.md#L"))
            .count(),
        1
    );
    assert_eq!(
        sensitive_text.matches.last().unwrap().source,
        "大小写.md#L4"
    );
    assert_eq!(
        sensitive_regex.matches.last().unwrap().source,
        "大小写.md#L4"
    );
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
        .search_regex("start.*end", true, &[], &[], 1)
        .expect("line-oriented regex search");
    let anchored = queries
        .search_regex("^alpha omega$", true, &[], &[], 1)
        .expect("anchored regex search");

    assert!(
        !across_lines
            .matches
            .iter()
            .any(|matched| matched.source.starts_with("逐行.md#L"))
    );
    let anchored_match = anchored
        .matches
        .iter()
        .find(|matched| matched.source == "逐行.md#L3")
        .expect("anchored line match");
    assert_eq!(anchored_match.preview, "alpha omega");
}

#[test]
fn empty_text_and_regex_queries_match_each_line() {
    let (dir, queries) = fixture();
    fs::write(dir.path().join("空查询.md"), "first\nsecond\n").expect("write empty query fixture");

    let text = queries
        .search_text("", true, &[], &[], 1)
        .expect("empty text search");
    let regex = queries
        .search_regex("", true, &[], &[], 1)
        .expect("empty regex search");

    assert_eq!(
        text.matches
            .iter()
            .filter(|matched| matched.source.starts_with("空查询.md#L"))
            .count(),
        2
    );
    assert_eq!(
        regex
            .matches
            .iter()
            .filter(|matched| matched.source.starts_with("空查询.md#L"))
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
