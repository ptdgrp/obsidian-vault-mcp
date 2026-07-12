use std::fs;

use camino::Utf8PathBuf;
use tempfile::tempdir;

use super::*;
use crate::resolver::ResolveResult;
use crate::vault::{DEFAULT_MAX_READ_NOTE_CHARS, Vault, VaultConfig, VaultError};

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
    let notes = queries.list_notes().expect("list notes");
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
    assert_eq!(
        notes
            .notes
            .iter()
            .find(|note| note.path == "发动机.md")
            .expect("发动机 note")
            .size,
        format!("{} B", "# 发动机\n\n## 原理\n\n链接到 [[林动]]\n".len())
    );
    let value = serde_json::to_value(notes).expect("list notes json");
    assert!(value["notes"][0].get("preview").is_none());
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

    let notes = queries.list_notes().expect("list notes");
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

    let result = queries.read_note("排序标题", None).expect("read note");

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
    let result = queries.list_tags(TagScope::Note).expect("tags");
    assert!(result.tags.contains(&"主角".to_string()));
    assert!(result.tags.contains(&"状态/身体".to_string()));
    assert!(result.tags.contains(&"河流".to_string()));
    assert!(result.tags.contains(&"设定".to_string()));
}

#[test]
fn get_tags_returns_locations_and_verbose_sources() {
    let (_dir, queries) = fixture();
    let compact = queries
        .get_tags(&["状态/身体".to_string()], TagScope::Note, false)
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
        .get_tags(&["状态/身体".to_string()], TagScope::Note, true)
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
        .get_tags(&["状态/身体".to_string()], TagScope::Frontmatter, true)
        .expect("frontmatter tags");
    assert_eq!(frontmatter.tags.len(), 1);
    assert!(
        frontmatter.tags[0]
            .occurrences
            .iter()
            .all(|occurrence| occurrence.source_kind == TagSourceKind::Frontmatter)
    );

    let body = queries
        .get_tags(&["状态/身体".to_string()], TagScope::Body, true)
        .expect("body tags");
    assert_eq!(body.tags.len(), 1);
    assert!(
        body.tags[0]
            .occurrences
            .iter()
            .all(|occurrence| occurrence.source_kind == TagSourceKind::Frontmatter)
    );

    let section = queries
        .get_tags(&["河流".to_string()], TagScope::Section, true)
        .expect("section tags");
    assert_eq!(section.tags.len(), 1);
    assert!(section.tags[0].occurrences.iter().any(|occurrence| {
        occurrence.source_kind == TagSourceKind::Section && occurrence.location == "河流.md#L3"
    }));

    let line = queries
        .get_tags(&["设定".to_string()], TagScope::Line, true)
        .expect("line tags");
    assert_eq!(line.tags.len(), 1);
    assert!(line.tags[0].occurrences.iter().any(|occurrence| {
        occurrence.source_kind == TagSourceKind::Line && occurrence.location == "河流.md#L4"
    }));
}

#[test]
fn list_categories_returns_unique_folder_names() {
    let (_dir, queries) = fixture();
    let result = queries.list_categories().expect("categories");
    assert!(result.categories.contains(&"正文".to_string()));
    assert!(result.categories.contains(&"资料".to_string()));
    assert!(!result.categories.contains(&".obsidian".to_string()));
    assert!(!result.categories.contains(&"ignored-dir".to_string()));
}

#[test]
fn get_categories_returns_matching_note_files() {
    let (_dir, queries) = fixture();
    let result = queries
        .get_categories(&["正文".to_string()])
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
fn resolve_ref_supports_alias_and_heading() {
    let (_dir, queries) = fixture();
    let resolved = queries.resolve_ref("[[动林#身体]]").expect("resolve");
    match resolved {
        ResolveResult::Resolved { path, heading, .. } => {
            assert_eq!(path, "林动.md");
            assert_eq!(heading.as_deref(), Some("身体"));
        }
        other => panic!("expected resolved, got {other:?}"),
    }
}

#[test]
fn backlinks_include_source_section() {
    let (_dir, queries) = fixture();
    let result = queries.get_backlinks("林动").expect("backlinks");
    assert_eq!(result.backlinks.len(), 1);
    assert_eq!(result.backlinks[0].source.path, "发动机.md");
    assert_eq!(
        result.backlinks[0]
            .source
            .section
            .as_ref()
            .map(|section| section.heading_path.clone()),
        Some(vec!["原理".to_string()])
    );
}

#[test]
fn link_outputs_default_to_compact_and_support_verbose() {
    let (_dir, queries) = fixture();

    let compact = queries
        .get_backlinks_output("林动", false)
        .expect("compact backlinks");
    let compact_value = serde_json::to_value(compact).expect("compact json");
    assert_eq!(compact_value["backlinks"][0]["location"], "发动机.md#L5");
    assert_eq!(compact_value["backlinks"][0]["section"], "原理");
    assert!(compact_value["backlinks"][0].get("source").is_none());
    assert!(compact_value["backlinks"][0].get("snippet").is_none());

    let verbose = queries
        .get_backlinks_output("林动", true)
        .expect("verbose backlinks");
    let verbose_value = serde_json::to_value(verbose).expect("verbose json");
    assert_eq!(
        verbose_value["backlinks"][0]["source"]["path"],
        "发动机.md#L5"
    );
    assert!(
        verbose_value["backlinks"][0]["source"]
            .get("lines")
            .is_none()
    );
    assert!(
        verbose_value["backlinks"][0]["source"]
            .get("line_start")
            .is_none()
    );
    assert!(verbose_value["backlinks"][0].get("snippet").is_some());

    let outlinks = queries
        .get_outlinks_output("林动", false)
        .expect("compact outlinks");
    let outlinks_value = serde_json::to_value(outlinks).expect("outlinks json");
    assert_eq!(outlinks_value["links"][0]["location"], "林动.md#L14");
    assert!(outlinks_value["links"][0].get("source").is_none());
}

#[test]
fn mcp_output_schemas_have_object_roots() {
    for schema in [
        serde_json::to_value(schemars::schema_for!(OutlinksOutput)).expect("outlinks schema"),
        serde_json::to_value(schemars::schema_for!(BacklinksOutput)).expect("backlinks schema"),
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
fn truncation_preserves_utf8_boundaries() {
    let text = "林动abc";
    assert_eq!(truncate_utf8(text, 4), "林");
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

    let result = queries.read_note("发动机.md", None).expect("read note");

    assert_eq!(result.content, "# 发动机\n");
    assert!(result.truncated);
    assert_eq!(
        result.next_step.as_deref(),
        Some(
            "Use get_note_outline to discover structure, then read_section with a heading, line, or block selector for targeted access. If you still need a larger prefix, retry read_note with a larger max_chars value."
        )
    );
}

#[test]
fn read_note_allows_per_request_max_chars_override() {
    let (_dir, mut queries) = fixture();
    queries.vault.config.max_read_note_chars = "# 发动机\n".chars().count();

    let result = queries
        .read_note("发动机.md", Some("# 发动机\n\n## 原理\n".chars().count()))
        .expect("read note with override");

    assert_eq!(result.content, "# 发动机\n\n## 原理\n");
    assert!(result.truncated);
}

#[test]
fn read_note_counts_unicode_characters_not_utf8_bytes() {
    let (dir, mut queries) = fixture();
    fs::write(dir.path().join("字符.md"), "甲乙丙丁").expect("write unicode note");
    queries.vault.config.max_read_note_chars = 2;

    let result = queries.read_note("字符.md", None).expect("read note");

    assert_eq!(result.content, "甲乙");
    assert!(result.truncated);
}

#[test]
fn read_section_by_heading_returns_heading_scope() {
    let (_dir, queries) = fixture();
    let result = queries
        .read_section(
            "发动机.md",
            SectionSelector::Heading {
                heading: "原理".to_string(),
            },
        )
        .expect("read section");
    assert_eq!(result.source.section.unwrap().heading_path, vec!["原理"]);
    assert!(result.content.contains("链接到 [[林动]]"));
}

#[test]
fn note_outline_returns_heading_tree() {
    let (_dir, queries) = fixture();
    let result = queries.get_note_outline("发动机.md").expect("outline");
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

    let result = queries.get_note_outline("多根.md").expect("outline");

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

    let result = queries.get_note_outline("只有标题.md").expect("outline");

    assert!(result.outline.is_empty());
}

#[test]
fn regex_search_supports_path_glob_and_sections() {
    let (_dir, queries) = fixture();
    let result = queries
        .search_regex("林动.{0,20}代偿", false, 1, Some("正文/**/*.md"))
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
