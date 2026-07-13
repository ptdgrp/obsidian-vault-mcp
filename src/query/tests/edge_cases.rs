use std::fs;

use camino::Utf8PathBuf;
use tempfile::tempdir;

use super::fixture;
use crate::parser::SectionInfo;
use crate::query::path_filter::PathFilter;
use crate::query::{ResolveSummary, SearchSource, SectionSelector, VaultQueries};
use crate::resolver::{ResolveCandidate, ResolveResult};
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

    match result {
        ResolveResult::Ambiguous { candidates, .. } => {
            assert_eq!(candidates.len(), 2);
            assert!(
                candidates
                    .iter()
                    .any(|candidate| candidate.path == "发动机.md")
            );
            assert!(
                candidates
                    .iter()
                    .any(|candidate| candidate.path == "资料/发动机.md")
            );
        }
        other => panic!("expected ambiguous, got {other:?}"),
    }
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
fn graph_health_queries_truncate_when_result_budget_is_small() {
    let (dir, mut queries) = fixture();
    fs::write(
        dir.path().join("额外1.md"),
        "# 额外1\n\n[[缺失]]\n[[发动机]]\n",
    )
    .expect("write extra1");
    fs::write(
        dir.path().join("额外2.md"),
        "# 额外2\n\n[[缺失]]\n[[发动机]]\n",
    )
    .expect("write extra2");
    queries.vault.config.max_results = 1;

    let unresolved = queries.find_unresolved_links().expect("find unresolved");
    assert_eq!(unresolved.links.len(), 1);
    assert!(unresolved.truncated);

    let ambiguous = queries.find_ambiguous_links().expect("find ambiguous");
    assert_eq!(ambiguous.links.len(), 1);
    assert!(ambiguous.truncated);
}

#[test]
fn empty_vault_queries_return_empty_results() {
    let dir = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path");
    let vault = Vault::open(root, VaultConfig::default()).expect("vault");
    let queries = VaultQueries::new(vault);

    let notes = queries.list_notes().expect("list notes");
    assert!(notes.notes.is_empty());

    let categories = queries.list_categories(&[], &[]).expect("list categories");
    assert!(categories.categories.is_empty());
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

#[test]
fn resolve_summary_preserves_resolved_ambiguous_and_unresolved_shapes() {
    let resolved: ResolveSummary = ResolveResult::Resolved {
        reference: crate::resolver::ObsidianRef {
            raw: "[[林动]]".to_string(),
            target: "林动".to_string(),
            reference: None,
        },
        path: "林动.md".to_string(),
        heading: Some("身体".to_string()),
        block_id: None,
    }
    .into();
    assert!(matches!(
        resolved,
        ResolveSummary::Resolved { path, heading, .. }
            if path == "林动.md" && heading.as_deref() == Some("身体")
    ));

    let ambiguous: ResolveSummary = ResolveResult::Ambiguous {
        reference: crate::resolver::ObsidianRef {
            raw: "[[发动机]]".to_string(),
            target: "发动机".to_string(),
            reference: None,
        },
        candidates: vec![ResolveCandidate {
            path: "发动机.md".to_string(),
            match_kind: "stem".to_string(),
        }],
    }
    .into();
    assert!(matches!(ambiguous, ResolveSummary::Ambiguous { candidates } if candidates.len() == 1));

    let unresolved: ResolveSummary = ResolveResult::Unresolved {
        reference: crate::resolver::ObsidianRef {
            raw: "[[缺失]]".to_string(),
            target: "缺失".to_string(),
            reference: None,
        },
    }
    .into();
    assert!(matches!(unresolved, ResolveSummary::Unresolved));
}
