use std::fs;
use std::sync::Arc;

use camino::Utf8PathBuf;
use tempfile::tempdir;

use super::fixture;
use crate::parser::SectionInfo;
use crate::query::path_filter::PathFilter;
use crate::query::{SearchSource, SectionSelector, VaultQueries};
use crate::vault::{Vault, VaultConfig};

#[test]
fn path_filter_applies_include_union_and_exclude_precedence() {
    let filter = PathFilter::new(
        &["正文/**/*.md".to_string(), "资料/**/*.md".to_string()],
        &["**/草稿/**".to_string()],
    )
    .expect("valid filter");

    assert!(filter.is_match("正文/001.md"));
    assert!(filter.is_match("资料/设定.md"));
    assert!(!filter.is_match("正文/草稿/002.md"));
}

#[test]
fn path_filter_with_empty_patterns_accepts_every_path() {
    let filter = PathFilter::new(&[], &[]).expect("empty filters are valid");

    assert!(filter.is_match("正文/001.md"));
    assert!(filter.is_match("资料/设定.md"));
    assert!(filter.is_match("正文/草稿/002.md"));
}

#[test]
fn path_filter_reports_the_invalid_pattern_and_its_field() {
    let include_error =
        PathFilter::new(&["[".to_string()], &[]).expect_err("invalid include should fail");
    assert!(include_error.to_string().contains("include"));
    assert!(include_error.to_string().contains('['));

    let exclude_error =
        PathFilter::new(&[], &["[".to_string()]).expect_err("invalid exclude should fail");
    assert!(exclude_error.to_string().contains("exclude"));
    assert!(exclude_error.to_string().contains('['));
}

#[test]
fn resolve_ref_reports_ambiguous_candidates_for_duplicate_notes() {
    let (_dir, queries) = fixture();

    let result = queries.resolve_ref("[[发动机]]").expect("resolve ref");
    let value = serde_json::to_value(result).expect("resolve json");

    assert_eq!(
        value,
        serde_json::json!({
            "ambiguous_targets": ["发动机.md", "资料/发动机.md"]
        })
    );
}

#[test]
fn read_note_reports_unresolved_reference_for_missing_note() {
    let (_dir, queries) = fixture();

    let error = queries
        .read_note("缺失.md", None, None)
        .expect_err("missing note should fail");

    assert!(error.to_string().contains("unresolved note reference"));
}

#[test]
fn read_note_resolves_a_unique_nested_filename_with_markdown_extension() {
    let dir = tempdir().expect("tempdir");
    fs::create_dir(dir.path().join("人物")).expect("create note directory");
    fs::write(dir.path().join("人物/林动.md"), "# 林动\n").expect("write note");
    let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("UTF-8 temp path");
    let vault = Vault::open(&root, VaultConfig::default()).expect("vault");
    let queries = VaultQueries::new(Arc::new(vault));

    let result = queries
        .read_note("林动.md", None, None)
        .expect("resolve unique filename without its directory");

    assert_eq!(result.source, "人物/林动.md#L1");
    assert_eq!(result.content, "# 林动\n");
}

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
        .read_note(reference, None, None)
        .expect_err("missing reference should suggest the actual file");
    let conflict_error = queries
        .read_note(
            reference,
            None,
            Some(SectionSelector::Heading {
                heading: "章节".to_string(),
            }),
        )
        .expect_err("fragment and explicit selector should conflict");

    let message = read_note_error.to_string();
    assert!(message.contains("unresolved note reference"));
    assert!(message.contains(reference));
    assert!(message.contains("正确目录/唯一笔记"));

    assert!(conflict_error.to_string().contains("selector"));
}

#[test]
fn missing_reference_suggestion_preserves_heading_suffix_and_stops_at_distance_three() {
    let (dir, queries) = fixture();
    fs::create_dir(dir.path().join("人物")).expect("create note directory");
    fs::write(
        dir.path().join("人物/林动.md"),
        "# 林动\n\n## 身体\n\n内容\n",
    )
    .expect("write note");

    let error = queries
        .read_note("人物/林冻#身体", None, None)
        .expect_err("misspelled note should suggest the real reference");
    assert!(error.to_string().contains("人物/林动.md#身体"));

    let far_error = queries
        .read_note("完全不同的目标", None, None)
        .expect_err("far target should remain unresolved");
    assert!(!far_error.to_string().contains("人物/林动.md"));
}

#[test]
fn missing_path_prefers_same_directory_stem_prefix_suggestions_without_resolving() {
    let (dir, queries) = fixture();
    let chapter_dir = dir.path().join("正文/vol01-卷一/章节");
    fs::create_dir_all(&chapter_dir).expect("create chapter directory");
    fs::write(chapter_dir.join("ch005-归途与北声点火.md"), "# ch005\n")
        .expect("write prefixed chapter");
    fs::write(chapter_dir.join("ch006.md"), "# ch006\n").expect("write near chapter");

    let error = queries
        .read_note("正文/vol01-卷一/章节/ch005.md", None, None)
        .expect_err("prefix candidate must not resolve the request");
    assert!(
        error
            .to_string()
            .contains("正文/vol01-卷一/章节/ch005-归途与北声点火.md")
    );
    assert!(!error.to_string().contains("ch006.md"));
}

#[test]
fn missing_path_lists_same_directory_stem_prefix_suggestions_only() {
    let (dir, queries) = fixture();
    let chapter_dir = dir.path().join("正文/vol01-卷一/章节");
    fs::create_dir_all(&chapter_dir).expect("create chapter directory");
    for path in [
        "正文/vol01-卷一/章节/ch005-b.md",
        "正文/vol01-卷一/章节/ch005-a.md",
        "正文/vol01-卷一/章节/ch005c.md",
        "正文/vol01-卷一/章节/ch005-note.txt",
        "正文/vol01-卷一/other/ch005-c.md",
    ] {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
        fs::write(path, "# candidate\n").expect("write candidate");
    }

    let message = queries
        .read_note("正文/vol01-卷一/章节/ch005.md", None, None)
        .expect_err("prefix candidates must not resolve the request")
        .to_string();

    let first = message
        .find("正文/vol01-卷一/章节/ch005-a.md")
        .expect("first prefix candidate");
    let second = message
        .find("正文/vol01-卷一/章节/ch005-b.md")
        .expect("second prefix candidate");
    assert!(
        first < second,
        "prefix candidates should be naturally sorted"
    );
    assert!(!message.contains("ch005c.md"));
    assert!(!message.contains("正文/vol01-卷一/other/ch005-c.md"));
    assert!(!message.contains("ch005-note.txt"));
}

#[test]
fn audit_links_returns_unresolved_and_ambiguous_pages_together() {
    let (dir, queries) = fixture();
    fs::write(
        dir.path().join("审计.md"),
        "# 审计\n\n[[缺失目标]]\n\n[[发动机]]\n",
    )
    .expect("write audit note");

    let result = queries.audit_links(1).expect("audit links");
    assert_eq!(result.unresolved.len(), 2);
    assert_eq!(result.ambiguous.len(), 0);
    assert!(
        result
            .unresolved
            .iter()
            .any(|link| link.target == "缺失目标" && link.source.starts_with("审计.md#L"))
    );
    assert_eq!(result.totals.unresolved, 2);
    assert_eq!(result.totals.ambiguous, 0);
    assert_eq!(result.pagination.page, 1);
    assert_eq!(result.pagination.total_pages, 1);
}

#[test]
fn audit_links_treats_missing_heading_and_block_selectors_as_unresolved() {
    let (dir, queries) = fixture();
    fs::write(
        dir.path().join("Target.md"),
        "# Target\n\n## Present\n\n^present\n",
    )
    .expect("write target note");
    fs::write(
        dir.path().join("selector-audit.md"),
        "# Selector Audit\n\n[[Target#Missing Heading]]\n\n[[Target#^missing-block]]\n\n[[Target#Present]]\n\n[[Target#^present]]\n",
    )
    .expect("write selector audit note");

    let result = queries.audit_links(1).expect("audit links");
    let source_targets = result
        .unresolved
        .iter()
        .filter(|link| link.source.starts_with("selector-audit.md#L"))
        .map(|link| link.target.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        source_targets,
        vec!["Target#Missing Heading", "Target#^missing-block"]
    );
}

#[test]
fn audit_links_resolves_relative_markdown_links_to_filtered_notes() {
    let (dir, queries) = fixture();
    fs::write(dir.path().join("filtered-target.md"), "# Target\n").expect("write target");
    fs::write(
        dir.path().join("relative-audit.md"),
        "[relative](filtered-target.md)\n\n[[filtered-target]]\n",
    )
    .expect("write audit note");
    queries
        .vault
        .modify_config(|config| config.exclude = vec!["filtered-target.md".to_string()]);

    let result = queries.audit_links(1).expect("audit links");
    assert_eq!(
        result
            .unresolved
            .iter()
            .filter(|link| link.source.starts_with("relative-audit.md#L"))
            .map(|link| link.target.as_str())
            .collect::<Vec<_>>(),
        vec!["filtered-target"]
    );
}

#[test]
fn audit_links_prefers_root_note_for_bare_wikilinks() {
    let (dir, queries) = fixture();
    fs::create_dir_all(dir.path().join("设定集/世界观")).expect("create nested directory");
    fs::write(dir.path().join("README.md"), "# Root README\n").expect("write root README");
    fs::write(dir.path().join("设定集/README.md"), "# Settings README\n")
        .expect("write settings README");
    fs::write(
        dir.path().join("设定集/世界观/README.md"),
        "# World README\n",
    )
    .expect("write world README");
    fs::write(dir.path().join("设定集/世界观/source.md"), "[[README]]\n").expect("write source");

    let result = queries.audit_links(1).expect("audit links");
    assert!(
        !result
            .ambiguous
            .iter()
            .any(|link| link.source == "设定集/世界观/source.md#L1")
    );
}

#[test]
fn audit_links_resolves_obsidian_relative_suffix_and_local_heading_links() {
    let (dir, queries) = fixture();
    fs::create_dir_all(dir.path().join("资料库/调查报告/社会研究")).expect("create target dir");
    fs::write(
        dir.path().join("资料库/调查报告/社会研究/结果.md"),
        "# 结果\n\n## 2026年：开局\n",
    )
    .expect("write target");
    fs::create_dir_all(dir.path().join("大纲/第一卷")).expect("create source dir");
    fs::write(
        dir.path().join("大纲/第一卷/source.md"),
        "[[调查报告/社会研究/结果]]\n[[../../资料库/调查报告/社会研究/结果]]\n[[调查报告/社会研究/结果#2026年：开局]]\n## Local\n[[#Local]]\n",
    )
    .expect("write source");

    let result = queries.audit_links(1).expect("audit links");
    assert!(
        !result
            .unresolved
            .iter()
            .any(|link| link.source.starts_with("大纲/第一卷/source.md#L"))
    );
}

#[test]
fn note_neighborhood_traverses_only_resolved_links_and_omits_center() {
    let (dir, queries) = fixture();
    fs::write(
        dir.path().join("邻居.md"),
        "# 邻居\n\n[[林动]]\n\n[[缺失目标]]\n",
    )
    .expect("write neighbor");

    let result = queries
        .get_note_neighborhood("林动", 1, crate::query::NeighborhoodDirection::Both)
        .expect("neighborhood");

    assert_eq!(result.center.path, "林动.md");
    assert!(!result.notes.iter().any(|note| note.path == "林动.md"));
    assert!(
        result
            .notes
            .iter()
            .any(|note| note.path == "邻居.md" && note.distance == 1)
    );
    assert!(
        result
            .links
            .iter()
            .any(|link| link.from == "邻居.md" && link.to == "林动.md")
    );
    assert!(!result.links.iter().any(|link| link.to == "缺失目标"));
}

#[test]
fn note_neighborhood_excludes_edges_with_unresolved_selectors() {
    let (dir, queries) = fixture();
    fs::write(dir.path().join("Target.md"), "# Target\n\n## Present\n").expect("write target");
    fs::write(
        dir.path().join("Selector Source.md"),
        "# Selector Source\n\n[[Target#Missing Heading]]\n",
    )
    .expect("write selector source");

    let result = queries
        .get_note_neighborhood("Target", 1, crate::query::NeighborhoodDirection::In)
        .expect("neighborhood");

    assert!(
        !result
            .notes
            .iter()
            .any(|note| note.path == "Selector Source.md")
    );
    assert!(
        !result
            .links
            .iter()
            .any(|link| { link.from == "Selector Source.md" && link.to == "Target.md" })
    );
}

#[test]
fn empty_vault_queries_return_empty_results() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path");
    let vault = Vault::open(&root, VaultConfig::default()).expect("vault");
    let queries = VaultQueries::new(Arc::new(vault));

    let notes = queries.list_notes(&[], &[], 1, None).expect("list notes");
    assert!(notes.notes.is_empty());

    let categories = queries
        .list_categories(&[], &[], 1)
        .expect("list categories");
    assert!(categories.categories.is_empty());
    assert_eq!(categories.pagination.total_categories, 0);
}

#[test]
fn search_source_serializes_as_a_public_contract() {
    let source = SearchSource {
        path: "note.md".to_string(),
        line_start: 3,
        line_end: 5,
        section: Some(SectionInfo {
            heading: "原理".to_string(),
            heading_level: 2,
            heading_path: vec!["发动机".to_string(), "原理".to_string()],
            heading_anchor: "原理".to_string(),
        }),
    };
    let source_json = serde_json::to_value(source).expect("source json");
    assert_eq!(source_json["path"], "note.md#L3-L5");
    assert!(source_json.get("line_start").is_none());
}
