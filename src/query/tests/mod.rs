use std::fs;

use camino::Utf8PathBuf;
use tempfile::tempdir;

use super::*;
use crate::vault::{DEFAULT_MAX_READ_NOTE_CHARS, Vault, VaultConfig, VaultError};

mod contract_primitives;
mod edge_cases;
mod files_and_context;
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
fn get_note_structure_uses_compact_shared_output() {
    let (_dir, queries) = fixture();
    let result = queries.get_note_structure("林动").expect("parse result");
    assert_eq!(result.path, "林动.md");
    assert_eq!(result.headings[0].text, "林动");
    assert_eq!(result.headings[0].line, 10);
    let link = result
        .links
        .iter()
        .find(|link| link.target == "发动机")
        .expect("发动机 link");
    assert_eq!(link.alias.as_deref(), Some("发动机"));
    assert_eq!(link.section.as_deref(), Some("林动"));

    let value = serde_json::to_value(result).expect("json");
    assert!(value["links"][0].get("source").is_none());
    assert!(value["links"][0].get("raw").is_none());
    assert!(value["headings"][0].get("source").is_none());
    assert!(value["headings"][0].get("anchor").is_none());
}

#[test]
fn get_note_stats_counts_words_characters_and_total_backlinks() {
    let (_dir, queries) = fixture();
    let content = "---\naliases:\n  - 动林\ntags:\n  - 主角\n  - 状态/身体\nphase: active\narc: 引擎线\n---\n# 林动\n\n身体 #状态/身体\n\n[[发动机#原理|发动机]]\n\n[[缺失设定]]\n";

    let result = queries
        .get_note_stats("林动", WordCountMode::Source)
        .expect("note stats");
    assert_eq!(result.note, "林动.md");
    assert_eq!(result.word_count_mode, WordCountMode::Source);
    assert_eq!(result.word_count, 36);
    assert_eq!(result.character_count, content.chars().count());
    assert_eq!(result.line_count, content.lines().count());
    assert_eq!(result.backlink_count, 1);
}

#[test]
fn get_note_stats_scopes_text_and_backlinks_to_ref() {
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
        .get_note_stats("Target#Section", WordCountMode::Source)
        .expect("heading stats");
    assert_eq!(heading.word_count, 4);
    assert_eq!(heading.line_count, 5);
    assert_eq!(heading.backlink_count, 1);
    assert_eq!(
        heading.source.as_ref().expect("heading source").line_start,
        3
    );

    let block = queries
        .get_note_stats("Target#^state", WordCountMode::Source)
        .expect("block stats");
    assert_eq!(block.word_count, 1);
    assert_eq!(block.line_count, 1);
    assert_eq!(block.backlink_count, 1);
    assert_eq!(block.source.as_ref().expect("block source").line_start, 7);

    let whole = queries
        .get_note_stats("Target", WordCountMode::Source)
        .expect("whole-note stats");
    assert_eq!(whole.backlink_count, 3);
    assert!(whole.source.is_none());
}

#[test]
fn get_note_stats_counts_blank_lines_without_extra_trailing_line() {
    let (dir, queries) = fixture();
    fs::write(dir.path().join("line-count.md"), "first\n\nthird\n").expect("write line count note");

    let result = queries
        .get_note_stats("line-count.md", WordCountMode::Source)
        .expect("note stats");

    assert_eq!(result.line_count, 3);
}

#[test]
fn get_note_stats_visible_mode_ignores_markdown_metadata_and_comments() {
    let (dir, queries) = fixture();
    std::fs::write(
        dir.path().join("visible.md"),
        "---\ntitle: Hidden Title\nalias: 隐藏\n---\n# Visible Title\n\nBody **strong** [[Target Note|Alias Text]] [Markdown Text](https://example.com).\n\n%% hidden obsidian comment %%\n<!-- hidden markdown comment -->\n",
    )
    .expect("write visible note");

    let source = queries
        .get_note_stats("visible.md", WordCountMode::Source)
        .expect("source stats");
    let visible = queries
        .get_note_stats("visible.md", WordCountMode::Visible)
        .expect("visible stats");

    assert_eq!(visible.word_count_mode, WordCountMode::Visible);
    assert_eq!(visible.word_count, 8);
    assert!(source.word_count > visible.word_count);
}

#[test]
fn get_note_stats_treats_hyphenated_ascii_sequences_as_one_word() {
    let (dir, queries) = fixture();
    std::fs::write(dir.path().join("hyphen.md"), "foo-bar baz\n").expect("write hyphen note");

    let result = queries
        .get_note_stats("hyphen.md", WordCountMode::Source)
        .expect("note stats");
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

    assert_eq!(result.path, "001-排序标题.md");
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
fn list_tags_returns_unique_tag_names() {
    let (_dir, queries) = fixture();
    let result = queries.list_tags(TagScope::Note, &[], &[]).expect("tags");
    assert!(result.tags.contains(&"主角".to_string()));
    assert!(result.tags.contains(&"状态/身体".to_string()));
    assert!(result.tags.contains(&"河流".to_string()));
    assert!(result.tags.contains(&"设定".to_string()));
}

#[test]
fn get_tags_returns_locations_and_verbose_sources() {
    let (_dir, queries) = fixture();
    let compact = queries
        .get_tags(&["状态/身体".to_string()], TagScope::Note, false, &[], &[])
        .expect("compact tags");
    assert_eq!(compact.tags.len(), 1);
    assert!(
        compact.tags[0]
            .notes
            .iter()
            .any(|note| note.note == "林动.md"
                && note.source_kind == TagSourceKind::Frontmatter
                && note.section.is_none())
    );
    assert!(compact.tags[0].occurrences.is_empty());

    let verbose = queries
        .get_tags(&["状态/身体".to_string()], TagScope::Note, true, &[], &[])
        .expect("verbose tags");
    assert!(verbose.tags[0].occurrences.iter().any(|occurrence| {
        occurrence.location == "林动.md#L12"
            && occurrence.section.as_ref().is_some_and(|section| {
                section.heading == "林动"
                    && section.heading_level == 1
                    && section.heading_path.is_none()
            })
    }));
    let verbose_value = serde_json::to_value(&verbose).expect("verbose json");
    assert!(
        verbose_value["tags"][0]["occurrences"][0]["section"]
            .get("heading_anchor")
            .is_none()
    );
    assert!(
        verbose.tags[0]
            .occurrences
            .iter()
            .all(|occurrence| !occurrence.location.is_empty())
    );
}

#[test]
fn tags_scope_filters_frontmatter_and_body_sources() {
    let (_dir, queries) = fixture();

    let frontmatter = queries
        .get_tags(
            &["状态/身体".to_string()],
            TagScope::Frontmatter,
            true,
            &[],
            &[],
        )
        .expect("frontmatter tags");
    assert_eq!(frontmatter.tags.len(), 1);
    assert!(
        frontmatter.tags[0]
            .occurrences
            .iter()
            .all(|occurrence| occurrence.source_kind == TagSourceKind::Frontmatter)
    );

    let body = queries
        .get_tags(&["状态/身体".to_string()], TagScope::Body, true, &[], &[])
        .expect("body tags");
    assert_eq!(body.tags.len(), 1);
    assert!(
        body.tags[0]
            .occurrences
            .iter()
            .all(|occurrence| occurrence.source_kind == TagSourceKind::Frontmatter)
    );

    let section = queries
        .get_tags(&["河流".to_string()], TagScope::Section, true, &[], &[])
        .expect("section tags");
    assert_eq!(section.tags.len(), 1);
    assert!(section.tags[0].occurrences.iter().any(|occurrence| {
        occurrence.source_kind == TagSourceKind::Note && occurrence.location == "河流.md#L3"
    }));

    let line = queries
        .get_tags(&["设定".to_string()], TagScope::Line, true, &[], &[])
        .expect("line tags");
    assert_eq!(line.tags.len(), 1);
    assert!(line.tags[0].occurrences.iter().any(|occurrence| {
        occurrence.source_kind == TagSourceKind::Line && occurrence.location == "河流.md#L4"
    }));
}

#[test]
fn get_tags_classifies_tags_under_h1_as_note_and_h2_as_section() {
    let (dir, queries) = fixture();
    fs::write(
        dir.path().join("层级标签.md"),
        "# Note title\n\nH1 content #h1-tag\n\n## Section title\n\nH2 content #h2-tag\n",
    )
    .expect("write hierarchy tag note");

    let h1 = queries
        .get_tags(&["h1-tag".to_string()], TagScope::Note, false, &[], &[])
        .expect("h1 tag");
    assert_eq!(
        serde_json::to_value(&h1).expect("serialize h1 tag")["tags"][0]["notes"][0]["source_kind"],
        "note"
    );

    let h2 = queries
        .get_tags(&["h2-tag".to_string()], TagScope::Note, false, &[], &[])
        .expect("h2 tag");
    assert_eq!(
        serde_json::to_value(&h2).expect("serialize h2 tag")["tags"][0]["notes"][0]["source_kind"],
        "section"
    );
}

#[test]
fn get_tags_uses_the_shortest_heading_selector_that_distinguishes_duplicates() {
    let (dir, queries) = fixture();
    fs::write(
        dir.path().join("标签章节.md"),
        "# Note title\n\nNote tag #scope\n\n## Unique\n\nUnique tag #scope\n\n## Parent A\n\n### Detail\n\nA tag #scope\n\n## Parent B\n\n### Detail\n\nB tag #scope\n",
    )
    .expect("write tagged sections");

    let result = queries
        .get_tags(&["scope".to_string()], TagScope::Note, false, &[], &[])
        .expect("tags");
    let matches = &result.tags[0].notes;

    assert!(
        matches
            .iter()
            .any(|tag| tag.note == "标签章节.md#L3" && tag.section.is_none())
    );
    assert!(
        matches
            .iter()
            .any(|tag| tag.note == "标签章节.md#L7" && tag.section.as_deref() == Some("Unique"))
    );
    assert!(matches.iter().any(|tag| {
        tag.note == "标签章节.md#L13" && tag.section.as_deref() == Some("Parent A/Detail")
    }));
    assert!(matches.iter().any(|tag| {
        tag.note == "标签章节.md#L19" && tag.section.as_deref() == Some("Parent B/Detail")
    }));
}

#[test]
fn get_tags_omits_unneeded_ancestors_from_duplicate_heading_selectors() {
    let (dir, queries) = fixture();
    fs::write(
        dir.path().join("最短路径.md"),
        "# Note title\n\n## Root A\n\n### Branch A\n\n#### Detail\n\nA tag #scope\n\n## Root B\n\n### Branch B\n\n#### Detail\n\nB tag #scope\n",
    )
    .expect("write nested duplicate headings");

    let result = queries
        .get_tags(&["scope".to_string()], TagScope::Note, false, &[], &[])
        .expect("tags");
    let matches = &result.tags[0].notes;

    assert!(matches.iter().any(|tag| {
        tag.note == "最短路径.md#L9" && tag.section.as_deref() == Some("Branch A/Detail")
    }));
    assert!(matches.iter().any(|tag| {
        tag.note == "最短路径.md#L17" && tag.section.as_deref() == Some("Branch B/Detail")
    }));
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
fn list_categories_returns_unique_folder_names() {
    let (_dir, queries) = fixture();
    let result = queries.list_categories(&[], &[]).expect("categories");
    assert!(result.categories.contains(&"正文".to_string()));
    assert!(result.categories.contains(&"资料".to_string()));
    assert!(!result.categories.contains(&".obsidian".to_string()));
    assert!(!result.categories.contains(&"ignored-dir".to_string()));
}

#[test]
fn get_categories_returns_matching_note_files() {
    let (_dir, queries) = fixture();
    let result = queries
        .get_categories(&["正文".to_string()], &[], &[])
        .expect("category files");
    assert_eq!(result.categories.len(), 1);
    assert_eq!(result.categories[0].category, "正文");
    assert!(
        result.categories[0]
            .files
            .contains(&"正文/README.md".to_string())
    );
    assert!(
        result.categories[0]
            .files
            .contains(&"正文/001.md".to_string())
    );
    assert!(
        !result.categories[0]
            .files
            .contains(&"ignored-dir/ignored.md".to_string())
    );
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
        .list_tags(TagScope::Note, &include, &exclude)
        .expect("list filtered tags");
    assert_eq!(tags.tags, vec!["筛选标签".to_string()]);

    let selected_tags = queries
        .get_tags(
            &["筛选标签".to_string()],
            TagScope::Note,
            false,
            &include,
            &exclude,
        )
        .expect("get filtered tags");
    assert_eq!(selected_tags.tags[0].notes.len(), 2);
    assert!(
        selected_tags.tags[0]
            .notes
            .iter()
            .all(|tag| tag.note != "正文/草稿/drop.md")
    );

    let categories = queries
        .list_categories(&include, &exclude)
        .expect("list filtered categories");
    assert_eq!(
        categories.categories,
        vec!["正文".to_string(), "资料".to_string()]
    );

    let selected_categories = queries
        .get_categories(&["正文".to_string()], &include, &exclude)
        .expect("get filtered categories");
    assert!(
        selected_categories.categories[0]
            .files
            .contains(&"正文/keep.md".to_string())
    );
    assert!(
        !selected_categories.categories[0]
            .files
            .contains(&"正文/草稿/drop.md".to_string())
    );

    let text = queries
        .search_text("shared filtered content", false, 0, &include, &exclude)
        .expect("filtered text search");
    assert_eq!(text.matches.len(), 2);
    assert!(
        text.matches
            .iter()
            .all(|matched| matched.source.path != "正文/草稿/drop.md")
    );

    let regex = queries
        .search_regex("shared filtered content", false, 0, &include, &exclude)
        .expect("filtered regex search");
    assert_eq!(regex.matches.len(), 2);
    assert!(
        regex
            .matches
            .iter()
            .all(|matched| matched.source.path != "正文/草稿/drop.md")
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
            0,
            &["资料/**/*.md".to_string()],
            &[],
        )
        .expect("request filter cannot restore globally excluded note");
    assert!(globally_hidden.matches.is_empty());
}

#[test]
fn frontmatter_query_supports_exists_equals_and_regex_modes() {
    let (_dir, queries) = fixture();
    let exists = queries
        .query_frontmatter(FrontmatterQueryOptions {
            field: "phase".to_string(),
            mode: FrontmatterMatchMode::Exists,
            value: None,
        })
        .expect("exists query");
    assert_eq!(exists.matches[0].note, "林动.md");

    let equals = queries
        .query_frontmatter(FrontmatterQueryOptions {
            field: "phase".to_string(),
            mode: FrontmatterMatchMode::Equals,
            value: Some("active".to_string()),
        })
        .expect("equals query");
    assert_eq!(equals.matches.len(), 1);

    let regex = queries
        .query_frontmatter(FrontmatterQueryOptions {
            field: "arc".to_string(),
            mode: FrontmatterMatchMode::Regex,
            value: Some("引擎.*".to_string()),
        })
        .expect("regex query");
    assert_eq!(regex.matches[0].note, "林动.md");
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
        serde_json::to_value(schemars::schema_for!(GetTagsResult)).expect("get tags schema"),
        serde_json::to_value(schemars::schema_for!(ListCategoriesResult))
            .expect("list categories schema"),
        serde_json::to_value(schemars::schema_for!(GetCategoriesResult))
            .expect("get categories schema"),
    ] {
        assert_eq!(schema["type"], "object");
    }
}

#[test]
fn collect_note_context_returns_navigation_items_without_content() {
    let (_dir, queries) = fixture();

    let result = queries.collect_note_context("林动.md").expect("context");

    let current = &result.groups[0].items[0];
    assert_eq!(current.path, "林动.md");
    assert_eq!(current.title.as_deref(), Some("林动"));
    let value = serde_json::to_value(&result).expect("context json");
    assert!(value["groups"][0]["items"][0].get("content").is_none());
    assert!(value["groups"][0]["items"][0].get("source").is_none());
}

#[test]
fn collect_reference_context_returns_navigation_items_without_content() {
    let (_dir, queries) = fixture();

    let result = queries
        .collect_reference_context("[[林动]]")
        .expect("reference context");

    assert_eq!(result.groups[0].items[0].path, "林动.md");
    let value = serde_json::to_value(&result).expect("context json");
    assert!(value["groups"][0]["items"][0].get("content").is_none());
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
    assert_eq!(
        result.next_step.as_deref(),
        Some(
            "Retry read_note with a bare heading, block, or line reference for targeted access. If you still need more content, retry read_note with a larger max_chars value."
        )
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
    assert_eq!(result.source.section.unwrap().heading_path, vec!["原理"]);
    assert!(result.content.contains("链接到 [[林动]]"));
}

#[test]
fn note_outline_returns_heading_tree() {
    let (_dir, queries) = fixture();
    let result = queries
        .get_note_outline("发动机.md", None)
        .expect("outline");
    assert_eq!(result.note, "发动机.md");
    assert_eq!(result.outline.len(), 1);
    assert_eq!(result.outline[0].heading, "原理");
    assert_eq!(result.outline[0].heading_path, vec!["原理".to_string()]);
    let value = serde_json::to_value(&result).expect("outline json");
    assert!(value["outline"][0].get("heading_anchor").is_none());
    assert!(
        value["outline"][0]["source"]["section"]
            .get("heading_anchor")
            .is_none()
    );
}

#[test]
fn note_outline_skips_h1_and_resets_tree_after_each_h1() {
    let (dir, queries) = fixture();
    fs::write(
        dir.path().join("多根.md"),
        "# 第一标题\n\n## A\n\n### A1\n\n# 第二标题\n\n## B\n\n### B1\n\n#### B2\n",
    )
    .expect("write multi-root note");

    let result = queries.get_note_outline("多根.md", None).expect("outline");

    assert_eq!(result.outline.len(), 2);
    assert_eq!(result.outline[0].heading, "A");
    assert_eq!(result.outline[0].heading_path, vec!["A"]);
    assert_eq!(result.outline[0].children[0].heading, "A1");
    assert_eq!(result.outline[0].children[0].heading_path, vec!["A", "A1"]);
    assert_eq!(result.outline[1].heading, "B");
    assert_eq!(result.outline[1].heading_path, vec!["B"]);
    assert_eq!(result.outline[1].children[0].heading, "B1");
    assert_eq!(
        result.outline[1].children[0].children[0].heading_path,
        vec!["B", "B1", "B2"]
    );
}

#[test]
fn note_outline_is_empty_when_note_only_has_h1_titles() {
    let (dir, queries) = fixture();
    fs::write(dir.path().join("只有标题.md"), "# 标题一\n\n# 标题二\n").expect("write h1 note");

    let result = queries
        .get_note_outline("只有标题.md", None)
        .expect("outline");

    assert!(result.outline.is_empty());
}

#[test]
fn note_outline_can_return_the_ancestor_chain_for_a_heading_path() {
    let (dir, queries) = fixture();
    fs::write(
        dir.path().join("大纲链路.md"),
        "# Note title\n\n## Parent\n\n### Sibling\n\n#### Target\n\nBody\n\n### Other\n",
    )
    .expect("write outline chain note");

    let result = queries
        .get_note_outline("大纲链路.md", Some("Parent/Sibling/Target"))
        .expect("outline chain");

    assert_eq!(result.outline.len(), 1);
    assert_eq!(result.outline[0].heading, "Parent");
    assert_eq!(result.outline[0].children.len(), 1);
    assert_eq!(result.outline[0].children[0].heading, "Sibling");
    assert_eq!(result.outline[0].children[0].children.len(), 1);
    assert_eq!(result.outline[0].children[0].children[0].heading, "Target");
}

#[test]
fn regex_search_supports_include_filters_and_sections() {
    let (_dir, queries) = fixture();
    let result = queries
        .search_regex(
            "林动.{0,20}代偿",
            false,
            1,
            &["正文/**/*.md".to_string()],
            &[],
        )
        .expect("regex search");
    assert_eq!(result.matches.len(), 1);
    assert_eq!(result.matches[0].source.path, "正文/001.md");
    let value = serde_json::to_value(&result).expect("regex search json");
    assert_eq!(value["matches"][0]["source"]["path"], "正文/001.md#L4-L6");
    assert_eq!(
        result.matches[0]
            .source
            .section
            .as_ref()
            .map(|section| section.heading_path.clone()),
        Some(vec!["代偿".to_string()])
    );
}

#[test]
fn unresolved_links_are_reported() {
    let (_dir, queries) = fixture();
    let result = queries.find_unresolved_links().expect("unresolved");
    assert_eq!(result.links.len(), 1);
    assert_eq!(result.links[0].target, "缺失设定");
}

#[test]
fn ambiguous_links_are_reported() {
    let (_dir, queries) = fixture();
    let result = queries.find_ambiguous_links().expect("ambiguous");
    assert_eq!(result.links.len(), 1);
    assert_eq!(result.links[0].target, "发动机");
}

#[test]
fn vault_graph_contains_nodes_and_edges() {
    let (_dir, queries) = fixture();
    let result = queries.get_vault_graph().expect("graph");
    assert!(result.nodes.iter().any(|node| node.path == "林动.md"));
    assert!(result.nodes.iter().any(|node| node.path == "发动机.md"));
    assert!(
        result
            .nodes
            .iter()
            .any(|node| node.path == "资料/发动机.md")
    );
    assert!(result.edges.iter().any(|edge| edge.from == "林动.md"));
    assert!(
        result
            .edges
            .iter()
            .any(|edge| edge.status == "ambiguous" && edge.target == "发动机")
    );
}

#[test]
fn graph_neighborhood_returns_resolved_neighbors_by_direction() {
    let (_dir, queries) = fixture();
    let result = queries
        .get_graph_neighborhood(GraphNeighborhoodOptions {
            target: "林动".to_string(),
            depth: 1,
            direction: GraphNeighborhoodDirection::Both,
            include_unresolved: false,
        })
        .expect("graph neighborhood");

    assert!(result.nodes.iter().any(|node| node.path == "林动.md"));
    assert!(result.nodes.iter().any(|node| node.path == "发动机.md"));
    assert!(result.edges.iter().any(|edge| {
        edge.from == "发动机.md" && edge.to == "林动.md" && edge.status == "resolved"
    }));
    assert!(
        !result
            .edges
            .iter()
            .any(|edge| edge.status == "unresolved" || edge.status == "ambiguous")
    );
}

#[test]
fn graph_neighborhood_can_include_dangling_edges_without_expanding_them() {
    let (_dir, queries) = fixture();
    let result = queries
        .get_graph_neighborhood(GraphNeighborhoodOptions {
            target: "林动".to_string(),
            depth: 1,
            direction: GraphNeighborhoodDirection::Out,
            include_unresolved: true,
        })
        .expect("graph neighborhood");

    assert_eq!(result.nodes.len(), 1);
    assert_eq!(result.nodes[0].path, "林动.md");
    assert!(
        result
            .edges
            .iter()
            .any(|edge| edge.status == "ambiguous" && edge.target == "发动机")
    );
    assert!(
        result
            .edges
            .iter()
            .any(|edge| edge.status == "unresolved" && edge.target == "缺失设定")
    );
}

#[test]
fn graph_neighborhood_traverses_safe_relative_markdown_links() {
    let (_dir, queries) = fixture();
    let result = queries
        .get_graph_neighborhood(GraphNeighborhoodOptions {
            target: "正文/001.md".to_string(),
            depth: 1,
            direction: GraphNeighborhoodDirection::Out,
            include_unresolved: false,
        })
        .expect("graph neighborhood");

    assert!(result.nodes.iter().any(|node| node.path == "正文/001.md"));
    assert!(result.nodes.iter().any(|node| node.path == "发动机.md"));
    assert!(result.edges.iter().any(|edge| {
        edge.from == "正文/001.md" && edge.to == "发动机.md" && edge.status == "resolved"
    }));
    assert!(
        !result
            .edges
            .iter()
            .any(|edge| edge.target == "../../outside.md")
    );
}

#[test]
fn list_vault_files_returns_flat_gitignore_aware_file_list() {
    let (_dir, queries) = fixture();
    let result = queries
        .list_vault_files(VaultFilesOptions::default())
        .expect("list vault files");
    assert_eq!(result.summary.notes, 6);
    assert_eq!(result.summary.attachments, 1);
    assert!(
        !result
            .files
            .iter()
            .any(|file| file.path.contains(".agents"))
    );
    assert!(result.files.iter().any(|file| {
        file.path == "正文/README.md" && file.title.as_deref() == Some("正文索引")
    }));
    assert!(result.files.iter().all(|file| file.path != "正文/场景"));
    assert!(
        !result
            .files
            .iter()
            .any(|file| file.path == "ignored-by-git.md")
    );
    assert!(
        !result
            .files
            .iter()
            .any(|file| file.path == "ignored-dir/ignored.md")
    );
    assert!(
        !result
            .files
            .iter()
            .any(|file| file.path == "资料/ignored-here.md")
    );
    assert_eq!(result.summary.attachments, 1);
    assert!(!result.files.iter().any(|file| file.path == "地图.png"));
    let value = serde_json::to_value(&result).expect("json");
    assert!(value.get("root").is_none());
    assert!(value.get("ignored").is_none());
    assert!(value["files"][0].get("size").is_some());
    assert!(value["files"][0].get("size_bytes").is_none());
    assert!(value["files"][0].get("modified_unix_ms").is_none());

    let with_attachments = queries
        .list_vault_files(VaultFilesOptions {
            include_attachments: true,
            ..VaultFilesOptions::default()
        })
        .expect("list vault files with attachments");
    assert!(
        with_attachments
            .files
            .iter()
            .any(|file| file.path == "地图.png")
    );
}
