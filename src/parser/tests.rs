use super::{
    LinkKind, NoteParser, ReferenceInfo, SectionInfo, SourceSpan, TagScope, byte_offset_for_line,
    heading_anchor, inline_raw, is_external_or_absolute_url, line_number_for_byte,
    local_markdown_link_target, normalize_relative_markdown_path, path_with_line_ref,
    percent_decode, recover_inline_source_span, reference_suffix, slice_text,
};
use ptdgrp_markdown::reference::Reference;
use std::sync::{Arc, Mutex};
use tracing::{Subscriber, field::Visit};
use tracing_subscriber::{Layer, layer::SubscriberExt};

#[derive(Clone, Default)]
struct CapturedSpanFields(Arc<Mutex<Vec<(String, String)>>>);

impl<S> Layer<S> for CapturedSpanFields
where
    S: Subscriber,
{
    fn on_new_span(
        &self,
        attributes: &tracing::span::Attributes<'_>,
        _id: &tracing::span::Id,
        _context: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut visitor = FieldVisitor::default();
        attributes.record(&mut visitor);
        self.0
            .lock()
            .expect("capture span fields")
            .extend(visitor.0);
    }
}

#[derive(Default)]
struct FieldVisitor(Vec<(String, String)>);

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0
            .push((field.name().to_string(), format!("{value:?}")));
    }
}

#[test]
fn parse_markdown_span_omits_note_content() {
    let captured = CapturedSpanFields::default();
    let fields = captured.0.clone();
    let subscriber = tracing_subscriber::registry().with(captured);

    tracing::subscriber::with_default(subscriber, || {
        NoteParser::parse("secret.md", "PRIVATE NOTE CONTENT", 4096).expect("parse note");
    });

    let fields = fields.lock().expect("read span fields");
    assert!(fields.iter().any(|(name, _)| name == "path_str"));
    assert!(fields.iter().any(|(name, _)| name == "input_bytes"));
    assert!(!fields.iter().any(|(name, _)| name == "text"));
    assert!(
        !fields
            .iter()
            .any(|(_, value)| value.contains("PRIVATE NOTE CONTENT"))
    );
}

#[test]
fn parse_relative_markdown_links_support_current_note_and_percent_decoding() {
    let parsed = NoteParser::parse(
        "正文/001.md",
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
fn parse_markdown_footnote_labels_do_not_create_wikilinks() {
    let parsed = NoteParser::parse(
        "note.md",
        "格雷厄姆^[[[\\[1\\]]{.calibre3}](#index_split_003.html_filepos70626)]{.small}^教派\n",
        4096,
    )
    .expect("parse note");

    assert!(parsed.links.is_empty());
}

#[test]
fn parse_tags_distinguishes_section_and_line_scope() {
    let parsed = NoteParser::parse(
        "note.md",
        "# 标题\n\n首行 #节标签\n继续内容 #行标签\n",
        4096,
    )
    .expect("parse note");

    assert_eq!(parsed.tags.len(), 2);
    assert_eq!(parsed.tags[0].scope, TagScope::Section);
    assert_eq!(parsed.tags[1].scope, TagScope::Line);
}

#[test]
fn parse_headings_preserves_visible_inline_text() {
    let parsed = NoteParser::parse(
        "note.md",
        "# **Bold** and *emphasis* and [Link](other.md) and ==mark==\n",
        4096,
    )
    .expect("parse note");

    assert_eq!(parsed.headings.len(), 1);
    assert_eq!(
        parsed.headings[0].text,
        "Bold and emphasis and Link and mark"
    );
    assert!(parsed.headings[0].path.is_empty());
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
    let parsed =
        NoteParser::parse("note.md", "[[#原理]]\n[[#^state]]\n", 4096).expect("parse note");

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

#[test]
fn selective_headings_match_full_parse_with_markdown_context() {
    let source = "---\ntitle: Metadata\n---\n# Root\n\n## **Strong** and [reference][ref]\n\nBody with [[links]], #tags and *formatting*.\n\n### 子章节 `code`\n\n```md\n## Not a heading\n```\n\n> ## Quoted heading\n> text\n\nSetext *heading*\n----------------\n\n## Same\n\n## Same\n\n[ref]: target.md\n";
    let full = NoteParser::parse("note.md", source, usize::MAX).unwrap();
    let headings = NoteParser::parse_headings("note.md", source, usize::MAX).unwrap();
    assert!(!headings.is_empty());
    assert_eq!(
        serde_json::to_value(&headings).unwrap(),
        serde_json::to_value(&full.headings).unwrap()
    );
    for (selective, full) in headings.iter().zip(&full.headings) {
        assert_eq!(selective.source, full.source);
    }
}
