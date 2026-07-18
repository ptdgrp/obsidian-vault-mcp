use super::super::document::{DocumentSchema, ParsedDocument};

const SCHEMA: DocumentSchema = DocumentSchema {
    name: "test/v2",
    required_sections: &["Intent", "Results"],
};

#[test]
fn parses_frontmatter_and_ordered_sections_then_preserves_unknown_markdown() {
    let source = "---\nschema: test/v2\nid: doc-1\nstate: active\n---\n\n# 标题\n\n## Intent\n\n目标\n\n## Extra\n\n<!-- keep -->\n\n## Results\n\n结果\n";
    let parsed = ParsedDocument::parse("doc.md", source, SCHEMA).unwrap();
    assert_eq!(parsed.frontmatter_string("schema"), Some("test/v2"));
    assert_eq!(parsed.h1(), "标题");
    assert_eq!(
        parsed.section("Intent").unwrap().body(source).trim(),
        "目标"
    );

    let next = parsed.replace_section(source, "Results", "新结果").unwrap();
    assert!(next.contains("## Extra\n\n<!-- keep -->"));
    assert!(next.contains("## Results\n\n新结果"));
}

#[test]
fn rejects_missing_duplicate_sections_and_wrong_schema() {
    let missing = "---\nschema: test/v2\n---\n\n# T\n\n## Intent\n\nx\n";
    assert!(
        ParsedDocument::parse("doc.md", missing, SCHEMA)
            .unwrap_err()
            .to_string()
            .contains("missing required section: Results")
    );
    let duplicate =
        "---\nschema: test/v2\n---\n\n# T\n\n## Intent\n\na\n\n## Intent\n\nb\n\n## Results\n\nr\n";
    assert!(
        ParsedDocument::parse("doc.md", duplicate, SCHEMA)
            .unwrap_err()
            .to_string()
            .contains("required section must occur exactly once: Intent")
    );

    let wrong_schema = "---\nschema: other/v2\n---\n\n# T\n\n## Intent\n\nx\n\n## Results\n\nr\n";
    assert!(
        ParsedDocument::parse("doc.md", wrong_schema, SCHEMA)
            .unwrap_err()
            .to_string()
            .contains("unsupported schema: other/v2")
    );
}

#[test]
fn appends_to_sections_and_replaces_frontmatter_fields_in_place() {
    let source =
        "---\nschema: test/v2\nid: doc-1\n---\n\n# T\n\n## Intent\n\nx\n\n## Results\n\nr\n";
    let parsed = ParsedDocument::parse("doc.md", source, SCHEMA).unwrap();

    let appended = parsed.append_section(source, "Results", "more").unwrap();
    assert!(appended.contains("## Results\n\nr\n\nmore\n"));

    let next = parsed
        .replace_frontmatter_field(source, "id", "doc-2")
        .unwrap();
    assert!(
        next.starts_with("---\nschema: test/v2\nid: doc-2\n---"),
        "{next}"
    );
}
