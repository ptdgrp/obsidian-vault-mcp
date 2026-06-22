use super::{
    LinkKind, NoteParser, ReferenceInfo, SectionInfo, SourceSpan, TagScope, byte_offset_for_line,
    byte_offset_for_location, heading_anchor, inline_raw, is_external_or_absolute_url,
    line_number_for_byte, local_markdown_link_target, normalize_relative_markdown_path,
    path_with_line_ref, percent_decode, recover_inline_source_span, reference_suffix, slice_text,
};
use markdown::reference::Reference;

#[test]
fn parse_relative_markdown_links_support_current_note_and_percent_decoding() {
    let parsed = NoteParser::parse(
        "正文/001.md".to_string(),
        "[当前](#身体)\n[编码](../%E5%8F%91%E5%8A%A8%E6%9C%BA.md#%E5%8E%9F%E7%90%86)\n[外链](mailto:test@example.com)\n",
        4096,
    )
    .expect("parse note");

    assert_eq!(parsed.links.len(), 2);
    assert_eq!(parsed.links[0].target, "正文/001.md");
    assert_eq!(
        parsed.links[0].reference,
        Some(ReferenceInfo::Heading {
            value: "身体".to_string()
        })
    );
    assert_eq!(parsed.links[0].kind, LinkKind::Markdown);

    assert_eq!(parsed.links[1].target, "发动机.md");
    assert_eq!(
        parsed.links[1].reference,
        Some(ReferenceInfo::Heading {
            value: "原理".to_string()
        })
    );
}

#[test]
fn parse_tags_distinguishes_section_and_line_scope() {
    let parsed = NoteParser::parse(
        "note.md".to_string(),
        "# 标题\n\n首行 #节标签\n继续内容 #行标签\n",
        4096,
    )
    .expect("parse note");

    assert_eq!(parsed.tags.len(), 2);
    assert_eq!(parsed.tags[0].scope, TagScope::Section);
    assert_eq!(parsed.tags[1].scope, TagScope::Line);
}

#[test]
fn path_with_line_ref_formats_single_and_multi_line_ranges() {
    assert_eq!(path_with_line_ref("note.md", 3, 3), "note.md#L3");
    assert_eq!(path_with_line_ref("note.md", 3, 5), "note.md#L3-L5");
}

#[test]
fn parser_helpers_reject_external_urls_and_invalid_relative_paths() {
    assert!(is_external_or_absolute_url("mailto:test@example.com"));
    assert!(is_external_or_absolute_url("/absolute/path.md"));
    assert!(!is_external_or_absolute_url("#身体"));

    assert_eq!(
        local_markdown_link_target("正文/001.md", "#身体"),
        Some((
            "正文/001.md".to_string(),
            Some(ReferenceInfo::Heading {
                value: "身体".to_string(),
            }),
        ))
    );
    assert!(local_markdown_link_target("正文/001.md", "../note.md?x=1").is_none());
    assert!(normalize_relative_markdown_path("正文/001.md", "../../outside.md").is_none());
}

#[test]
fn parser_helpers_decode_percent_encoding_and_calculate_offsets() {
    assert_eq!(
        percent_decode("%E6%9E%97%E5%8A%A8"),
        Some("林动".to_string())
    );
    assert!(percent_decode("%GG").is_none());

    let text = "林动\n发动机\n";
    assert_eq!(byte_offset_for_line(text, 1), 0);
    assert_eq!(byte_offset_for_line(text, 2), "林动\n".len());
    assert_eq!(byte_offset_for_location(text, 2, 2), "林动\n发".len());
}

#[test]
fn parser_helpers_build_and_recover_inline_ranges() {
    assert_eq!(
        inline_raw(
            "[[",
            "发动机",
            Some(&Reference::MultiHeading(vec![
                "章节".to_string(),
                "原理".to_string(),
            ])),
            Some(&"查看".to_string()),
        ),
        "[[发动机#章节#原理|查看]]"
    );
    assert_eq!(
        reference_suffix(&Reference::BlockId("state".to_string())),
        "#^state"
    );

    let original = SourceSpan {
        path: "note.md".to_string(),
        line_start: 1,
        line_end: 1,
        byte_start: 0,
        byte_end: 0,
        section: Some(SectionInfo {
            heading: "原理".to_string(),
            heading_level: 2,
            heading_path: vec!["发动机".to_string(), "原理".to_string()],
            heading_anchor: "原理".to_string(),
        }),
    };
    let mut search_start = 0;
    let recovered = recover_inline_source_span(
        "note.md",
        "前缀 [[发动机#原理]]",
        original.clone(),
        "[[发动机#原理]]",
        &mut search_start,
    );
    assert_eq!(
        slice_text(
            "前缀 [[发动机#原理]]",
            recovered.byte_start,
            recovered.byte_end
        ),
        "[[发动机#原理]]"
    );
    assert_eq!(search_start, recovered.byte_end);

    let unchanged = recover_inline_source_span(
        "note.md",
        "没有匹配",
        original.clone(),
        "[[不存在]]",
        &mut search_start,
    );
    assert_eq!(unchanged.byte_start, original.byte_start);
    assert_eq!(line_number_for_byte("甲\n乙\n", 0), 1);
    assert_eq!(line_number_for_byte("甲\n乙\n", "甲\n".len()), 2);
    assert_eq!(heading_anchor(" 原理\n补充 "), "原理 补充");
}

#[test]
fn parse_self_referential_wikilinks_normalizes_heading_and_block_targets() {
    let parsed = NoteParser::parse("note.md".to_string(), "[[#原理]]\n[[#^state]]\n", 4096)
        .expect("parse note");

    assert_eq!(parsed.links[0].target, "");
    assert_eq!(
        parsed.links[0].reference,
        Some(ReferenceInfo::Heading {
            value: "原理".to_string()
        })
    );
    assert_eq!(parsed.links[1].target, "");
    assert_eq!(
        parsed.links[1].reference,
        Some(ReferenceInfo::BlockId {
            value: "state".to_string()
        })
    );
}
