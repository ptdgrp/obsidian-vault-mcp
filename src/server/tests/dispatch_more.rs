use std::fs;

use rmcp::handler::server::wrapper::{Json, Parameters};

use super::fixture;
use crate::query::{
    FrontmatterMatchMode, GraphNeighborhoodDirection, GraphNeighborhoodOptions, TagScope,
};
use crate::server::{
    AppendSectionRequest, ContextNoteRequest, ContextReferenceRequest, EmptyRequest,
    NoteOutlineRequest, NoteStructureRequest, ObsidianVaultMcp, ReadSectionRequest,
    RenameBlockIdRequest, ReplaceSectionRequest, SearchRegexRequest, SearchTextRequest,
    TagsRequest, VaultFilesRequest,
};

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
        }
    }
}

#[test]
fn query_tools_expose_request_path_filter_arrays_without_legacy_path_glob() {
    let definitions = ObsidianVaultMcp::tool_definitions();
    for name in [
        "list_tags",
        "get_tags",
        "list_categories",
        "get_categories",
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
        .get_tags(Parameters(TagsRequest {
            tags: vec!["筛选标签".to_string()],
            scope: TagScope::Note,
            verbose: false,
            include: include.clone(),
            exclude: exclude.clone(),
        }))
        .expect("filtered tags");
    assert_eq!(tags.tags[0].notes.len(), 2);
    assert!(
        tags.tags[0]
            .notes
            .iter()
            .all(|tag| tag.note != "正文/草稿/drop.md")
    );

    let Json(matches) = server
        .search_text(Parameters(SearchTextRequest {
            query: "shared filtered content".to_string(),
            case_sensitive: false,
            context_lines: 0,
            include,
            exclude,
        }))
        .expect("filtered text search");
    assert_eq!(matches.matches.len(), 2);
    assert!(
        matches
            .matches
            .iter()
            .all(|matched| matched.source.path != "正文/草稿/drop.md")
    );
}

#[test]
fn list_and_note_structure_tools_return_note_metadata() {
    let (_dir, server) = fixture();
    assert!(server.tool_router.has_route("get_note_structure"));
    assert!(!server.tool_router.has_route("parse_note"));

    let Json(listed) = server
        .list_notes(Parameters(EmptyRequest {}))
        .expect("list notes");
    assert_eq!(listed.notes.len(), 3);

    let Json(structure) = server
        .get_note_structure(Parameters(NoteStructureRequest {
            note: "发动机.md".to_string(),
        }))
        .expect("get note structure");
    assert_eq!(structure.path, "发动机.md");
    assert_eq!(structure.links.len(), 1);
    assert_eq!(structure.links[0].line, 5);
}

#[test]
fn search_and_frontmatter_tools_surface_results_and_regex_errors() {
    let (_dir, server) = fixture();

    let Json(text_result) = server
        .search_text(Parameters(SearchTextRequest {
            query: "林动".to_string(),
            case_sensitive: false,
            context_lines: 0,
            include: vec![],
            exclude: vec![],
        }))
        .expect("search text");
    assert_eq!(text_result.matches.len(), 2);

    let Json(frontmatter) = server
        .query_frontmatter(Parameters(crate::query::FrontmatterQueryOptions {
            field: "aliases".to_string(),
            mode: FrontmatterMatchMode::Exists,
            value: None,
        }))
        .expect("query frontmatter");
    assert_eq!(frontmatter.matches.len(), 1);

    let regex_error = match server.search_regex(Parameters(SearchRegexRequest {
        pattern: "(".to_string(),
        case_sensitive: false,
        context_lines: 0,
        include: vec![],
        exclude: vec![],
    })) {
        Ok(_) => panic!("invalid regex should fail"),
        Err(error) => error,
    };
    assert!(regex_error.contains("regex parse error"));
}

#[test]
fn list_vault_files_tool_respects_limits_and_readme_outline() {
    let (dir, server) = fixture();
    fs::create_dir_all(dir.path().join("正文")).expect("正文 dir");
    fs::write(dir.path().join("正文/README.md"), "# 正文索引\n").expect("write readme");
    fs::write(dir.path().join("图.png"), b"png").expect("write attachment");

    let Json(files) = server
        .list_vault_files(Parameters(VaultFilesRequest {
            include_files: true,
            include_attachments: true,
            include_readme_outline: true,
            max_files: 2,
        }))
        .expect("list files");
    assert_eq!(files.files.len(), 2);
    assert!(files.truncated_files > 0);

    let Json(full) = server
        .list_vault_files(Parameters(VaultFilesRequest {
            include_files: true,
            include_attachments: true,
            include_readme_outline: true,
            max_files: 100,
        }))
        .expect("list files full");
    let readme = full
        .files
        .iter()
        .find(|file| file.path == "正文/README.md")
        .expect("readme");
    assert_eq!(readme.outline.as_ref(), Some(&Vec::<String>::new()));
}

#[test]
fn context_and_graph_tools_handle_empty_and_successful_cases() {
    let (_dir, server) = fixture();

    let Json(note_context) = server
        .collect_note_context(Parameters(ContextNoteRequest {
            note: "林动.md".to_string(),
        }))
        .expect("note context");
    assert_eq!(note_context.groups[0].kind, "current");

    let Json(reference_context) = server
        .collect_reference_context(Parameters(ContextReferenceRequest {
            reference: "[[缺失]]".to_string(),
        }))
        .expect("reference context");
    assert!(reference_context.groups.is_empty());

    let Json(graph) = server
        .get_vault_graph(Parameters(EmptyRequest {}))
        .expect("graph");
    assert!(graph.nodes.iter().any(|node| node.path == "林动.md"));

    let Json(neighborhood) = server
        .get_graph_neighborhood(Parameters(GraphNeighborhoodOptions {
            target: "林动".to_string(),
            depth: 1,
            direction: GraphNeighborhoodDirection::Both,
            include_unresolved: false,
        }))
        .expect("graph neighborhood");
    assert!(neighborhood.nodes.iter().any(|node| node.path == "林动.md"));
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
            line: None,
            content: "## 原理\n\n已替换\n".to_string(),
        }))
        .expect("replace section");
    assert_eq!(replaced.note, "发动机.md");
    assert!(
        fs::read_to_string(dir.path().join("发动机.md"))
            .expect("read note")
            .contains("已替换")
    );

    let Json(unresolved) = server
        .find_unresolved_links(Parameters(EmptyRequest {}))
        .expect("find unresolved");
    assert!(!unresolved.links.is_empty());

    let Json(ambiguous) = server
        .find_ambiguous_links(Parameters(EmptyRequest {}))
        .expect("find ambiguous");
    assert!(ambiguous.links.is_empty());
}

#[test]
fn append_read_and_rename_block_id_tools_work_through_server_surface() {
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
            line: None,
            content: "\n补充说明\n".to_string(),
        }))
        .expect("append section");
    assert_eq!(appended.note, "块.md");

    let Json(read) = server
        .read_section(Parameters(ReadSectionRequest {
            note: "块.md".to_string(),
            heading: Some("正文".to_string()),
            block_id: None,
            line: None,
        }))
        .expect("read section");
    assert!(read.content.contains("补充说明"));

    let Json(renamed) = server
        .rename_block_id(Parameters(RenameBlockIdRequest {
            note: "块.md".to_string(),
            old_block_id: "state".to_string(),
            new_block_id: "status".to_string(),
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
            heading: Some("原理".to_string()),
        }))
        .expect("outline");
    assert_eq!(outline.outline[0].heading, "原理");

    let Json(tags) = server
        .get_tags(Parameters(TagsRequest {
            tags: vec!["状态/身体".to_string()],
            scope: TagScope::Note,
            verbose: true,
            include: vec![],
            exclude: vec![],
        }))
        .expect("get tags");
    assert_eq!(tags.tags.len(), 1);

    let Json(ambiguous) = server
        .find_ambiguous_links(Parameters(EmptyRequest {}))
        .expect("find ambiguous");
    assert_eq!(ambiguous.links.len(), 1);
}
