use std::fs;

use camino::Utf8PathBuf;
use rmcp::handler::server::wrapper::{Json, Parameters};
use tempfile::tempdir;

use super::{
    BacklinksRequest, CategoriesRequest, ObsidianVaultMcp, OutlinksRequest, ReadNoteRequest,
    RenameHeadingRequest, RenameNoteRequest, TagsRequest,
};
use crate::{
    query::TagScope,
    server::{NoteStatsRequest, ResolveRefRequest},
    vault::{Vault, VaultConfig},
};

const FORBIDDEN_TOP_LEVEL_SCHEMA_KEYS: [&str; 6] =
    ["oneOf", "anyOf", "allOf", "enum", "const", "not"];

mod dispatch_more;

pub(super) fn fixture() -> (tempfile::TempDir, ObsidianVaultMcp) {
    let dir = tempdir().expect("tempdir");
    fs::write(
        dir.path().join("林动.md"),
        "---\naliases:\n  - 动林\n---\n# 林动\n\n身体\n",
    )
    .expect("write note");
    fs::write(
        dir.path().join("发动机.md"),
        "# 发动机\n\n## 原理\n\n链接到 [[林动#身体]]\n",
    )
    .expect("write note");
    fs::write(dir.path().join("引用.md"), "# 引用\n\n[[发动机.md#原理]]\n").expect("write note");

    let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path");
    let vault = Vault::open(root, VaultConfig::default()).expect("vault");
    (dir, ObsidianVaultMcp::new(vault))
}

#[test]
fn all_tool_input_schemas_have_plain_object_roots() {
    for tool in ObsidianVaultMcp::tool_definitions() {
        assert_eq!(
            tool.input_schema
                .get("type")
                .and_then(|value| value.as_str()),
            Some("object"),
            "{} input schema must have an object root",
            tool.name
        );

        for key in FORBIDDEN_TOP_LEVEL_SCHEMA_KEYS {
            assert!(
                !tool.input_schema.contains_key(key),
                "{} input schema must not have top-level {key}",
                tool.name
            );
        }
    }
}

#[test]
fn resolve_ref_tool_returns_resolved_heading_result() {
    let (_dir, server) = fixture();

    let Json(result) = server
        .resolve_ref(Parameters(ResolveRefRequest {
            reference: "[[动林#身体]]".to_string(),
        }))
        .expect("resolve ref");
    let value = serde_json::to_value(result).expect("resolve ref json");

    assert_eq!(value, serde_json::json!({"target": "林动.md#身体"}));
}

#[test]
fn read_note_surfaces_missing_note_error() {
    let (_dir, server) = fixture();

    let result = server.read_note(Parameters(ReadNoteRequest {
        note: "缺失.md".to_string(),
        max_chars: None,
        heading: None,
        block_id: None,
        line: None,
    }));
    let error = match result {
        Ok(_) => panic!("missing note should fail"),
        Err(error) => error,
    };

    assert!(error.contains("unresolved note reference"));
}

#[test]
fn read_note_schema_exposes_max_chars_not_max_bytes() {
    let tool = ObsidianVaultMcp::tool_definitions()
        .into_iter()
        .find(|tool| tool.name == "read_note")
        .expect("read_note tool");
    let properties = tool
        .input_schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .expect("read_note input properties");

    assert!(properties.contains_key("max_chars"));
    assert!(!properties.contains_key("max_bytes"));
}

#[test]
fn read_note_schema_exposes_selectors_and_read_section_is_absent() {
    let definitions = ObsidianVaultMcp::tool_definitions();
    let tool = definitions
        .iter()
        .find(|tool| tool.name == "read_note")
        .expect("read_note tool");
    let properties = tool.input_schema["properties"]
        .as_object()
        .expect("read_note input properties");
    for selector in ["heading", "block_id", "line", "max_chars"] {
        assert!(properties.contains_key(selector), "missing {selector}");
    }
    assert!(!definitions.iter().any(|tool| tool.name == "read_section"));

    let (_dir, server) = fixture();
    let Json(result) = server
        .read_note(Parameters(ReadNoteRequest {
            note: "发动机".to_string(),
            max_chars: Some(4),
            heading: Some("原理".to_string()),
            block_id: None,
            line: None,
        }))
        .expect("read heading through MCP");
    assert_eq!(result.source.line_start, 3);
    assert!(result.truncated);
}

#[test]
fn rename_heading_surfaces_user_visible_mutation_failure() {
    let (_dir, server) = fixture();

    let result = server.rename_heading(Parameters(RenameHeadingRequest {
        note: "发动机.md".to_string(),
        old_heading: "不存在".to_string(),
        new_heading: "机制".to_string(),
        dry_run: false,
    }));
    let error = match result {
        Ok(_) => panic!("missing heading should fail"),
        Err(error) => error,
    };

    assert!(error.contains("heading not found: 不存在"));
}

#[test]
fn outlinks_tool_returns_compact_links_for_existing_note() {
    let (_dir, server) = fixture();

    let Json(result) = server
        .get_outlinks(Parameters(OutlinksRequest {
            note: "发动机.md".to_string(),
            page: 1,
        }))
        .expect("get outlinks");
    let value = serde_json::to_value(result).expect("outlinks json");

    assert_eq!(
        value,
        serde_json::json!({
            "note": "发动机.md",
            "targets": [{
                "source": "发动机.md#L5",
                "target": "林动.md#身体"
            }],
            "pagination": {
                "page": 1,
                "total_pages": 1,
                "total_links": 1
            }
        })
    );
}

#[test]
fn backlinks_tool_returns_compact_results_for_reference() {
    let (_dir, server) = fixture();

    let Json(result) = server
        .get_backlinks(Parameters(BacklinksRequest {
            target: "[[林动#身体]]".to_string(),
            include: vec![],
            exclude: vec![],
            page: 1,
        }))
        .expect("get backlinks");
    let value = serde_json::to_value(result).expect("backlinks json");

    assert_eq!(
        value,
        serde_json::json!({
            "scope": "林动.md#身体",
            "references": [{
                "target": "林动.md#身体",
                "sources": ["发动机.md#L5"]
            }],
            "pagination": {
                "page": 1,
                "total_pages": 1,
                "total_backlinks": 1
            }
        })
    );
}

#[test]
fn note_stats_tool_returns_word_character_and_backlink_counts() {
    let (_dir, server) = fixture();

    let Json(result) = server
        .get_note_stats(Parameters(NoteStatsRequest {
            note: "林动.md".to_string(),
            word_count_mode: Default::default(),
        }))
        .expect("get note stats");

    assert_eq!(result.note, "林动.md");
    assert_eq!(result.word_count, 7);
    assert_eq!(result.line_count, 7);
    assert_eq!(result.backlink_count, 1);
    assert!(result.character_count > result.word_count);
}

#[test]
fn note_stats_tool_returns_source_for_a_scoped_reference() {
    let (_dir, server) = fixture();

    let Json(result) = server
        .get_note_stats(Parameters(NoteStatsRequest {
            note: "发动机#原理".to_string(),
            word_count_mode: Default::default(),
        }))
        .expect("scoped note stats");

    assert_eq!(result.source.expect("source").line_start, 3);
}

#[test]
fn get_tags_and_get_categories_reject_empty_inputs() {
    let (_dir, server) = fixture();

    let tags_error = match server.get_tags(Parameters(TagsRequest {
        tags: Vec::new(),
        scope: TagScope::Note,
        verbose: false,
        include: vec![],
        exclude: vec![],
    })) {
        Ok(_) => panic!("empty tags should fail"),
        Err(error) => error,
    };
    assert!(tags_error.contains("provide at least one tag"));

    let categories_error = match server.get_categories(Parameters(CategoriesRequest {
        categories: Vec::new(),
        include: vec![],
        exclude: vec![],
    })) {
        Ok(_) => panic!("empty categories should fail"),
        Err(error) => error,
    };
    assert!(categories_error.contains("provide at least one category"));
}

#[test]
fn rename_note_defaults_to_dry_run_and_reports_changed_notes() {
    let (_dir, server) = fixture();

    let Json(result) = server
        .rename_note(Parameters(RenameNoteRequest {
            note: "引用.md".to_string(),
            new_path: "archive/引用.md".to_string(),
            dry_run: true,
        }))
        .expect("rename note preview");

    assert!(result.dry_run);
    assert!(
        result
            .changed_notes
            .contains(&"archive/引用.md".to_string())
    );
}
