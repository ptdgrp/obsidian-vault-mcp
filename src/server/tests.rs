use std::fs;

use camino::Utf8PathBuf;
use rmcp::handler::server::wrapper::{Json, Parameters};
use tempfile::tempdir;

use super::{
    BacklinksRequest, CategoriesRequest, ObsidianVaultMcp, OutlinksRequest, ReadNoteRequest,
    ReadSectionRequest, RenameHeadingRequest, RenameNoteRequest, TagsRequest,
};
use crate::{
    query::{SectionSelector, TagScope},
    resolver::ResolveResult,
    server::ResolveRefRequest,
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
fn read_section_request_schema_has_plain_object_root() {
    let schema = serde_json::to_value(schemars::schema_for!(ReadSectionRequest)).expect("schema");

    assert_eq!(schema["type"], "object");
    for key in FORBIDDEN_TOP_LEVEL_SCHEMA_KEYS {
        assert!(schema.get(key).is_none());
    }
}

#[test]
fn read_section_request_accepts_obsidian_line_reference() {
    let (_, selector) = ReadSectionRequest {
        note: "note.md".to_string(),
        heading: None,
        block_id: None,
        line: Some("#L3-L5".to_string()),
    }
    .into_parts()
    .expect("line selector");

    assert!(matches!(
        selector,
        SectionSelector::Lines {
            line_start: 3,
            line_end: 5
        }
    ));
}

#[test]
fn resolve_ref_tool_returns_resolved_heading_result() {
    let (_dir, server) = fixture();

    let Json(result) = server
        .resolve_ref(Parameters(ResolveRefRequest {
            reference: "[[动林#身体]]".to_string(),
        }))
        .expect("resolve ref");

    assert!(matches!(
        result.result,
        ResolveResult::Resolved {
            path,
            heading,
            block_id: None,
            ..
        } if path == "林动.md" && heading.as_deref() == Some("身体")
    ));
}

#[test]
fn read_section_rejects_malformed_line_selector() {
    let (_dir, server) = fixture();

    let result = server.read_section(Parameters(ReadSectionRequest {
        note: "发动机.md".to_string(),
        heading: None,
        block_id: None,
        line: Some("#L0-L2".to_string()),
    }));
    let error = match result {
        Ok(_) => panic!("invalid line range should fail"),
        Err(error) => error,
    };

    assert_eq!(error, "invalid line range");
}

#[test]
fn read_note_surfaces_missing_note_error() {
    let (_dir, server) = fixture();

    let result = server.read_note(Parameters(ReadNoteRequest {
        note: "缺失.md".to_string(),
    }));
    let error = match result {
        Ok(_) => panic!("missing note should fail"),
        Err(error) => error,
    };

    assert!(error.contains("unresolved note reference"));
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
            verbose: false,
        }))
        .expect("get outlinks");

    assert_eq!(result.note, "发动机.md");
    assert_eq!(result.links.len(), 1);
    assert_eq!(result.links[0].target, "林动");
}

#[test]
fn backlinks_tool_returns_verbose_results_for_reference() {
    let (_dir, server) = fixture();

    let Json(result) = server
        .get_backlinks(Parameters(BacklinksRequest {
            target: "[[林动#身体]]".to_string(),
            verbose: true,
        }))
        .expect("get backlinks");

    assert_eq!(result.target, "[[林动#身体]]");
    assert_eq!(result.backlinks.len(), 1);
    assert!(result.backlinks[0].snippet.is_some());
}

#[test]
fn get_tags_and_get_categories_reject_empty_inputs() {
    let (_dir, server) = fixture();

    let tags_error = match server.get_tags(Parameters(TagsRequest {
        tags: Vec::new(),
        scope: TagScope::Note,
        verbose: false,
    })) {
        Ok(_) => panic!("empty tags should fail"),
        Err(error) => error,
    };
    assert!(tags_error.contains("provide at least one tag"));

    let categories_error = match server.get_categories(Parameters(CategoriesRequest {
        categories: Vec::new(),
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
