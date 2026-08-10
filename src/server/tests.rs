use std::fs;

use camino::Utf8PathBuf;
use rmcp::handler::server::wrapper::{Json, Parameters};
use tempfile::tempdir;

use super::{
    BacklinksRequest, GetCategoryRequest, GetTagRequest, NoteOutlineRequest, NoteStructureRequest,
    ObsidianVaultMcp, OutlinksRequest, ReadNoteRequest, RenameHeadingRequest, RenameNoteRequest,
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
        "---\naliases:\n  - 动林\n---\n# 林动\n\n## 身体\n",
    )
    .expect("write note");
    fs::write(
        dir.path().join("发动机.md"),
        "# 发动机\n\n## 原理\n\n链接到 [[林动#身体]]\n",
    )
    .expect("write note");
    fs::write(dir.path().join("引用.md"), "# 引用\n\n[[发动机.md#原理]]\n").expect("write note");

    let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path");
    let vault = Vault::open(&root, VaultConfig::default()).expect("vault");
    (dir, ObsidianVaultMcp::new(std::sync::Arc::new(vault)))
}

#[test]
fn public_tool_definitions_preserve_names_and_object_schemas() {
    let tools = ObsidianVaultMcp::tool_definitions();
    let names = tools
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect::<Vec<&str>>();
    assert_eq!(
        names,
        vec![
            "append_section",
            "audit_links",
            "delete_section",
            "get_backlinks",
            "get_category",
            "get_note_neighborhood",
            "get_note_outline",
            "get_note_stats",
            "get_note_structure",
            "get_outlinks",
            "get_tag",
            "list_categories",
            "list_notes",
            "list_tags",
            "query_frontmatter",
            "read_note",
            "rename_heading",
            "rename_note",
            "replace_section",
            "resolve_ref",
            "search_regex",
            "search_text",
            "set_block_id",
        ]
    );
    for tool in tools {
        assert_eq!(
            tool.input_schema
                .get("type")
                .and_then(|value| value.as_str()),
            Some("object")
        );
        if let Some(output_schema) = &tool.output_schema {
            assert_eq!(
                output_schema.get("type").and_then(|value| value.as_str()),
                Some("object")
            );
        }
    }
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
fn mcp_tool_schemas_do_not_use_uint_format() {
    for tool in ObsidianVaultMcp::tool_definitions() {
        assert_schema_has_no_uint_format(
            &serde_json::Value::Object((*tool.input_schema).clone()),
            &tool.name,
        );
        if let Some(output_schema) = &tool.output_schema {
            assert_schema_has_no_uint_format(
                &serde_json::Value::Object((**output_schema).clone()),
                &tool.name,
            );
        }
    }
}

#[test]
fn read_note_output_schema_allows_omitted_truncated() {
    let read_note = ObsidianVaultMcp::tool_definitions()
        .into_iter()
        .find(|tool| tool.name == "read_note")
        .expect("read_note tool");
    let required = read_note
        .output_schema
        .as_ref()
        .expect("read_note output schema")
        .get("required")
        .and_then(serde_json::Value::as_array)
        .expect("read_note output required fields");

    assert!(
        !required.iter().any(|field| field == "truncated"),
        "read_note omits truncated when false, so the output schema must not require it"
    );
}

fn assert_schema_has_no_uint_format(schema: &serde_json::Value, tool_name: &str) {
    match schema {
        serde_json::Value::Array(values) => {
            for value in values {
                assert_schema_has_no_uint_format(value, tool_name);
            }
        }
        serde_json::Value::Object(values) => {
            assert_ne!(
                values.get("format").and_then(serde_json::Value::as_str),
                Some("uint"),
                "{tool_name} schema contains the unsupported uint format"
            );
            for value in values.values() {
                assert_schema_has_no_uint_format(value, tool_name);
            }
        }
        _ => {}
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
    assert_eq!(result.source, "发动机.md#L3-L5");
    assert!(result.truncated);
}

#[test]
fn read_note_explicit_line_selector_accepts_optional_second_l_marker() {
    let (_dir, server) = fixture();

    let Json(with_marker) = server
        .read_note(Parameters(ReadNoteRequest {
            note: "发动机".to_string(),
            max_chars: None,
            heading: None,
            block_id: None,
            line: Some("L3-L5".to_string()),
        }))
        .expect("read line range with second L marker");
    let Json(without_marker) = server
        .read_note(Parameters(ReadNoteRequest {
            note: "发动机".to_string(),
            max_chars: None,
            heading: None,
            block_id: None,
            line: Some("L3-5".to_string()),
        }))
        .expect("read line range without second L marker");

    assert_eq!(without_marker.source, with_marker.source);
    assert_eq!(without_marker.content, with_marker.content);
}

#[test]
fn note_structure_tool_returns_compact_contract_without_parser_links() {
    let (_dir, server) = fixture();

    let Json(result) = server
        .get_note_structure(Parameters(NoteStructureRequest {
            note: "发动机.md".to_string(),
        }))
        .expect("get note structure");
    let value = serde_json::to_value(result).expect("structure json");

    assert_eq!(value["note"], "发动机.md");
    assert_eq!(value["link_count"], 1);
    assert_eq!(
        value["headings"],
        serde_json::json!([{"heading": "原理", "line": 3}])
    );
    assert!(value.get("links").is_none());
    assert!(value.get("path").is_none());
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
            "ambiguous_targets": [],
            "unresolved_targets": [],
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
fn note_stats_tool_returns_local_counts() {
    let (_dir, server) = fixture();

    let Json(result) = server
        .get_note_stats(Parameters(NoteStatsRequest {
            note: "林动.md".to_string(),
        }))
        .expect("get note stats");

    assert_eq!(result.scope, "林动.md");
    assert_eq!(result.word_count, 7);
    assert_eq!(result.line_count, 7);
    assert!(result.character_count > result.word_count);
}

#[test]
fn note_stats_tool_returns_normalized_scope_for_a_scoped_reference() {
    let (_dir, server) = fixture();

    let Json(result) = server
        .get_note_stats(Parameters(NoteStatsRequest {
            note: "发动机#原理".to_string(),
        }))
        .expect("scoped note stats");

    let value = serde_json::to_value(result).expect("stats json");
    assert_eq!(value["scope"], "发动机.md#原理");
    assert!(value.get("source").is_none());
    assert!(value.get("word_count_mode").is_none());
}

#[test]
fn note_stats_schema_exposes_only_local_count_fields() {
    let tool = ObsidianVaultMcp::tool_definitions()
        .into_iter()
        .find(|tool| tool.name == "get_note_stats")
        .expect("get_note_stats tool");
    let properties = tool.input_schema["properties"]
        .as_object()
        .expect("note stats input properties");

    assert!(properties.contains_key("note"));
    assert!(!properties.contains_key("word_count_mode"));

    let output_properties = tool
        .output_schema
        .as_ref()
        .expect("note stats output schema")["properties"]
        .as_object()
        .expect("note stats output properties");
    assert!(!output_properties.contains_key("backlink_count"));
}

#[test]
fn note_outline_schema_uses_page_not_heading_selector() {
    let tool = ObsidianVaultMcp::tool_definitions()
        .into_iter()
        .find(|tool| tool.name == "get_note_outline")
        .expect("get_note_outline tool");
    let properties = tool.input_schema["properties"]
        .as_object()
        .expect("note outline input properties");

    assert!(properties.contains_key("page"));
    assert!(!properties.contains_key("heading"));

    let (_dir, server) = fixture();
    let Json(result) = server
        .get_note_outline(Parameters(NoteOutlineRequest {
            note: "发动机.md".to_string(),
            page: 1,
        }))
        .expect("get note outline");
    let value = serde_json::to_value(result).expect("outline json");
    assert_eq!(
        value["headings"],
        serde_json::json!([{"heading": "原理", "line": 3}])
    );
}

#[test]
fn get_tag_and_get_category_reject_empty_inputs() {
    let (_dir, server) = fixture();

    let tags_error = match server.get_tag(Parameters(GetTagRequest {
        tag: String::new(),
        scope: TagScope::Note,
        include: vec![],
        exclude: vec![],
        page: 1,
    })) {
        Ok(_) => panic!("empty tag should fail"),
        Err(error) => error,
    };
    assert!(tags_error.contains("provide a non-empty tag"));

    let categories_error = match server.get_category(Parameters(GetCategoryRequest {
        category: String::new(),
        include: vec![],
        exclude: vec![],
        page: 1,
    })) {
        Ok(_) => panic!("empty category should fail"),
        Err(error) => error,
    };
    assert!(categories_error.contains("provide a non-empty category"));
}

#[test]
fn rename_note_defaults_to_dry_run_and_reports_changed_notes() {
    let (_dir, server) = fixture();

    let Json(result) = server
        .rename_note(Parameters(RenameNoteRequest {
            path: "引用.md".to_string(),
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
