use std::{collections::BTreeSet, fs};

use rmcp::handler::server::wrapper::{Json, Parameters};

use super::fixture;
use crate::query::{FrontmatterMatchMode, TagScope};
use crate::server::{
    AppendSectionRequest, AuditLinksRequest, GetTagRequest, ListNotesRequest, NeighborhoodRequest,
    NoteOutlineRequest, NoteStructureRequest, ObsidianVaultMcp, ReadNoteRequest,
    ReplaceSectionRequest, SearchRegexRequest, SearchTextRequest, SetBlockIdRequest,
};

const TASK_DEFINITION_LIST_NOTES: &str = "Page through visible Markdown notes for lightweight navigation. Set limit between 1 and 100 to keep responses compact; it defaults to 100.";
const TASK_DEFINITION_AUDIT_LINKS: &str =
    "Audit unresolved and ambiguous local links across the visible vault.";
const TASK_DEFINITION_GET_NOTE_NEIGHBORHOOD: &str =
    "Return a bounded resolved-link neighborhood around one note reference.";

fn tool_properties(name: &str) -> serde_json::Map<String, serde_json::Value> {
    ObsidianVaultMcp::tool_definitions()
        .into_iter()
        .find(|definition| definition.name == name)
        .unwrap_or_else(|| panic!("{name} definition"))
        .input_schema["properties"]
        .as_object()
        .unwrap_or_else(|| panic!("{name} input schema properties"))
        .clone()
}

#[test]
fn public_tool_set_matches_task8_contract_exactly() {
    let actual = ObsidianVaultMcp::tool_definitions()
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect::<BTreeSet<_>>();
    let expected = [
        "list_notes",
        "audit_links",
        "get_note_neighborhood",
        "get_note_structure",
        "get_note_outline",
        #[cfg(feature = "attachments")]
        "read_attachment",
        "read_note",
        "get_note_stats",
        "search_text",
        "search_regex",
        "resolve_ref",
        "get_outlinks",
        "get_backlinks",
        "list_tags",
        "get_tag",
        "list_categories",
        "get_category",
        "query_frontmatter",
        "append_section",
        "replace_section",
        "delete_section",
        "rename_note",
        "rename_heading",
        "set_block_id",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<BTreeSet<_>>();

    assert_eq!(actual, expected);
}

#[test]
fn public_tool_set_paged_collections_use_fixed_page_contract() {
    for name in [
        "list_notes",
        "audit_links",
        "get_note_outline",
        "search_text",
        "search_regex",
        "get_outlinks",
        "get_backlinks",
        "list_tags",
        "get_tag",
        "list_categories",
        "get_category",
        "query_frontmatter",
    ] {
        let properties = tool_properties(name);
        assert!(properties.contains_key("page"), "{name} should expose page");
        assert!(
            !properties.contains_key("page_size"),
            "{name} must not expose page_size"
        );
        assert!(
            !properties.contains_key("cursor"),
            "{name} must not expose cursor"
        );
    }
}

#[test]
fn public_tool_set_filter_tools_expose_include_exclude_arrays() {
    for name in [
        "list_notes",
        "search_text",
        "search_regex",
        "get_backlinks",
        "list_tags",
        "get_tag",
        "list_categories",
        "get_category",
        "query_frontmatter",
    ] {
        let properties = tool_properties(name);
        assert_eq!(properties["include"]["type"], "array", "{name} include");
        assert_eq!(properties["exclude"]["type"], "array", "{name} exclude");
    }
}

#[test]
fn public_tool_set_removed_inputs_stay_absent_from_schemas() {
    for name in ["get_outlinks", "get_backlinks", "list_tags", "get_tag"] {
        let properties = tool_properties(name);
        assert!(!properties.contains_key("verbose"), "{name} verbose");
    }

    for name in ["search_text", "search_regex"] {
        let properties = tool_properties(name);
        assert!(
            !properties.contains_key("context_lines"),
            "{name} context_lines"
        );
    }

    let outline = tool_properties("get_note_outline");
    assert!(!outline.contains_key("heading"));

    for name in ["append_section", "replace_section", "delete_section"] {
        let properties = tool_properties(name);
        assert!(
            !properties.contains_key("line"),
            "{name} must only support heading and block_id selectors"
        );
    }
}

#[test]
fn public_tool_set_new_task_descriptions_match_canonical_sentences() {
    let definitions = ObsidianVaultMcp::tool_definitions();
    for (name, expected) in [
        ("list_notes", TASK_DEFINITION_LIST_NOTES),
        ("audit_links", TASK_DEFINITION_AUDIT_LINKS),
        (
            "get_note_neighborhood",
            TASK_DEFINITION_GET_NOTE_NEIGHBORHOOD,
        ),
    ] {
        let definition = definitions
            .iter()
            .find(|definition| definition.name == name)
            .unwrap_or_else(|| panic!("{name} definition"));
        assert_eq!(definition.description.as_deref(), Some(expected), "{name}");
    }
}

#[test]
fn tool_definitions_are_sorted_and_unknown_tools_stay_absent() {
    let (_dir, server) = fixture();
    let definitions = ObsidianVaultMcp::tool_definitions();
    let names = definitions
        .iter()
        .map(|tool| tool.name.to_string())
        .collect::<Vec<_>>();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
    assert!(!server.tool_router.has_route("missing_tool"));
    assert!(server.tool_router.get("missing_tool").is_none());
}

#[test]
fn tool_output_schemas_have_object_roots_when_present() {
    for tool in ObsidianVaultMcp::tool_definitions() {
        if let Some(schema) = tool.output_schema.as_ref() {
            assert_eq!(
                schema.get("type").and_then(|value| value.as_str()),
                Some("object"),
                "{} output schema must have an object root",
                tool.name
            );
            assert!(
                schema
                    .get("required")
                    .is_none_or(serde_json::Value::is_array),
                "{} output schema required properties must be an array",
                tool.name
            );
        }
    }
}

#[test]
fn query_tools_expose_request_path_filter_arrays_without_legacy_path_glob() {
    let definitions = ObsidianVaultMcp::tool_definitions();
    for name in [
        "list_tags",
        "get_tag",
        "list_categories",
        "get_category",
        "search_text",
        "search_regex",
    ] {
        let definition = definitions
            .iter()
            .find(|definition| definition.name == name)
            .expect("query tool definition");
        let properties = definition.input_schema["properties"]
            .as_object()
            .expect("input schema properties");
        assert_eq!(properties["include"]["type"], "array", "{name} include");
        assert_eq!(properties["exclude"]["type"], "array", "{name} exclude");
    }

    let regex = definitions
        .iter()
        .find(|definition| definition.name == "search_regex")
        .expect("regex definition");
    assert!(regex.input_schema["properties"].get("path_glob").is_none());
}

#[test]
fn backlinks_tool_exposes_source_path_filter_arrays() {
    let definition = ObsidianVaultMcp::tool_definitions()
        .into_iter()
        .find(|definition| definition.name == "get_backlinks")
        .expect("backlinks definition");
    let properties = definition.input_schema["properties"]
        .as_object()
        .expect("input schema properties");
    assert_eq!(properties["include"]["type"], "array");
    assert_eq!(properties["exclude"]["type"], "array");
    assert!(properties.contains_key("page"));
    assert!(!properties.contains_key("verbose"));

    let outlinks = ObsidianVaultMcp::tool_definitions()
        .into_iter()
        .find(|definition| definition.name == "get_outlinks")
        .expect("outlinks definition");
    let outlink_properties = outlinks.input_schema["properties"]
        .as_object()
        .expect("outlinks input schema properties");
    assert!(outlink_properties.contains_key("page"));
    assert!(!outlink_properties.contains_key("verbose"));
}

#[test]
fn query_tool_path_filters_reach_tag_and_search_handlers() {
    let (dir, server) = fixture();
    for path in ["正文/keep.md", "资料/keep.md", "正文/草稿/drop.md"] {
        if let Some(parent) = dir.path().join(path).parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(
            dir.path().join(path),
            "# Filtered\n\n#筛选标签\n\nshared filtered content\n",
        )
        .expect("write filtered note");
    }
    let include = vec!["正文/**/*.md".to_string(), "资料/**/*.md".to_string()];
    let exclude = vec!["**/草稿/**".to_string()];

    let Json(tags) = server
        .get_tag(Parameters(GetTagRequest {
            tag: "筛选标签".to_string(),
            scope: TagScope::Note,
            include: include.clone(),
            exclude: exclude.clone(),
            page: 1,
        }))
        .expect("filtered tags");
    assert_eq!(tags.matches.len(), 2);
    assert!(tags.matches.iter().all(|path| path != "正文/草稿/drop.md"));

    let Json(matches) = server
        .search_text(Parameters(SearchTextRequest {
            query: "shared filtered content".to_string(),
            case_sensitive: false,
            include,
            exclude,
            page: 1,
        }))
        .expect("filtered text search");
    assert_eq!(matches.matches.len(), 2);
    assert!(
        matches
            .matches
            .iter()
            .all(|matched| !matched.source.starts_with("正文/草稿/drop.md"))
    );
}

#[test]
fn list_and_note_structure_tools_return_note_metadata() {
    let (_dir, server) = fixture();
    assert!(server.tool_router.has_route("get_note_structure"));
    assert!(!server.tool_router.has_route("parse_note"));

    let Json(listed) = server
        .list_notes(Parameters(ListNotesRequest {
            include: vec![],
            exclude: vec![],
            page: 1,
            limit: None,
        }))
        .expect("list notes");
    assert_eq!(listed.notes.len(), 3);
    assert_eq!(listed.pagination.total_notes, 3);

    let Json(structure) = server
        .get_note_structure(Parameters(NoteStructureRequest {
            note: "发动机.md".to_string(),
        }))
        .expect("get note structure");
    assert_eq!(structure.note, "发动机.md");
    assert_eq!(structure.link_count, 1);
}

#[test]
fn task_oriented_link_tools_reach_query_layer() {
    let (_dir, server) = fixture();
    let Json(audit) = server
        .audit_links(Parameters(AuditLinksRequest { page: 1 }))
        .expect("audit");
    assert!(audit.unresolved.is_empty());
    assert!(audit.ambiguous.is_empty());

    let Json(neighborhood) = server
        .get_note_neighborhood(Parameters(NeighborhoodRequest {
            target: "林动".to_string(),
            depth: 1,
            direction: crate::query::NeighborhoodDirection::Both,
        }))
        .expect("neighborhood");
    assert_eq!(neighborhood.center.path, "林动.md");
}

#[test]
fn search_and_frontmatter_tools_surface_results_and_regex_errors() {
    let (_dir, server) = fixture();

    let Json(text_result) = server
        .search_text(Parameters(SearchTextRequest {
            query: "林动".to_string(),
            case_sensitive: false,
            include: vec![],
            exclude: vec![],
            page: 1,
        }))
        .expect("search text");
    assert_eq!(text_result.matches.len(), 2);

    let Json(frontmatter) = server
        .query_frontmatter(Parameters(crate::query::FrontmatterQueryOptions {
            field: "aliases".to_string(),
            mode: FrontmatterMatchMode::Exists,
            value: None,
            include: vec![],
            exclude: vec![],
            page: 1,
        }))
        .expect("query frontmatter");
    assert_eq!(frontmatter.notes.len(), 1);

    let regex_error = match server.search_regex(Parameters(SearchRegexRequest {
        pattern: "(".to_string(),
        case_sensitive: false,
        include: vec![],
        exclude: vec![],
        page: 1,
    })) {
        Ok(_) => panic!("invalid regex should fail"),
        Err(error) => error,
    };
    assert!(regex_error.contains("regex parse error"));
}

#[test]
fn replace_section_and_link_health_tools_work_through_server_surface() {
    let (dir, server) = fixture();
    fs::write(dir.path().join("额外.md"), "# 额外\n\n[[缺失]]\n").expect("write unresolved");

    let Json(replaced) = server
        .replace_section(Parameters(ReplaceSectionRequest {
            note: "发动机.md".to_string(),
            heading: Some("原理".to_string()),
            block_id: None,
            content: "## 原理\n\n已替换\n".to_string(),
        }))
        .expect("replace section");
    assert_eq!(
        serde_json::to_value(replaced).expect("replace section json"),
        serde_json::json!({"changed": "发动机.md#L3-L5"})
    );
    assert!(
        fs::read_to_string(dir.path().join("发动机.md"))
            .expect("read note")
            .contains("已替换")
    );

    let Json(audit) = server
        .audit_links(Parameters(AuditLinksRequest { page: 1 }))
        .expect("audit links");
    assert!(!audit.unresolved.is_empty());
    assert!(audit.ambiguous.is_empty());
}

#[test]
fn append_read_and_set_block_id_tools_work_through_server_surface() {
    let (dir, server) = fixture();
    fs::write(
        dir.path().join("块.md"),
        "# 块\n\n## 正文\n\n段落\n^state\n",
    )
    .expect("write block note");
    fs::write(dir.path().join("引用.md"), "# 引用\n\n[[块.md#^state]]\n").expect("write ref");

    let Json(appended) = server
        .append_section(Parameters(AppendSectionRequest {
            note: "块.md".to_string(),
            heading: Some("正文".to_string()),
            block_id: None,
            content: "\n补充说明\n".to_string(),
        }))
        .expect("append section");
    assert_eq!(
        serde_json::to_value(appended).expect("append section json"),
        serde_json::json!({"changed": "块.md#L7-L8"})
    );

    let Json(read) = server
        .read_note(Parameters(ReadNoteRequest {
            note: "块.md".to_string(),
            max_chars: None,
            heading: Some("正文".to_string()),
            block_id: None,
            line: None,
        }))
        .expect("read section");
    assert!(read.content.contains("补充说明"));

    let Json(renamed) = server
        .set_block_id(Parameters(SetBlockIdRequest {
            note: "块.md".to_string(),
            old_block_id: Some("state".to_string()),
            content: None,
            block_id: Some("status".to_string()),
            dry_run: false,
        }))
        .expect("rename block id");
    assert_eq!(renamed.updated_references, 1);
    assert!(
        fs::read_to_string(dir.path().join("引用.md"))
            .expect("read ref")
            .contains("#^status")
    );
}

#[test]
fn outline_tag_and_ambiguous_link_tools_surface_results() {
    let (dir, server) = fixture();
    fs::write(dir.path().join("标签.md"), "# 标签\n\n状态 #状态/身体\n").expect("write tagged");
    fs::create_dir_all(dir.path().join("资料")).expect("create category dir");
    fs::write(dir.path().join("资料/发动机.md"), "# 备用发动机\n").expect("write duplicate");
    fs::write(dir.path().join("歧义.md"), "# 歧义\n\n[[发动机]]\n").expect("write ambiguous ref");

    let Json(outline) = server
        .get_note_outline(Parameters(NoteOutlineRequest {
            note: "发动机.md".to_string(),
            page: 1,
        }))
        .expect("outline");
    assert_eq!(outline.headings[0].heading, "原理");

    let Json(tags) = server
        .get_tag(Parameters(GetTagRequest {
            tag: "状态/身体".to_string(),
            scope: TagScope::Note,
            include: vec![],
            exclude: vec![],
            page: 1,
        }))
        .expect("get tag");
    assert_eq!(tags.matches, vec!["标签.md".to_string()]);

    let Json(audit) = server
        .audit_links(Parameters(AuditLinksRequest { page: 1 }))
        .expect("audit links");
    assert_eq!(audit.ambiguous.len(), 0);
}
