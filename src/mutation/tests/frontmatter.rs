use super::{fixture, read_note, write_note};
use serde_json::json;

#[test]
fn field_types_preview_noop_and_history_preserve_body() {
    let (dir, mutations) = fixture();
    let original = "\u{feff}---\r\n# metadata\r\nkeep: 'quoted' # comment\r\nphase: old\r\n---\r\n# Body\r\n\r\n[[河流]]\r\n";
    write_note(&dir, "field.md", original);
    let value = json!({"string":"true", "list":[1,true,null], "nested":{"x":"中文"}});
    let preview = mutations
        .set_frontmatter_field("field.md", "phase", value.clone(), true)
        .unwrap();
    assert!(preview.changed);
    assert_eq!(preview.previous_value, "old");
    assert_eq!(read_note(&dir, "field.md"), original);
    assert!(mutations.edit_history().unwrap().undo.is_empty());
    mutations
        .set_frontmatter_field("field.md", "phase", value.clone(), false)
        .unwrap();
    let updated = read_note(&dir, "field.md");
    assert!(updated.starts_with("\u{feff}---\r\n# metadata\r\nkeep: 'quoted' # comment\r\n"));
    assert!(updated.ends_with("---\r\n# Body\r\n\r\n[[河流]]\r\n"));
    assert!(!updated.replace("\r\n", "").contains('\n'));
    assert!(
        !mutations
            .set_frontmatter_field("field.md", "phase", value, false)
            .unwrap()
            .changed
    );
    let history = mutations.edit_history().unwrap();
    assert_eq!(history.undo.len(), 1);
    assert_eq!(history.undo[0].command, "set_frontmatter_field");
    mutations.undo_edit(1).unwrap();
    assert_eq!(read_note(&dir, "field.md"), original);
    mutations.redo_edit(1).unwrap();
    assert_eq!(read_note(&dir, "field.md"), updated);
    let missing = mutations
        .set_frontmatter_field("field.md", "literal.dot", json!(null), false)
        .unwrap();
    assert!(!missing.previous_exists);
    assert_eq!(missing.result_value, json!(null));
    let null = mutations
        .set_frontmatter_field("field.md", "literal.dot", json!(null), true)
        .unwrap();
    assert!(null.previous_exists);
    assert!(!null.changed);
}

#[test]
fn lossless_scalar_flow_and_alias_edits() {
    let (dir, mutations) = fixture();
    for scalar in ["|", "|-", ">+"] {
        let source = format!(
            "---\nphase: {scalar}\n  first\n  ---\n  second\nkeep: 'original' # comment\n---\n# Body\n"
        );
        write_note(&dir, "scalar.md", &source);
        mutations
            .set_frontmatter_field("scalar.md", "phase", json!("new"), false)
            .unwrap();
        assert_eq!(
            read_note(&dir, "scalar.md"),
            "---\nphase: \"new\"\nkeep: 'original' # comment\n---\n# Body\n"
        );
    }
    write_note(&dir, "flow.md", "---\n{title: keep}\n---\n# Body\n");
    mutations
        .set_frontmatter_field("flow.md", "phase", json!("new"), false)
        .unwrap();
    assert_eq!(
        read_note(&dir, "flow.md"),
        "---\n{title: keep, \"phase\": \"new\"}\n---\n# Body\n"
    );
    write_note(
        &dir,
        "alias.md",
        "---\nkeep: &value old\nphase: *value\n---\nBody",
    );
    mutations
        .set_frontmatter_field("alias.md", "phase", json!(false), false)
        .unwrap();
    assert!(read_note(&dir, "alias.md").contains("keep: &value old\nphase: false\n"));
    write_note(&dir, "plain.md", "\u{feff}# Body\r\nno final newline");
    mutations
        .set_frontmatter_field("plain.md", "new", json!(123), false)
        .unwrap();
    assert_eq!(
        read_note(&dir, "plain.md"),
        "\u{feff}---\r\n\"new\": 123\r\n---\r\n# Body\r\nno final newline"
    );
}

#[test]
fn malformed_duplicates_and_metadata_side_effects_never_write() {
    let (dir, mutations) = fixture();
    for source in [
        "---\na: [\n---\nBody",
        "---\na: 1\nBody",
        "---\n[1,2]\n---\nBody",
        "---\nnull\n---\nBody",
        "---\na: 1\na: 2\n---\nBody",
        "---\nnested: {x: 1, x: 2}\n---\nBody",
        "---\nphase: &a old\nother: *a\n---\nBody",
        "---\nphase: &a {inherited: keep}\n<<: *a\n---\nBody",
    ] {
        write_note(&dir, "bad.md", source);
        assert!(
            mutations
                .set_frontmatter_field("bad.md", "phase", json!("new"), false)
                .is_err(),
            "{source}"
        );
        assert_eq!(read_note(&dir, "bad.md"), source);
        assert!(mutations.edit_history().unwrap().undo.is_empty());
    }
}

#[test]
fn alias_reference_protection_applies_to_metadata() {
    let (dir, mutations) = fixture();
    write_note(&dir, "target.md", "---\naliases: [Old]\n---\n# Target\n");
    write_note(&dir, "caller.md", "[[Old]]\n");
    assert!(
        mutations
            .set_frontmatter_field("target.md", "aliases", json!(["New"]), false)
            .is_err()
    );
    assert!(read_note(&dir, "target.md").contains("aliases: [Old]"));
    assert!(mutations.edit_history().unwrap().undo.is_empty());
}

#[test]
fn flow_trailing_commas_and_comments_remain_intact() {
    let (dir, mutations) = fixture();
    for source in [
        "{title: keep, # Keep comment\n}\n",
        "{title: keep # Keep comment\n}\n",
    ] {
        write_note(&dir, "flow.md", &format!("---\n{source}---\n# Body\n"));
        mutations
            .set_frontmatter_field("flow.md", "phase", json!("new"), false)
            .unwrap();
        let updated = read_note(&dir, "flow.md");
        assert!(updated.contains("# Keep comment\n"));
        let yaml = updated
            .strip_prefix("---\n")
            .unwrap()
            .split("---\n")
            .next()
            .unwrap();
        let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(value["title"].as_str(), Some("keep"));
        assert_eq!(value["phase"].as_str(), Some("new"));
    }
}

#[test]
fn field_mutations_invalidate_the_existing_parse_cache() {
    let (dir, mutations) = fixture();
    write_note(&dir, "cached.md", "---\nold: true\n---\n# Body\n");
    let before = mutations.queries.get_note_structure("cached.md").unwrap();
    assert_eq!(before.frontmatter_fields.unwrap(), vec!["old"]);
    mutations
        .set_frontmatter_field("cached.md", "added", json!(true), false)
        .unwrap();
    let after = mutations.queries.get_note_structure("cached.md").unwrap();
    assert!(after.frontmatter_fields.unwrap().contains(&"added".into()));
    mutations.undo_edit(1).unwrap();
    let undone = mutations.queries.get_note_structure("cached.md").unwrap();
    assert_eq!(undone.frontmatter_fields.unwrap(), vec!["old"]);
}
