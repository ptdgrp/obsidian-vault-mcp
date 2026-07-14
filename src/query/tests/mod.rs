use std::fs;

use camino::Utf8PathBuf;
use tempfile::tempdir;

use super::*;
use crate::vault::{DEFAULT_MAX_READ_NOTE_CHARS, Vault, VaultConfig, VaultError};

mod contract_primitives;
mod edge_cases;
mod search_and_section;

fn fixture() -> (tempfile::TempDir, VaultQueries) {
    let dir = tempdir().expect("tempdir");
    fs::write(
            dir.path().join("林动.md"),
            "---\naliases:\n  - 动林\ntags:\n  - 主角\n  - 状态/身体\nphase: active\narc: 引擎线\n---\n# 林动\n\n身体 #状态/身体\n\n[[发动机#原理|发动机]]\n\n[[缺失设定]]\n",
        )
        .expect("write note");
    fs::create_dir(dir.path().join("正文")).expect("正文 dir");
    fs::write(dir.path().join("正文/README.md"), "# 正文索引\n").expect("write readme");
    fs::write(
        dir.path().join("正文/001.md"),
        "# 第一章\n\n## 代偿\n\n林动在雨里完成第一次代偿。\n\n[发动机普通链接](../发动机.md)\n\n[越界链接](../../outside.md)\n",
    )
    .expect("write chapter");
    fs::write(
        dir.path().join("河流.md"),
        "# 卡尔特兰河\n\nThe Kaltram #河流\n发源于北方雪山冰川，最终注入东部开拓州的内陆盐湖盆地中。 #设定\n",
    )
    .expect("write river note");
    fs::create_dir(dir.path().join("正文/场景")).expect("empty scene dir");
    fs::write(dir.path().join("地图.png"), b"png").expect("write attachment");
    fs::write(
        dir.path().join("发动机.md"),
        "# 发动机\n\n## 原理\n\n链接到 [[林动]]\n",
    )
    .expect("write note");
    fs::create_dir(dir.path().join("资料")).expect("资料 dir");
    fs::write(
        dir.path().join("资料/发动机.md"),
        "# 另一个发动机\n\n用于制造歧义。\n",
    )
    .expect("write note");
    fs::write(dir.path().join("资料/.gitignore"), "ignored-here.md\n").expect("nested gitignore");
    fs::write(dir.path().join("资料/ignored-here.md"), "# ignored\n")
        .expect("nested gitignored note");
    fs::create_dir(dir.path().join(".obsidian")).expect("obsidian dir");
    fs::write(dir.path().join(".obsidian/ignored.md"), "# ignored\n").expect("ignored");
    fs::create_dir_all(dir.path().join(".agents/skills")).expect("agents dir");
    fs::write(dir.path().join(".agents/skills/ignored.md"), "# ignored\n").expect("ignored");
    fs::write(dir.path().join(".hidden.md"), "# ignored\n").expect("ignored");
    fs::write(
        dir.path().join(".gitignore"),
        "ignored-by-git.md\nignored-dir/\n",
    )
    .expect("gitignore");
    fs::write(dir.path().join("ignored-by-git.md"), "# ignored\n").expect("gitignored note");
    fs::create_dir(dir.path().join("ignored-dir")).expect("gitignored dir");
    fs::write(dir.path().join("ignored-dir/ignored.md"), "# ignored\n")
        .expect("gitignored nested note");

    let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path");
    let vault = Vault::open(root, VaultConfig::default()).expect("vault");
    (dir, VaultQueries::new(vault))
}

#[test]
fn vault_path_cannot_escape_root() {
    let (_dir, queries) = fixture();
    let err = queries.vault.resolve_path("../outside.md").unwrap_err();
    assert!(matches!(err, VaultError::PathEscapesVault));
}

#[test]
fn list_notes_ignores_hidden_paths() {
    let (_dir, queries) = fixture();
    let notes = queries.list_notes(&[], &[], 1).expect("list notes");
    assert_eq!(notes.notes.len(), 6);
    assert!(
        !notes
            .notes
            .iter()
            .any(|note| note.path.contains(".obsidian"))
    );
    assert!(!notes.notes.iter().any(|note| note.path.contains(".agents")));
    assert!(!notes.notes.iter().any(|note| note.path.contains(".hidden")));
    assert!(
        !notes
            .notes
            .iter()
            .any(|note| note.path == "资料/ignored-here.md")
    );
    let value = serde_json::to_value(notes).expect("list notes json");
    assert!(value["notes"][0].get("preview").is_none());
    assert!(value["notes"][0].get("size").is_none());
}

#[test]
fn list_notes_title_prefers_h1_then_frontmatter_title_then_pathname() {
    let (dir, queries) = fixture();
    fs::write(
        dir.path().join("标题优先.md"),
        "---\ntitle: Frontmatter Title\n---\n# H1 Title\n\n## Section\n",
    )
    .expect("write h1 title note");
    fs::write(
        dir.path().join("仅元数据.md"),
        "---\ntitle: Metadata Title\n---\n\n## Section\n",
    )
    .expect("write frontmatter title note");
    fs::write(dir.path().join("只有路径.md"), "## Section\n").expect("write pathname note");

    let notes = queries.list_notes(&[], &[], 1).expect("list notes");
    let title_for = |path: &str| {
        notes
            .notes
            .iter()
            .find(|note| note.path == path)
            .and_then(|note| note.title.as_deref())
    };

    assert_eq!(title_for("标题优先.md"), Some("H1 Title"));
    assert_eq!(title_for("仅元数据.md"), Some("Metadata Title"));
    assert_eq!(title_for("只有路径.md"), Some("只有路径"));
}

#[test]
fn list_notes_filters_before_fixed_page_and_omits_non_navigation_fields() {
    let (dir, queries) = fixture();
    fs::create_dir_all(dir.path().join("人物/草稿")).expect("draft dir");
    fs::write(dir.path().join("人物/林动.md"), "# 林动\n").expect("write character");
    fs::write(dir.path().join("人物/草稿/林动.md"), "# 草稿\n").expect("write draft");

    let result = queries
        .list_notes(
            &["人物/**/*.md".to_string(), "正文/**/*.md".to_string()],
            &["**/草稿/**".to_string()],
            1,
        )
        .expect("filtered notes");

    assert_eq!(
        result
            .notes
            .iter()
            .map(|note| &note.path)
            .collect::<Vec<_>>(),
        vec![
            &"人物/林动.md".to_string(),
            &"正文/001.md".to_string(),
            &"正文/README.md".to_string()
        ]
    );
    assert_eq!(result.pagination.page, 1);
    assert_eq!(result.pagination.total_pages, 1);
    assert_eq!(result.pagination.total_notes, 3);
    let value = serde_json::to_value(result).expect("json");
    assert!(
        value["notes"]
            .as_array()
            .expect("notes")
            .iter()
            .all(|note| note.get("size").is_none())
    );
}

#[test]
fn parse_note_extracts_obsidian_structures_and_sections() {
    let (_dir, queries) = fixture();
    let parsed = queries.parse_note("林动").expect("parse");
    assert_eq!(parsed.headings[0].text, "林动");
    let link = parsed
        .links
        .iter()
        .find(|link| link.target == "发动机")
        .expect("发动机 link");
    assert_eq!(link.alias.as_deref(), Some("发动机"));
    assert_eq!(parsed.tags[0].tag, "状态/身体");
    assert_eq!(
        link.source
            .section
            .as_ref()
            .map(|section| section.heading_path.clone()),
        Some(vec![])
    );
}

#[test]
fn note_structure_returns_bounded_compact_inspection_groups() {
    let (dir, queries) = fixture();
    let mut content = String::from(
        "---\naliases: [Alias]\nnested: {key: value}\ntags: [front/b, front/a]\n---\n# Title\n\n[[Target]]\n![[Image.png]]\n![[Image.png]]\n^dup\n^dup\n",
    );
    for index in 0..51 {
        content.push_str(&format!(
            "\n## Section {index:02}\n\n#tag{index:02} #tag00\n"
        ));
    }
    fs::write(dir.path().join("Structure.md"), content).expect("write structure note");

    let result = queries
        .get_note_structure("Structure.md")
        .expect("structure");
    let value = serde_json::to_value(result).expect("structure json");

    assert_eq!(value["note"], "Structure.md");
    assert_eq!(value["link_count"], 1);
    assert_eq!(
        value["frontmatter_fields"],
        serde_json::json!(["aliases", "nested", "tags"])
    );
    assert!(value.get("frontmatter").is_none());
    assert!(value.get("links").is_none());
    assert_eq!(value["embeds"], serde_json::json!(["Image.png"]));
    assert_eq!(value["blocks"], serde_json::json!(["dup"]));

    let headings = value["headings"].as_array().expect("headings");
    assert_eq!(headings.len(), 50);
    assert_eq!(
        headings[0],
        serde_json::json!({"heading": "Section 00", "line": 14})
    );
    assert_eq!(
        headings[49],
        serde_json::json!({"heading": "Section 49", "line": 210})
    );
    assert!(headings.iter().all(|heading| heading["heading"] != "Title"));

    let tags = value["tags"].as_array().expect("tags");
    assert_eq!(tags.len(), 50);
    assert_eq!(tags[0], "front/a");
    assert_eq!(tags[49], "tag47");
    assert_eq!(value["omitted"], serde_json::json!(["headings", "tags"]));
}

#[test]
fn get_note_stats_counts_words_characters_and_total_backlinks() {
    let (_dir, queries) = fixture();
    let content = "---\naliases:\n  - 动林\ntags:\n  - 主角\n  - 状态/身体\nphase: active\narc: 引擎线\n---\n# 林动\n\n身体 #状态/身体\n\n[[发动机#原理|发动机]]\n\n[[缺失设定]]\n";

    let result = queries.get_note_stats("林动").expect("note stats");
    assert_eq!(result.scope, "林动.md");
    assert_eq!(result.word_count, 36);
    assert_eq!(result.character_count, content.chars().count());
    assert_eq!(result.line_count, content.lines().count());
    assert_eq!(result.backlink_count, 1);
    let value = serde_json::to_value(result).expect("stats json");
    assert!(value.get("word_count_mode").is_none());
    assert!(value.get("source").is_none());
}

#[test]
fn note_stats_scopes_text_and_backlinks_to_normalized_ref() {
    let (dir, queries) = fixture();
    fs::write(
        dir.path().join("Target.md"),
        "# Target\n\n## Section\n\nalpha beta\n\n^state\n",
    )
    .expect("write target");
    fs::write(dir.path().join("heading-link.md"), "[[Target#Section]]\n")
        .expect("write heading link");
    fs::write(dir.path().join("block-link.md"), "[[Target#^state]]\n").expect("write block link");
    fs::write(dir.path().join("note-link.md"), "[[Target]]\n").expect("write note link");

    let heading = queries
        .get_note_stats("Target#Section")
        .expect("heading stats");
    assert_eq!(heading.scope, "Target.md#Section");
    assert_eq!(heading.word_count, 4);
    assert_eq!(heading.line_count, 5);
    assert_eq!(heading.backlink_count, 1);

    let block = queries
        .get_note_stats("Target#^state")
        .expect("block stats");
    assert_eq!(block.scope, "Target.md#^state");
    assert_eq!(block.word_count, 1);
    assert_eq!(block.line_count, 1);
    assert_eq!(block.backlink_count, 1);

    let whole = queries.get_note_stats("Target").expect("whole-note stats");
    assert_eq!(whole.scope, "Target.md");
    assert_eq!(whole.backlink_count, 3);
    let value = serde_json::to_value(heading).expect("heading stats json");
    assert!(value.get("source").is_none());
    assert!(value.get("mode").is_none());
}

#[test]
fn get_note_stats_counts_blank_lines_without_extra_trailing_line() {
    let (dir, queries) = fixture();
    fs::write(dir.path().join("line-count.md"), "first\n\nthird\n").expect("write line count note");

    let result = queries.get_note_stats("line-count.md").expect("note stats");

    assert_eq!(result.line_count, 3);
}

#[test]
fn get_note_stats_treats_hyphenated_ascii_sequences_as_one_word() {
    let (dir, queries) = fixture();
    std::fs::write(dir.path().join("hyphen.md"), "foo-bar baz\n").expect("write hyphen note");

    let result = queries.get_note_stats("hyphen.md").expect("note stats");
    assert_eq!(result.word_count, 2);
}

#[test]
fn note_lookup_ignores_numeric_sort_prefix_when_no_exact_note_exists() {
    let (dir, queries) = fixture();
    std::fs::write(dir.path().join("001-排序标题.md"), "# 排序标题\n\n正文\n")
        .expect("write numbered note");

    let result = queries
        .read_note("排序标题", None, None)
        .expect("read note");

    assert_eq!(result.source, "001-排序标题.md#L1-L3");
}

#[test]
fn parse_note_extracts_safe_relative_markdown_links() {
    let (_dir, queries) = fixture();
    let parsed = queries.parse_note("正文/001").expect("parse");
    let link = parsed
        .links
        .iter()
        .find(|link| link.target == "发动机.md")
        .expect("relative markdown link");
    assert_eq!(link.kind, crate::parser::LinkKind::Markdown);
    assert_eq!(link.alias.as_deref(), Some("发动机普通链接"));
    assert!(
        !parsed
            .links
            .iter()
            .any(|link| link.raw.contains("越界链接"))
    );
}

#[test]
fn list_tags_paginates_unique_tag_names() {
    let (dir, queries) = fixture();
    fs::create_dir(dir.path().join("标签分页")).expect("tag page dir");
    for index in 1..=101 {
        fs::write(
            dir.path().join(format!("标签分页/{index:03}.md")),
            format!("# Tag {index}\n\n#tag-{index:03}\n"),
        )
        .expect("write tag page note");
    }
    let include = vec!["标签分页/**/*.md".to_string()];

    let page_1 = queries
        .list_tags(TagScope::Note, &include, &[], 1)
        .expect("list tags page 1");
    let page_2 = queries
        .list_tags(TagScope::Note, &include, &[], 2)
        .expect("list tags page 2");
    let out_of_range = queries
        .list_tags(TagScope::Note, &include, &[], 3)
        .expect("list tags page 3");
    let empty = queries
        .list_tags(TagScope::Note, &["missing/**/*.md".to_string()], &[], 1)
        .expect("empty list tags");

    assert_eq!(page_1.tags.len(), 100);
    assert_eq!(page_1.tags[0], "tag-001");
    assert_eq!(page_1.tags[99], "tag-100");
    assert_eq!(page_1.pagination.total_tags, 101);
    assert_eq!(page_1.pagination.total_pages, 2);
    assert_eq!(page_2.tags, vec!["tag-101".to_string()]);
    assert!(out_of_range.tags.is_empty());
    assert_eq!(out_of_range.pagination.page, 3);
    assert_eq!(empty.pagination.total_tags, 0);
    assert!(queries.list_tags(TagScope::Note, &include, &[], 0).is_err());
}

#[test]
fn get_tag_returns_scope_specific_locators_and_deduplicates() {
    let (dir, queries) = fixture();
    fs::write(
        dir.path().join("标签章节.md"),
        "# Note title\n\nNote tag #scope\n\n## Unique\n\nUnique tag #scope\nRepeated section tag #scope\n\n## Parent A\n\n### Detail\n\nA tag #scope\n\n## Parent B\n\n### Detail\n\nB tag #scope\n",
    )
    .expect("write tagged sections");

    let note = queries
        .get_tag("状态/身体", TagScope::Note, &[], &[], 1)
        .expect("note tag");
    assert_eq!(note.matches, vec!["林动.md".to_string()]);

    let frontmatter = queries
        .get_tag("状态/身体", TagScope::Frontmatter, &[], &[], 1)
        .expect("frontmatter tag");
    assert_eq!(frontmatter.matches, vec!["林动.md".to_string()]);

    let body = queries
        .get_tag("状态/身体", TagScope::Body, &[], &[], 1)
        .expect("body tag");
    assert_eq!(body.matches, vec!["林动.md".to_string()]);

    let sections = queries
        .get_tag("scope", TagScope::Section, &[], &[], 1)
        .expect("section tag");
    assert_eq!(
        sections.matches,
        vec![
            "标签章节.md".to_string(),
            "标签章节.md#Parent A#Detail".to_string(),
            "标签章节.md#Parent B#Detail".to_string(),
            "标签章节.md#Unique".to_string(),
        ]
    );

    let lines = queries
        .get_tag("scope", TagScope::Line, &[], &[], 1)
        .expect("line tag");
    assert_eq!(
        lines.matches,
        vec![
            "标签章节.md#L3".to_string(),
            "标签章节.md#L7".to_string(),
            "标签章节.md#L8".to_string(),
            "标签章节.md#L14".to_string(),
            "标签章节.md#L20".to_string(),
        ]
    );

    let value = serde_json::to_value(&sections).expect("tag json");
    assert!(value.get("tag").is_none());
    assert!(value.get("scope").is_none());
    assert!(value.get("tags").is_none());
}

#[test]
fn get_tag_paginates_fixed_locators() {
    let (dir, queries) = fixture();
    fs::create_dir(dir.path().join("标签定位分页")).expect("tag locator page dir");
    for index in 1..=101 {
        fs::write(
            dir.path().join(format!("标签定位分页/{index:03}.md")),
            "# Paged\n\n#paged\n",
        )
        .expect("write tag locator note");
    }
    let include = vec!["标签定位分页/**/*.md".to_string()];

    let page_1 = queries
        .get_tag("paged", TagScope::Note, &include, &[], 1)
        .expect("tag page 1");
    let page_2 = queries
        .get_tag("paged", TagScope::Note, &include, &[], 2)
        .expect("tag page 2");
    let out_of_range = queries
        .get_tag("paged", TagScope::Note, &include, &[], 3)
        .expect("tag page 3");
    let empty = queries
        .get_tag("missing", TagScope::Note, &include, &[], 1)
        .expect("empty get tag");

    assert_eq!(page_1.matches.len(), 100);
    assert_eq!(page_1.matches[0], "标签定位分页/001.md");
    assert_eq!(page_1.matches[99], "标签定位分页/100.md");
    assert_eq!(page_1.pagination.total_matches, 101);
    assert_eq!(page_2.matches, vec!["标签定位分页/101.md".to_string()]);
    assert!(out_of_range.matches.is_empty());
    assert_eq!(empty.pagination.total_matches, 0);
    assert!(
        queries
            .get_tag("paged", TagScope::Note, &include, &[], 0)
            .is_err()
    );
}

#[test]
fn read_note_accepts_compact_slash_separated_heading_paths() {
    let (dir, queries) = fixture();
    fs::write(
        dir.path().join("重复章节.md"),
        "# Note title\n\n## Parent A\n\n### Detail\n\nA content\n\n## Parent B\n\n### Detail\n\nB content\n",
    )
    .expect("write duplicate sections");

    let result = queries
        .read_note(
            "重复章节.md",
            None,
            Some(SectionSelector::Heading {
                heading: "Parent B/Detail".to_string(),
            }),
        )
        .expect("read compact heading path");

    assert!(result.content.contains("B content"));
    assert!(!result.content.contains("A content"));
}

#[test]
fn list_categories_paginates_unique_folder_names() {
    let (dir, queries) = fixture();
    for index in 1..=101 {
        let parent = dir.path().join(format!("分类分页/category-{index:03}"));
        fs::create_dir_all(&parent).expect("create category dir");
        fs::write(parent.join("note.md"), "# Category\n").expect("write category note");
    }
    let include = vec!["分类分页/**/*.md".to_string()];

    let page_1 = queries
        .list_categories(&include, &[], 1)
        .expect("list categories page 1");
    let page_2 = queries
        .list_categories(&include, &[], 2)
        .expect("list categories page 2");
    let out_of_range = queries
        .list_categories(&include, &[], 3)
        .expect("list categories page 3");
    let empty = queries
        .list_categories(&["missing/**/*.md".to_string()], &[], 1)
        .expect("empty categories");

    assert_eq!(page_1.categories.len(), 100);
    assert_eq!(page_1.categories[0], "category-001");
    assert_eq!(page_1.categories[99], "category-100");
    assert_eq!(page_1.pagination.total_categories, 102);
    assert_eq!(page_1.pagination.total_pages, 2);
    assert_eq!(
        page_2.categories,
        vec!["category-101".to_string(), "分类分页".to_string()]
    );
    assert!(out_of_range.categories.is_empty());
    assert_eq!(empty.pagination.total_categories, 0);
    assert!(queries.list_categories(&include, &[], 0).is_err());
}

#[test]
fn get_category_matches_normalized_segment_and_paginates_notes() {
    let (dir, queries) = fixture();
    for index in 1..=101 {
        let parent = dir.path().join(format!("分类定位分页/{index:03}/目标"));
        fs::create_dir_all(&parent).expect("create category locator dir");
        fs::write(parent.join("note.md"), "# Category locator\n").expect("write category locator");
    }
    fs::create_dir_all(dir.path().join("分类定位分页/not-target")).expect("other dir");
    fs::write(
        dir.path().join("分类定位分页/not-target/note.md"),
        "# Other\n",
    )
    .expect("other note");
    let include = vec!["分类定位分页/**/*.md".to_string()];

    let page_1 = queries
        .get_category(" /目标/ ", &include, &[], 1)
        .expect("category page 1");
    let page_2 = queries
        .get_category("目标", &include, &[], 2)
        .expect("category page 2");
    let out_of_range = queries
        .get_category("目标", &include, &[], 3)
        .expect("category page 3");
    let empty = queries
        .get_category("missing", &include, &[], 1)
        .expect("empty category");

    assert_eq!(page_1.notes.len(), 100);
    assert_eq!(page_1.notes[0], "分类定位分页/001/目标/note.md");
    assert_eq!(page_1.notes[99], "分类定位分页/100/目标/note.md");
    assert_eq!(page_1.pagination.total_notes, 101);
    assert_eq!(
        page_2.notes,
        vec!["分类定位分页/101/目标/note.md".to_string()]
    );
    assert!(out_of_range.notes.is_empty());
    assert_eq!(empty.pagination.total_notes, 0);
    assert!(queries.get_category("目标", &include, &[], 0).is_err());
    assert!(
        queries
            .get_category("目标/子类", &include, &[], 1)
            .expect_err("internal slash should fail")
            .to_string()
            .contains("category must be a single folder name")
    );

    let value = serde_json::to_value(&page_1).expect("category json");
    assert!(value.get("category").is_none());
    assert!(value.get("categories").is_none());
    assert!(value.get("files").is_none());
}

#[test]
fn query_path_filters_limit_tags_categories_and_searches_before_aggregation() {
    let (dir, mut queries) = fixture();
    for path in ["正文/keep.md", "资料/keep.md", "正文/草稿/drop.md"] {
        if let Some(parent) = dir.path().join(path).parent() {
            fs::create_dir_all(parent).expect("create note directory");
        }
        fs::write(
            dir.path().join(path),
            "# Filtered\n\n#筛选标签\n\nshared filtered content\n",
        )
        .expect("write filtered note");
    }
    let include = vec!["正文/**/*.md".to_string(), "资料/**/*.md".to_string()];
    let exclude = vec!["**/草稿/**".to_string()];

    let tags = queries
        .list_tags(TagScope::Note, &include, &exclude, 1)
        .expect("list filtered tags");
    assert_eq!(tags.tags, vec!["筛选标签".to_string()]);

    let selected_tags = queries
        .get_tag("筛选标签", TagScope::Note, &include, &exclude, 1)
        .expect("get filtered tags");
    assert_eq!(selected_tags.matches.len(), 2);
    assert!(
        selected_tags
            .matches
            .iter()
            .all(|path| path != "正文/草稿/drop.md")
    );

    let categories = queries
        .list_categories(&include, &exclude, 1)
        .expect("list filtered categories");
    assert_eq!(
        categories.categories,
        vec!["正文".to_string(), "资料".to_string()]
    );

    let selected_categories = queries
        .get_category("正文", &include, &exclude, 1)
        .expect("get filtered categories");
    assert!(
        selected_categories
            .notes
            .contains(&"正文/keep.md".to_string())
    );
    assert!(
        !selected_categories
            .notes
            .contains(&"正文/草稿/drop.md".to_string())
    );

    let text = queries
        .search_text("shared filtered content", false, &include, &exclude, 1)
        .expect("filtered text search");
    assert_eq!(text.matches.len(), 2);
    assert!(
        text.matches
            .iter()
            .all(|matched| !matched.source.starts_with("正文/草稿/drop.md"))
    );

    let regex = queries
        .search_regex("shared filtered content", false, &include, &exclude, 1)
        .expect("filtered regex search");
    assert_eq!(regex.matches.len(), 2);
    assert!(
        regex
            .matches
            .iter()
            .all(|matched| !matched.source.starts_with("正文/草稿/drop.md"))
    );

    queries
        .vault
        .config
        .exclude
        .push("资料/**/*.md".to_string());
    let globally_hidden = queries
        .search_text(
            "shared filtered content",
            false,
            &["资料/**/*.md".to_string()],
            &[],
            1,
        )
        .expect("request filter cannot restore globally excluded note");
    assert!(globally_hidden.matches.is_empty());
}

#[test]
fn query_frontmatter_returns_paged_paths_and_validates_mode_values() {
    let (dir, queries) = fixture();
    fs::create_dir(dir.path().join("元数据分页")).expect("frontmatter page dir");
    for index in 1..=101 {
        fs::write(
            dir.path().join(format!("元数据分页/{index:03}.md")),
            format!(
                "---\nphase: active\narc: 引擎线 {index}\nmeta:\n  nested: true\n---\n# Metadata\n"
            ),
        )
        .expect("write frontmatter page note");
    }
    let include = vec!["元数据分页/**/*.md".to_string()];

    let exists = queries
        .query_frontmatter(FrontmatterQueryOptions {
            field: "phase".to_string(),
            mode: FrontmatterMatchMode::Exists,
            value: None,
            include: include.clone(),
            exclude: vec![],
            page: 1,
        })
        .expect("exists query");
    assert_eq!(exists.notes.len(), 100);
    assert_eq!(exists.notes[0], "元数据分页/001.md");
    assert_eq!(exists.pagination.total_notes, 101);
    assert_eq!(exists.pagination.total_pages, 2);

    let equals = queries
        .query_frontmatter(FrontmatterQueryOptions {
            field: "phase".to_string(),
            mode: FrontmatterMatchMode::Equals,
            value: Some("active".to_string()),
            include: include.clone(),
            exclude: vec![],
            page: 2,
        })
        .expect("equals query");
    assert_eq!(equals.notes, vec!["元数据分页/101.md".to_string()]);

    let regex = queries
        .query_frontmatter(FrontmatterQueryOptions {
            field: "arc".to_string(),
            mode: FrontmatterMatchMode::Regex,
            value: Some("引擎线 0.*".to_string()),
            include: include.clone(),
            exclude: vec!["**/010.md".to_string()],
            page: 1,
        })
        .expect("regex query");
    assert!(!regex.notes.contains(&"元数据分页/010.md".to_string()));
    assert!(
        regex
            .notes
            .iter()
            .all(|path| path.starts_with("元数据分页/"))
    );

    let out_of_range = queries
        .query_frontmatter(FrontmatterQueryOptions {
            field: "phase".to_string(),
            mode: FrontmatterMatchMode::Exists,
            value: None,
            include: include.clone(),
            exclude: vec![],
            page: 3,
        })
        .expect("frontmatter out of range");
    assert!(out_of_range.notes.is_empty());
    assert_eq!(out_of_range.pagination.total_pages, 2);

    let empty = queries
        .query_frontmatter(FrontmatterQueryOptions {
            field: "missing".to_string(),
            mode: FrontmatterMatchMode::Exists,
            value: None,
            include,
            exclude: vec![],
            page: 1,
        })
        .expect("empty frontmatter");
    assert_eq!(empty.pagination.total_notes, 0);

    assert!(
        queries
            .query_frontmatter(FrontmatterQueryOptions {
                field: "phase".to_string(),
                mode: FrontmatterMatchMode::Exists,
                value: Some("active".to_string()),
                include: vec![],
                exclude: vec![],
                page: 1,
            })
            .expect_err("exists with value should fail")
            .to_string()
            .contains("exists query does not accept a value")
    );
    assert!(
        queries
            .query_frontmatter(FrontmatterQueryOptions {
                field: "phase".to_string(),
                mode: FrontmatterMatchMode::Equals,
                value: None,
                include: vec![],
                exclude: vec![],
                page: 1,
            })
            .is_err()
    );
    assert!(
        queries
            .query_frontmatter(FrontmatterQueryOptions {
                field: "phase".to_string(),
                mode: FrontmatterMatchMode::Regex,
                value: None,
                include: vec![],
                exclude: vec![],
                page: 1,
            })
            .is_err()
    );
    assert!(
        queries
            .query_frontmatter(FrontmatterQueryOptions {
                field: "phase".to_string(),
                mode: FrontmatterMatchMode::Exists,
                value: None,
                include: vec![],
                exclude: vec![],
                page: 0,
            })
            .is_err()
    );

    let value = serde_json::to_value(&exists).expect("frontmatter json");
    assert!(value.get("matches").is_none());
    assert!(value.get("field").is_none());
    assert!(value.get("mode").is_none());
    assert!(value.get("value").is_none());
}

#[test]
fn parse_cache_reuses_entries_and_invalidates_on_file_change() {
    let (dir, queries) = fixture();
    let parsed = queries.parse_note("林动").expect("parse");
    assert_eq!(parsed.headings[0].text, "林动");
    assert_eq!(queries.parse_cache.len(), 1);

    fs::write(dir.path().join("林动.md"), "# 林动新版\n\n内容变长。\n").expect("rewrite");
    let reparsed = queries.parse_note("林动").expect("reparse");
    assert_eq!(reparsed.headings[0].text, "林动新版");
    assert_eq!(queries.parse_cache.len(), 1);
}

#[test]
fn parse_cache_prunes_by_ttl_and_entry_limit() {
    let dir = tempdir().expect("tempdir");
    fs::write(dir.path().join("a.md"), "# A\n").expect("write a");
    fs::write(dir.path().join("b.md"), "# B\n").expect("write b");
    fs::write(dir.path().join("c.md"), "# C\n").expect("write c");

    let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path");
    let vault = Vault::open(
        root,
        VaultConfig {
            parse_cache_ttl_secs: 600,
            parse_cache_max_entries: 2,
            ..VaultConfig::default()
        },
    )
    .expect("vault");
    let queries = VaultQueries::new(vault);

    queries.parse_note("a").expect("parse a");
    queries.parse_note("b").expect("parse b");
    queries.parse_note("c").expect("parse c");
    assert_eq!(queries.parse_cache.len(), 2);

    queries.parse_cache.expire_all_for_test();
    queries.parse_note("a").expect("parse a after expiry");
    assert_eq!(queries.parse_cache.len(), 1);
}

#[test]
fn resolve_ref_outputs_compact_resolved_multi_heading_json() {
    let (dir, queries) = fixture();
    fs::create_dir_all(dir.path().join("人物")).expect("create characters dir");
    fs::write(
        dir.path().join("人物/林动.md"),
        "# 林动\n\n## 身体\n\n### 伤势\n\n内容\n",
    )
    .expect("write character");

    let resolved = queries
        .resolve_ref("[[人物/林动#身体#伤势]]")
        .expect("resolve");
    let value = serde_json::to_value(resolved).expect("resolve json");

    assert_eq!(
        value,
        serde_json::json!({"target": "人物/林动.md#身体#伤势"})
    );
}

#[test]
fn resolve_ref_and_backlinks_use_canonical_heading_path_for_equivalent_selector() {
    let (dir, queries) = fixture();
    fs::write(
        dir.path().join("Target.md"),
        "# Target\n\n## Parent\n\n### Child\n\nContent\n",
    )
    .expect("write target");
    fs::write(dir.path().join("Source.md"), "[[Target#Parent/Child]]\n").expect("write source");

    let resolved = queries
        .resolve_ref("[[Target#Parent/Child]]")
        .expect("resolve equivalent selector");
    assert_eq!(
        serde_json::to_value(resolved).expect("resolve json"),
        serde_json::json!({"target": "Target.md#Parent#Child"})
    );

    let backlinks = queries
        .get_backlinks("Target#Parent", &[], &[], 1)
        .expect("parent backlinks");
    assert_eq!(
        serde_json::to_value(backlinks).expect("backlinks json"),
        serde_json::json!({
            "scope": "Target.md#Parent",
            "references": [{
                "target": "Target.md#Parent#Child",
                "sources": ["Source.md#L1"]
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
fn resolve_ref_outputs_compact_ambiguous_json_with_selector() {
    let (_dir, queries) = fixture();

    let ambiguous = queries.resolve_ref("[[发动机#原理]]").expect("resolve");
    let value = serde_json::to_value(ambiguous).expect("resolve json");

    assert_eq!(
        value,
        serde_json::json!({
            "ambiguous_targets": ["发动机.md#原理", "资料/发动机.md#原理"]
        })
    );
}

#[test]
fn resolve_ref_outputs_compact_unresolved_json_with_unique_suggestion() {
    let (dir, queries) = fixture();
    fs::create_dir_all(dir.path().join("人物")).expect("create characters dir");
    fs::write(
        dir.path().join("人物/唯一笔记.md"),
        "# 唯一笔记\n\n## 身体\n",
    )
    .expect("write character");

    let unresolved = queries
        .resolve_ref("[[错误目录/唯一笔记#身体]]")
        .expect("resolve");
    let value = serde_json::to_value(unresolved).expect("resolve json");

    assert_eq!(
        value,
        serde_json::json!({
            "unresolved_target": "错误目录/唯一笔记#身体",
            "suggested_target": "人物/唯一笔记.md#身体"
        })
    );
}

#[test]
fn resolve_ref_outputs_compact_unresolved_json_without_suggestion() {
    let (_dir, queries) = fixture();

    let unresolved = queries.resolve_ref("[[不存在的人物]]").expect("resolve");
    let value = serde_json::to_value(unresolved).expect("resolve json");

    assert_eq!(
        value,
        serde_json::json!({"unresolved_target": "不存在的人物"})
    );
}

#[test]
fn resolve_ref_outputs_unresolved_for_missing_selectors_on_existing_note() {
    let (dir, queries) = fixture();
    fs::write(
        dir.path().join("Target.md"),
        "# Target\n\n## Present\n\n^present\n",
    )
    .expect("write target");

    let missing_heading = queries
        .resolve_ref("[[Target#Missing]]")
        .expect("resolve missing heading");
    let missing_block = queries
        .resolve_ref("[[Target#^missing]]")
        .expect("resolve missing block");

    assert_eq!(
        serde_json::to_value(missing_heading).expect("heading json"),
        serde_json::json!({"unresolved_target": "Target#Missing"})
    );
    assert_eq!(
        serde_json::to_value(missing_block).expect("block json"),
        serde_json::json!({"unresolved_target": "Target#^missing"})
    );
}

#[test]
fn outlinks_page_then_group_targets_and_keep_ambiguous_selectors() {
    let (dir, queries) = fixture();
    fs::create_dir_all(dir.path().join("A")).expect("create A dir");
    fs::create_dir_all(dir.path().join("B")).expect("create B dir");
    fs::write(dir.path().join("Target.md"), "# Target\n").expect("write target");
    fs::write(dir.path().join("A/Dup.md"), "# Dup\n").expect("write dup A");
    fs::write(dir.path().join("B/Dup.md"), "# Dup\n").expect("write dup B");
    let mut content = String::from("# Links\n\n[[Missing]]\n[[Dup#Detail]]\n");
    for index in 1..=49 {
        content.push_str(&format!("[[Target|target {index}]]\n"));
    }
    fs::write(dir.path().join("Links.md"), content).expect("write links");

    let first = queries.get_outlinks("Links", 1).expect("outlinks page 1");
    let first_value = serde_json::to_value(first).expect("outlinks json");
    let first_page_targets = (0..48)
        .map(|offset| {
            serde_json::json!({
                "source": format!("Links.md#L{}", 5 + offset),
                "target": "Target.md"
            })
        })
        .collect::<Vec<_>>();

    assert_eq!(
        first_value,
        serde_json::json!({
            "note": "Links.md",
            "targets": first_page_targets,
            "ambiguous_targets": [{
                "source": "Links.md#L4",
                "reference": "Dup#Detail",
                "candidates": ["A/Dup.md#Detail", "B/Dup.md#Detail"]
            }],
            "unresolved_targets": [{
                "source": "Links.md#L3",
                "reference": "Missing"
            }],
            "pagination": {
                "page": 1,
                "total_pages": 2,
                "total_links": 51
            }
        })
    );

    let second = queries.get_outlinks("Links", 2).expect("outlinks page 2");
    let second_value = serde_json::to_value(second).expect("outlinks json");
    assert_eq!(
        second_value,
        serde_json::json!({
            "note": "Links.md",
            "targets": [{
                "source": "Links.md#L53",
                "target": "Target.md"
            }],
            "pagination": {
                "page": 2,
                "total_pages": 2,
                "total_links": 51
            }
        })
    );
}

#[test]
fn outlinks_reports_missing_selectors_on_existing_note_as_unresolved() {
    let (dir, queries) = fixture();
    fs::write(
        dir.path().join("Target.md"),
        "# Target\n\n## Present\n\n^present\n",
    )
    .expect("write target");
    fs::write(
        dir.path().join("Links.md"),
        "# Links\n\n[[Target#Missing]]\n[[Target#^missing]]\n",
    )
    .expect("write links");

    let outlinks = queries.get_outlinks("Links", 1).expect("outlinks");
    let value = serde_json::to_value(outlinks).expect("outlinks json");

    assert_eq!(
        value,
        serde_json::json!({
            "note": "Links.md",
            "targets": [],
            "unresolved_targets": [
                {"source": "Links.md#L3", "reference": "Target#Missing"},
                {"source": "Links.md#L4", "reference": "Target#^missing"}
            ],
            "pagination": {
                "page": 1,
                "total_pages": 1,
                "total_links": 2
            }
        })
    );
}

#[test]
fn backlinks_page_after_filters_then_group_by_resolved_target_scope() {
    let (dir, queries) = fixture();
    fs::create_dir_all(dir.path().join("keep")).expect("create keep dir");
    fs::create_dir_all(dir.path().join("drop")).expect("create drop dir");
    fs::write(
        dir.path().join("Target.md"),
        "# Target\n\n## Parent\n\n### Child\n\n## Sibling\n\n^exact\n",
    )
    .expect("write target");
    fs::write(dir.path().join("keep/whole.md"), "[[Target]]\n").expect("write whole");
    fs::write(dir.path().join("keep/parent.md"), "[[Target#Parent]]\n").expect("write parent");
    fs::write(
        dir.path().join("keep/child.md"),
        "[[Target#Parent#Child]] and [[Target#Parent#Child]]\n",
    )
    .expect("write child");
    fs::write(dir.path().join("keep/sibling.md"), "[[Target#Sibling]]\n").expect("write sibling");
    fs::write(dir.path().join("keep/block.md"), "[[Target#^exact]]\n").expect("write block");
    fs::write(
        dir.path().join("keep/missing-selectors.md"),
        "[[Target#Missing]]\n[[Target#^missing]]\n",
    )
    .expect("write missing selectors");
    fs::write(
        dir.path().join("drop/excluded.md"),
        "[[Target#Parent#Child]]\n",
    )
    .expect("write excluded");

    let whole = queries
        .get_backlinks(
            "Target",
            &["keep/**/*.md".to_string()],
            &["keep/sibling.md".to_string()],
            1,
        )
        .expect("whole-note backlinks");
    let whole_value = serde_json::to_value(whole).expect("backlinks json");
    assert_eq!(
        whole_value,
        serde_json::json!({
            "scope": "Target.md",
            "references": [
                {"target": "Target.md#^exact", "sources": ["keep/block.md#L1"]},
                {"target": "Target.md#Parent#Child", "sources": ["keep/child.md#L1"]},
                {"target": "Target.md#Parent", "sources": ["keep/parent.md#L1"]},
                {"target": "Target.md", "sources": ["keep/whole.md#L1"]}
            ],
            "pagination": {
                "page": 1,
                "total_pages": 1,
                "total_backlinks": 4
            }
        })
    );

    let heading = queries
        .get_backlinks("Target#Parent", &["keep/**/*.md".to_string()], &[], 1)
        .expect("heading backlinks");
    let heading_value = serde_json::to_value(heading).expect("backlinks json");
    assert_eq!(
        heading_value,
        serde_json::json!({
            "scope": "Target.md#Parent",
            "references": [
                {"target": "Target.md#Parent#Child", "sources": ["keep/child.md#L1"]},
                {"target": "Target.md#Parent", "sources": ["keep/parent.md#L1"]}
            ],
            "pagination": {
                "page": 1,
                "total_pages": 1,
                "total_backlinks": 2
            }
        })
    );

    let block = queries
        .get_backlinks("Target#^exact", &["keep/**/*.md".to_string()], &[], 1)
        .expect("block backlinks");
    let block_value = serde_json::to_value(block).expect("backlinks json");
    assert_eq!(
        block_value,
        serde_json::json!({
            "scope": "Target.md#^exact",
            "references": [
                {"target": "Target.md#^exact", "sources": ["keep/block.md#L1"]}
            ],
            "pagination": {
                "page": 1,
                "total_pages": 1,
                "total_backlinks": 1
            }
        })
    );

    let missing_heading = queries.get_backlinks("Target#Missing", &[], &[], 1);
    assert!(
        missing_heading
            .unwrap_err()
            .to_string()
            .contains("target must resolve to exactly one note reference")
    );
    let missing_block = queries.get_backlinks("Target#^missing", &[], &[], 1);
    assert!(
        missing_block
            .unwrap_err()
            .to_string()
            .contains("target must resolve to exactly one note reference")
    );
}

#[test]
fn mcp_output_schemas_have_object_roots() {
    for schema in [
        serde_json::to_value(schemars::schema_for!(OutlinksResult)).expect("outlinks schema"),
        serde_json::to_value(schemars::schema_for!(BacklinksResult)).expect("backlinks schema"),
        serde_json::to_value(schemars::schema_for!(ListTagsResult)).expect("list tags schema"),
        serde_json::to_value(schemars::schema_for!(GetTagResult)).expect("get tag schema"),
        serde_json::to_value(schemars::schema_for!(ListCategoriesResult))
            .expect("list categories schema"),
        serde_json::to_value(schemars::schema_for!(GetCategoryResult))
            .expect("get category schema"),
    ] {
        assert_eq!(schema["type"], "object");
    }
}

#[test]
fn read_note_uses_its_own_character_budget_and_directs_to_section_reads() {
    let (_dir, mut queries) = fixture();
    queries.vault.config.max_read_note_chars = "# 发动机\n".chars().count();

    assert_eq!(DEFAULT_MAX_READ_NOTE_CHARS, 4 * 1024);

    let result = queries
        .read_note("发动机.md", None, None)
        .expect("read note");

    assert_eq!(result.content, "# 发动机\n");
    assert!(result.truncated);
    let value = serde_json::to_value(result).expect("read json");
    assert_eq!(value["source"], "发动机.md#L1-L5");
    assert!(value.get("path").is_none());
    assert!(value.get("next_step").is_none());
}

#[test]
fn read_note_omits_truncated_field_when_full_content_is_returned() {
    let (_dir, queries) = fixture();

    let result = queries
        .read_note("发动机.md", Some(usize::MAX), None)
        .expect("read full note");
    let value = serde_json::to_value(result).expect("read json");

    assert_eq!(value["source"], "发动机.md#L1-L5");
    assert!(value.get("path").is_none());
    assert!(value.get("next_step").is_none());
    assert!(value.get("truncated").is_none());
    assert!(
        value["content"]
            .as_str()
            .expect("content")
            .contains("## 原理")
    );
}

#[test]
fn read_note_allows_per_request_max_chars_override() {
    let (_dir, mut queries) = fixture();
    queries.vault.config.max_read_note_chars = "# 发动机\n".chars().count();

    let result = queries
        .read_note(
            "发动机.md",
            Some("# 发动机\n\n## 原理\n".chars().count()),
            None,
        )
        .expect("read note with override");

    assert_eq!(result.content, "# 发动机\n\n## 原理\n");
    assert!(result.truncated);
}

#[test]
fn read_note_counts_unicode_characters_not_utf8_bytes() {
    let (dir, mut queries) = fixture();
    fs::write(dir.path().join("字符.md"), "甲乙丙丁").expect("write unicode note");
    queries.vault.config.max_read_note_chars = 2;

    let result = queries.read_note("字符.md", None, None).expect("read note");

    assert_eq!(result.content, "甲乙");
    assert!(result.truncated);
}

#[test]
fn read_note_by_heading_returns_heading_scope() {
    let (_dir, queries) = fixture();
    let result = queries
        .read_note(
            "发动机.md",
            None,
            Some(SectionSelector::Heading {
                heading: "原理".to_string(),
            }),
        )
        .expect("read section");
    assert_eq!(result.source, "发动机.md#L3-L5");
    assert!(result.content.contains("链接到 [[林动]]"));
}

#[test]
fn note_outline_returns_heading_tree() {
    let (_dir, queries) = fixture();
    let result = queries.get_note_outline("发动机.md", 1).expect("outline");
    assert_eq!(result.note, "发动机.md");
    let value = serde_json::to_value(&result).expect("outline json");
    assert_eq!(
        value["headings"],
        serde_json::json!([{"heading": "原理", "level": 2, "line": 3}])
    );
    assert_eq!(
        value["pagination"],
        serde_json::json!({"page": 1, "total_pages": 1, "total_headings": 1})
    );
    assert!(value.get("outline").is_none());
}

#[test]
fn note_outline_flattens_non_h1_headings_in_document_order_with_slash_paths() {
    let (dir, queries) = fixture();
    fs::write(
        dir.path().join("多根.md"),
        "# 第一标题\n\n## A\n\n### A1\n\n# 第二标题\n\n## B\n\n### B1\n\n#### B2\n",
    )
    .expect("write multi-root note");

    let result = queries.get_note_outline("多根.md", 1).expect("outline");
    let value = serde_json::to_value(result).expect("outline json");

    assert_eq!(
        value["headings"],
        serde_json::json!([
            {"heading": "A", "level": 2, "line": 3},
            {"heading": "A/A1", "level": 3, "line": 5},
            {"heading": "B", "level": 2, "line": 9},
            {"heading": "B/B1", "level": 3, "line": 11},
            {"heading": "B/B1/B2", "level": 4, "line": 13}
        ])
    );
}

#[test]
fn note_outline_is_empty_when_note_only_has_h1_titles() {
    let (dir, queries) = fixture();
    fs::write(dir.path().join("只有标题.md"), "# 标题一\n\n# 标题二\n").expect("write h1 note");

    let result = queries.get_note_outline("只有标题.md", 1).expect("outline");

    let value = serde_json::to_value(result).expect("outline json");
    assert_eq!(value["headings"], serde_json::json!([]));
}

#[test]
fn note_outline_paginates_flat_headings_with_fixed_100_item_pages() {
    let (dir, queries) = fixture();
    let mut content = String::from("# Title\n");
    for index in 1..=101 {
        content.push_str(&format!("\n## Heading {index:03}\n"));
    }
    fs::write(dir.path().join("分页.md"), content).expect("write paged outline note");

    let page_1 = serde_json::to_value(
        queries
            .get_note_outline("分页.md", 1)
            .expect("outline page 1"),
    )
    .expect("page 1 json");
    assert_eq!(page_1["headings"].as_array().expect("headings").len(), 100);
    assert_eq!(page_1["headings"][0]["heading"], "Heading 001");
    assert_eq!(page_1["headings"][99]["heading"], "Heading 100");
    assert_eq!(
        page_1["pagination"],
        serde_json::json!({"page": 1, "total_pages": 2, "total_headings": 101})
    );

    let page_2 = serde_json::to_value(
        queries
            .get_note_outline("分页.md", 2)
            .expect("outline page 2"),
    )
    .expect("page 2 json");
    assert_eq!(
        page_2["headings"],
        serde_json::json!([{"heading": "Heading 101", "level": 2, "line": 203}])
    );
    assert_eq!(
        page_2["pagination"],
        serde_json::json!({"page": 2, "total_pages": 2, "total_headings": 101})
    );

    let error = queries
        .get_note_outline("分页.md", 3)
        .expect_err("out of range page");
    assert!(error.to_string().contains("page 3 out of range"));
}

#[test]
fn regex_search_supports_include_filters_and_sections() {
    let (_dir, queries) = fixture();
    let result = queries
        .search_regex(
            "林动.{0,20}代偿",
            false,
            &["正文/**/*.md".to_string()],
            &[],
            1,
        )
        .expect("regex search");
    assert_eq!(result.matches.len(), 1);
    assert_eq!(result.matches[0].source, "正文/001.md#L5");
    let value = serde_json::to_value(&result).expect("regex search json");
    assert_eq!(value["matches"][0]["source"], "正文/001.md#L5");
    assert!(value["matches"][0].get("section").is_none());
}
