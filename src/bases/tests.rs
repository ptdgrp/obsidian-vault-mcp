use super::*;
use crate::vault::{Vault, VaultConfig};
use camino::Utf8PathBuf;
use std::{fs, sync::Arc};

fn fixture() -> (tempfile::TempDir, VaultQueries) {
    let dir = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(dir.path().into()).unwrap();
    fs::create_dir(root.join(".obsidian")).unwrap();
    fs::create_dir(root.join("items")).unwrap();
    fs::write(
        root.join(".obsidian/types.json"),
        r#"{"types":{"due":"date"}}"#,
    )
    .unwrap();
    fs::write(root.join("items/A.md"),"---\nstatus: active\nprice: 12\nqty: 3\ndue: 2026-02-28\ntags: [project/sub]\nrelated: '[[items/B|B alias]]'\n---\n[[items/B]]\n[Relative](B.md#Heading)\n![Picture](../image.png)\n![External](https://example.com/x.png)\n`![Fake](fake.png)`\n```\n![Fake](fake.png)\n```\n").unwrap();
    fs::write(
        root.join("items/B.md"),
        "---\nstatus: done\nprice: 7\nqty: 5\n---\n# Heading\n",
    )
    .unwrap();
    fs::write(
        root.join("items/C.md"),
        "---\nstatus: active\nprice: 2\nqty: 10\n---\n",
    )
    .unwrap();
    fs::write(root.join("image.png"), [0, 159, 255]).unwrap();
    fs::write(
        root.join("view.base"),
        "views: [{type: table, name: Test}]\n",
    )
    .unwrap();
    let queries = VaultQueries::new(Arc::new(
        Vault::open(&root, VaultConfig::default()).unwrap(),
    ));
    (dir, queries)
}
fn read(
    queries: &VaultQueries,
    source: &str,
    budget: usize,
    page: usize,
) -> anyhow::Result<ReadBaseResult> {
    fs::write(queries.vault.root.join("view.base"), source).unwrap();
    queries.read_base(ReadBaseRequest {
        path: "view.base".into(),
        view: None,
        context_note: None,
        page,
        max_chars: Some(budget),
    })
}
#[test]
fn public_formula_families_and_js_edges() {
    let (_dir, queries) = fixture();
    let data = Dataset::new(&queries).unwrap();
    let row = data.exact("items/A.md").unwrap();
    let context = data.exact("view.base").unwrap();
    let formulas = BTreeMap::from([("total".into(), expression::parse("price * qty").unwrap())]);
    let cases = [
        ("2 + 3 * 4", "14"),
        ("if(true, 'ok', number('bad'))", "ok"),
        ("false && number('bad')", "false"),
        ("true || number('bad')", "true"),
        ("missing == null", "true"),
        ("missing.isEmpty()", "true"),
        ("note['price']", "12"),
        ("formula.total + 1", "37"),
        ("price.isType('number')", "true"),
        ("(0).isTruthy()", "false"),
        ("(123).toString()", "123"),
        ("'hello'.containsAll('h','e')", "true"),
        ("'hello'.containsAny('x','e')", "true"),
        ("'hello'.endsWith('lo')", "true"),
        ("'hello'.startsWith('he')", "true"),
        ("'HELLO'.lower()", "hello"),
        ("'a:b:c'.replace(':','-')", "a-b-c"),
        ("'a:b:c'.replace(/:/,'-')", "a-b:c"),
        ("'a:b:c'.replace(/:/g,'-')", "a-b-c"),
        (
            r"'John Smith'.replace(/(\w+) (\w+)/, '$2, $1')",
            "Smith, John",
        ),
        ("'ab'.repeat(3)", "ababab"),
        ("'hello'.reverse()", "olleh"),
        ("'hello'.slice(-2)", "lo"),
        ("'a,b,c'.split(',',2).join('-')", "a-b"),
        ("'abc'.split('').join('-')", "a-b-c"),
        ("'hELLO-world'.title()", "Hello-World"),
        ("' hi '.trim()", "hi"),
        ("'😀'.length", "2"),
        ("(-5).abs()", "5"),
        ("(2.1).ceil()", "3"),
        ("(2.9).floor()", "2"),
        ("(-1.5).round()", "-1"),
        ("(2.3333).round(2)", "2.33"),
        ("(2.5).toFixed(0)", "3"),
        ("(-2.5).toFixed(0)", "-3"),
        ("(1.25).toFixed(1)", "1.3"),
        ("(2.675).toFixed(2)", "2.67"),
        ("(1e21).toFixed(2)", "1e+21"),
        ("[1,2,3].containsAll(1,3)", "true"),
        ("[1,2,3].containsAny(0,3)", "true"),
        ("[1,2,3,4].filter(value > 2).join(',')", "3,4"),
        ("[1,2,3].map(value + index).join(',')", "1,3,5"),
        ("[1,2,3].reduce(acc + value,0)", "6"),
        (
            "[1,2].map([3,4].map(value+index).join('-')).join(',')",
            "3-5,3-5",
        ),
        ("[1,[2,3]].flat().join(',')", "1,2,3"),
        ("[1,2,3].reverse().slice(1).join(',')", "2,1"),
        ("[3,1,2].sort().unique().join(',')", "1,2,3"),
        ("[].isEmpty()", "true"),
        ("[1,2][1]", "2"),
        ("list('a').join(',')", "a"),
        ("min(3,1,2)", "1"),
        ("max(3,1,2)", "3"),
        ("number(true)", "1"),
        ("number('3.4')+1", "4.4"),
        ("number('0x10')", "16"),
        (
            "(date('2026-01-31')+'1 month').format('YYYY-MM-DD')",
            "2026-02-28",
        ),
        (
            "(date('2026-02-28')+duration('1d')*2).format('YYYY-MM-DD')",
            "2026-03-02",
        ),
        ("date('2026-02-28 12:34:56').time()", "12:34:56"),
        ("date('2026-02-28 12:34:56').date().hour", "0"),
        ("due.format('YYYY-MM-DD')", "2026-02-28"),
        ("today().hour", "0"),
        ("now().isType('date')", "true"),
        ("file.name", "A.md"),
        ("file.basename", "A"),
        ("file.folder", "items"),
        ("file.ext", "md"),
        ("file.hasTag('project')", "true"),
        ("file.hasProperty('price')", "true"),
        ("file.inFolder('items')", "true"),
        ("file.size > 0", "true"),
        ("file.hasLink('items/B.md')", "true"),
        ("file('items/B.md').basename", "B"),
        ("related == file('items/B.md')", "true"),
        ("link('items/B.md#Heading','Other') == related", "true"),
        ("file.links.contains(link('items/B.md'))", "true"),
        ("file.embeds.contains(link('image.png'))", "true"),
        ("file.embeds.length", "1"),
        ("file('image.png').backlinks.contains(file)", "true"),
        (
            "file('image.png').backlinks.map(value.asFile().path).join(',')",
            "items/A.md",
        ),
        ("file.tags.contains('project/sub')", "true"),
        ("link('items/A.md').linksTo(file('items/B.md'))", "true"),
        ("file.properties.keys().contains('price')", "true"),
        ("file.properties.values().contains(12)", "true"),
        ("{}.isEmpty()", "true"),
        ("/abc/i.matches('ABC')", "true"),
        (r"/(?<=a)b/.matches('ab')", "true"),
        ("[1,2,3].mean()", "2"),
        ("[1,2,3].sum()", "6"),
        ("[1,2,3,4].median()", "2.5"),
        ("icon('plus').isType('icon')", "true"),
        ("image(file).isType('image')", "true"),
        ("html('<b>x</b>').isType('html')", "true"),
        ("escapeHTML('<&>')", "&lt;&amp;&gt;"),
    ];
    for (source, expected) in cases {
        let mut eval = Eval::new(&data, row, context, Local::now(), &formulas);
        let actual = eval
            .eval(&expression::parse(source).unwrap())
            .unwrap_or_else(|e| panic!("{source}: {e:#}"))
            .text();
        assert_eq!(actual, expected, "{source}");
    }
}
#[test]
fn filters_formula_order_group_limit_and_summaries() {
    let (_dir, queries) = fixture();
    let source = r#"
filters:
  and: ['file.ext == "md"', 'file.inFolder("items")']
formulas:
  total: price * qty
  dependent: formula.total + 1
properties:
  formula.total: {displayName: Total}
summaries:
  Rounded: values.mean().round(1)
views:
  - type: table
    name: Active
    filters: {not: ['status == "done"', 'price < 1']}
    order: [file.name, status, formula.total]
    sort: [{property: formula.dependent, direction: DESC}]
    groupBy: {property: status, direction: ASC}
    groupOrder: [active, planned]
    limit: 2
    summaries: {formula.total: Sum, price: Rounded}
"#;
    let result = read(&queries, source, 100000, 1).unwrap();
    assert!(
        result
            .content
            .contains("| [A.md](<items/A.md>) | active | 36 |")
    );
    assert!(
        result
            .content
            .contains("| [C.md](<items/C.md>) | active | 20 |")
    );
    assert!(!result.content.contains("items/B.md"));
    assert!(result.content.contains("| Total | Sum | 56 |"));
    assert!(result.content.contains("| price | Rounded | 7 |"));
    assert!(result.content.contains("## status: planned"));
    let first = read(&queries, source, 0, 1).unwrap();
    assert!(first.content.contains("[A.md](<items/A.md>)"));
    assert!(!first.content.contains("items/C.md"));
    assert!(first.content.contains("| Total | Sum | 56 |"));
    assert_eq!(first.pagination.total_pages, 3);
    let second = read(&queries, source, 0, 2).unwrap();
    assert!(second.content.contains("[C.md](<items/C.md>)"));
    assert!(!second.content.contains("items/A.md"));
    assert!(second.content.contains("| Total | Sum | 56 |"));
    let third = read(&queries, source, 0, 3).unwrap();
    assert!(!third.content.contains("](<items/"));
    assert_eq!(third.pagination.page, 3);
    assert!(third.content.contains("No rows in this group"));
}
#[test]
fn all_file_types_ignores_and_lazy_body_reads() {
    let (_dir, queries) = fixture();
    fs::write(
        queries.vault.root.join("broken.md"),
        "---\n[bad YAML\n---\n",
    )
    .unwrap();
    fs::write(queries.vault.root.join(".gitignore"), "ignored.md\n").unwrap();
    fs::write(queries.vault.root.join("ignored.md"), "ignored").unwrap();
    let result = read(
        &queries,
        "views: [{type: table, order: [file.path,file.ext]}]",
        100000,
        1,
    )
    .unwrap();
    assert!(result.content.contains("| image.png | png |"));
    assert!(!result.content.contains("ignored.md"));
    assert!(result.content.contains("| broken.md | md |"));
    let error = read(
        &queries,
        "filters: 'file.path == \"broken.md\"'\nviews: [{type: table, order: [note.status]}]",
        10000,
        1,
    )
    .unwrap_err();
    assert!(error.to_string().contains("invalid YAML frontmatter"));
}
#[test]
fn unknown_syntax_cycles_and_invalid_calls_are_diagnostic_even_without_rows() {
    let (_dir, queries) = fixture();
    for (source, needle) in [
        (
            "filters: 'file.unknownFunction()'\nviews: [{type: table}]",
            "unsupported function",
        ),
        (
            "filters: 'file.ext == \"never\"'\nformulas: {a: 'formula.b', b: 'formula.a'}\nviews: [{type: table}]",
            "circular formula",
        ),
        (
            "filters: 'false'\nformulas: {a: 'missing.xyz()'}\nviews: [{type: table}]",
            "unsupported function",
        ),
        (
            "filters: {and: [], or: []}\nviews: [{type: table}]",
            "exactly one",
        ),
        (
            "views: [{type: table, sort: [{property: price, direction: sideways}]}]",
            "invalid Base YAML",
        ),
        (
            "views: [{type: table, pluginFilter: x}]",
            "unsupported Base view setting",
        ),
    ] {
        let error = read(&queries, source, 10000, 1).unwrap_err();
        assert!(format!("{error:#}").contains(needle), "{source}: {error:#}");
    }
}
#[test]
fn explicit_context_view_selection_and_markdown_escaping() {
    let (_dir, queries) = fixture();
    fs::write(
        queries.vault.root.join("items/context.md"),
        "---\nstatus: active\n---\n",
    )
    .unwrap();
    let source = r#"filters: 'file.path == "items/A.md"'
formulas:
  literal: '"a|b\n<em>x</em> `code`"'
  alias: 'link("items/B.md", "B|alias")'
  context: this.file
views:
  - {type: table, name: First, order: [this]}
  - {type: cards, name: Second, order: [formula.literal, formula.alias, formula.context], image: related}
"#;
    fs::write(queries.vault.root.join("view.base"), source).unwrap();
    let result = queries
        .read_base(ReadBaseRequest {
            path: "view.base".into(),
            view: Some("Second".into()),
            context_note: Some("items/context.md".into()),
            page: 1,
            max_chars: Some(10000),
        })
        .unwrap();
    assert!(result.content.starts_with("# Second\n"));
    assert!(
        result
            .content
            .contains("[items/context.md](<items/context.md>)")
    );
    assert!(
        result
            .content
            .contains("a&#124;b<br>&lt;em&gt;x&lt;/em&gt; \\`code\\`")
    );
    assert!(result.content.contains("[B&#124;alias](<items/B.md>)"));
    assert!(result.content.contains("related"));
    let result=read(&queries,"filters: 'file.ext == \"md\" && status == this.status'\nviews: [{type: list, order: [file.path]}]",100000,1).unwrap();
    assert!(result.content.contains("No rows on this page"));
}
#[test]
fn map_and_kanban_keep_semantic_fields() {
    let (_dir, queries) = fixture();
    fs::write(
        queries.vault.root.join("items/place.md"),
        "---\ncoordinates: [48.8, 2.3]\nicon: landmark\ncolor: red\n---\n",
    )
    .unwrap();
    let result=read(&queries,r#"filters: 'file.path == "items/place.md"'
views: [{type: map, name: Map, coordinates: note.coordinates, markerIcon: note.icon, markerColor: note.color, center: '[48, 2]', defaultZoom: 12, mapTiles: https://example.com/tiles}]
"#,100000,1).unwrap();
    assert!(result.content.contains("](<items/place.md>)"));
    assert!(result.content.contains("48.8, 2.3"));
    assert!(result.content.contains("landmark"));
    assert!(result.content.contains("Map center: 48, 2"));
    let result = read(
        &queries,
        "filters: false\nviews: [{type: kanban, groupBy: {property: status}, groupOrder: [planned], columnWidth: 200, hideEmptyColumns: true}]",
        10000,
        1,
    );
    assert!(
        result.is_err(),
        "boolean filter is invalid; filter expressions must be strings"
    );
}
#[test]
fn duration_aliases_dates_and_moment_tokens() {
    for unit in [
        "y", "year", "years", "M", "month", "months", "d", "day", "days", "w", "week", "weeks",
        "h", "hour", "hours", "m", "minute", "minutes", "s", "second", "seconds",
    ] {
        assert!(dates::duration(&format!("1 {unit}")).is_ok(), "{unit}");
    }
    let date = dates::parse_date("2026-02-28 12:34:56.123").unwrap();
    assert_eq!(
        dates::format(date, "dddd, MMMM Do YYYY [at] h:mm:ss A SSS").unwrap(),
        "Saturday, February 28th 2026 at 12:34:56 PM 123"
    );
    assert!(dates::parse_date("2026-02-30").is_err());
    assert!(dates::duration("1 nonsense").is_err());
}
#[test]
fn invalid_numbers_regex_flags_and_unicode_loss_never_become_null() {
    let (_dir, queries) = fixture();
    let data = Dataset::new(&queries).unwrap();
    let row = data.exact("items/A.md").unwrap();
    let formulas = BTreeMap::new();
    let mut eval = Eval::new(&data, row, row, Local::now(), &formulas);
    for source in [
        "number('not a number')",
        "'😀'.slice(0,1)",
        "(1).unknown()",
        "/abc/ii.matches('abc')",
        "/abc/z.matches('abc')",
    ] {
        let result = expression::parse(source).and_then(|expr| eval.eval(&expr));
        assert!(result.is_err(), "{source}");
    }
}

#[test]
fn nullable_known_methods_and_repeated_references() {
    let (_dir, queries) = fixture();
    let result=read(&queries,"filters: 'categories.containsAny(link(\"Places\"))'\nviews: [{type: map, coordinates: due}]",10000,1).unwrap();
    assert!(result.content.contains("No rows on this page"));
    let data = Dataset::new(&queries).unwrap();
    let row = data.exact("items/A.md").unwrap();
    let formulas = BTreeMap::new();
    let mut eval = Eval::new(&data, row, row, Local::now(), &formulas);
    assert_eq!(
        eval.eval(
            &expression::parse("file.links.filter(value == link('items/B.md')).length").unwrap()
        )
        .unwrap()
        .text(),
        "3"
    );
    assert_eq!(
        eval.eval(&expression::parse("file.links[0].toString()").unwrap())
            .unwrap()
            .text(),
        "[[items/B|B alias]]"
    );
    assert_eq!(
        eval.eval(&expression::parse("missing.map(value.unknownProperty).isEmpty()").unwrap())
            .unwrap()
            .text(),
        "true"
    );
}

#[test]
fn expression_and_formula_depth_are_bounded_before_recursive_validation() {
    let expression = format!("1{}", ".abs()".repeat(12000));
    assert!(format!("{:#}", expression::parse(&expression).unwrap_err()).contains("depth exceeds"));
    let mut formulas = BTreeMap::new();
    for i in 0..300 {
        formulas.insert(
            format!("a{i:04}"),
            expression::parse(&format!("formula.a{:04}", i + 1)).unwrap(),
        );
    }
    formulas.insert("a0300".into(), expression::parse("1").unwrap());
    assert!(
        validate_dependencies(&formulas)
            .unwrap_err()
            .to_string()
            .contains("depth exceeds")
    );
}

#[test]
fn insertion_order_and_builtin_summary_types() {
    let (_dir, queries) = fixture();
    let data = Dataset::new(&queries).unwrap();
    let row = data.exact("items/A.md").unwrap();
    let formulas = BTreeMap::new();
    let mut eval = Eval::new(&data, row, row, Local::now(), &formulas);
    assert_eq!(
        eval.eval(&expression::parse("{z: 1, a: 2}.keys().join(',')").unwrap())
            .unwrap()
            .text(),
        "z,a"
    );
    let values = vec![Value::Number(1.), Value::Number(3.), Value::Null];
    for (name, expected) in [
        ("Average", "2"),
        ("Min", "1"),
        ("Max", "3"),
        ("Sum", "4"),
        ("Range", "2"),
        ("Median", "2"),
        ("Stddev", "1"),
        ("Empty", "1"),
        ("Filled", "2"),
        ("Unique", "3"),
    ] {
        assert_eq!(
            default_summary(name, &values).unwrap().text(),
            expected,
            "{name}"
        );
    }
    let dates = vec![
        dates::value("2026-01-01").unwrap(),
        dates::value("2026-01-03").unwrap(),
    ];
    assert_eq!(
        default_summary("Earliest", &dates).unwrap().text(),
        "2026-01-01"
    );
    assert_eq!(
        default_summary("Latest", &dates).unwrap().text(),
        "2026-01-03"
    );
    assert_eq!(
        default_summary("Range", &dates).unwrap().text(),
        "172800000"
    );
}

#[test]
fn view_discovery_is_configuration_only_and_preserves_names() {
    let (_dir, queries) = fixture();
    fs::write(queries.vault.root.join("view.base"), "formulas: {broken: 'unknown()'}\nfilters: 'not a valid expression'\nviews:\n  - {type: third-party}\n  - {type: table, name: Same}\n  - {type: list, name: Same}\n").unwrap();
    let result = queries
        .query_base_views(QueryBaseViewsRequest {
            path: "view.base".into(),
        })
        .unwrap();
    assert_eq!(result.path, "view.base");
    assert_eq!(result.views.len(), 3);
    assert_eq!(result.views[0].name, None);
    assert_eq!(result.views[0].kind, "third-party");
    assert!(result.views[0].is_default);
    assert_eq!(result.views[1].position, 2);
    assert_eq!(result.views[1].name, result.views[2].name);
    assert!(!result.views[1].is_default);
    for source in [
        "[]",
        "views: wrong",
        "views: [{name: missing-type}]",
        "views: []\nviews: []",
    ] {
        fs::write(queries.vault.root.join("view.base"), source).unwrap();
        assert!(
            queries
                .query_base_views(QueryBaseViewsRequest {
                    path: "view.base".into()
                })
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn markdown_rows_keep_identity_without_changing_column_aliases_or_values() {
    let (_dir, queries) = fixture();
    for (column, expected) in [
        ("file.name", "[A.md](<items/A.md>)"),
        ("file.basename", "[A](<items/A.md>)"),
        ("file.path", "items/A.md"),
        ("file.file", "[items/A.md](<items/A.md>)"),
    ] {
        let result = read(&queries, &format!("filters: 'file.path == \"items/A.md\"'\nproperties:\n  {column}: {{displayName: Source}}\nviews: [{{type: table, order: [{column}, price]}}]"), 100000, 1).unwrap();
        assert!(
            result.content.contains("| Source | price |"),
            "{}",
            result.content
        );
        assert!(result.content.contains(&format!("| {expected} | 12 |")));
        assert!(!result.content.contains("| file.path |"));
    }
    let result = read(&queries, "filters: 'file.path == \"items/A.md\"'\nformulas:\n  context: this.file\nviews: [{type: table, order: [formula.context, price]}]", 100000, 1).unwrap();
    assert!(
        result
            .content
            .contains("| file.path | formula.context | price |")
    );
    assert!(
        result
            .content
            .contains("| items/A.md | [view.base](<view.base>) | 12 |")
    );
    let json = serde_json::to_value(result).unwrap();
    assert_eq!(
        json.as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>(),
        [
            "content",
            "output_format",
            "pagination",
            "path",
            "view",
            "view_type"
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(
        json["pagination"],
        serde_json::json!({"page":1,"total_pages":1})
    );
}

fn markdown_urls(source: &str) -> Vec<String> {
    use ptdgrp_markdown::{MarkdownNode, Parser, ParserOptions, link::Link};
    let document =
        Parser::new_with_options(source, ParserOptions::default().enabled_gfm().enabled_ofm())
            .parse()
            .unwrap();
    crate::parser::active_node_indices(&document)
        .into_iter()
        .filter_map(|index| {
            if let MarkdownNode::Link(link) = &document.tree[index].body
                && let Link::Default(link) = link.as_ref()
            {
                // The Markdown parser normalizes URIs to percent-encoded UTF-8.
                // Decode once to assert the actual destination, including
                // literal filename percent signs, rather than its spelling.
                let mut input = document.text(&link.url).as_bytes();
                let mut bytes = Vec::new();
                while !input.is_empty() {
                    if input[0] == b'%' {
                        bytes.push(
                            u8::from_str_radix(std::str::from_utf8(&input[1..3]).unwrap(), 16)
                                .unwrap(),
                        );
                        input = &input[3..];
                    } else {
                        bytes.push(input[0]);
                        input = &input[1..];
                    }
                }
                Some(String::from_utf8(bytes).unwrap())
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn rendered_links_use_resolved_paths_and_preserve_fragments_aliases_and_urls() {
    let (_dir, queries) = fixture();
    fs::create_dir(queries.vault.root.join("资料")).unwrap();
    fs::write(
        queries.vault.root.join("资料/目标.md"),
        "# 标题\nblock ^id\n",
    )
    .unwrap();
    fs::write(queries.vault.root.join("items/A.md"), "---\nrelated:\n  - '[[目标#标题|中文别名]]'\n  - '[[目标#^id]]'\n  - '[[missing#标题|找不到]]'\n  - '[[https://example.com/中文?q=a&b=2#片段|网址]]'\nnested: {dependency: '[[目标#^id|阻塞]]', missing: null, list: [null]}\n---\n").unwrap();
    let result = read(&queries, "filters: 'file.path == \"items/A.md\"'\nviews: [{type: table, order: [file.name, related, nested]}]", 100000, 1).unwrap();
    assert!(result.content.contains("[中文别名](<资料/目标.md#标题>)"));
    assert!(result.content.contains("[目标#^id](<资料/目标.md#^id>)"));
    assert!(result.content.contains("[找不到](<missing#标题>)"));
    assert!(
        result
            .content
            .contains("{dependency: [阻塞](<资料/目标.md#^id>), missing: null, list: null}")
    );
    let urls = markdown_urls(&result.content);
    assert_eq!(
        urls,
        [
            "items/A.md",
            "资料/目标.md#标题",
            "资料/目标.md#^id",
            "missing#标题",
            "https://example.com/中文?q=a&b=2#片段",
            "资料/目标.md#^id",
        ]
    );
}

#[test]
#[cfg(unix)]
fn markdown_destinations_preserve_unicode_and_escape_literal_filename_delimiters() {
    let (_dir, queries) = fixture();
    fs::create_dir(queries.vault.root.join("资料")).unwrap();
    let path = "资料/中 文(a)[b]#?%|&copy;`.md";
    fs::write(queries.vault.root.join(path), "# Special\n").unwrap();
    let data = Dataset::new(&queries).unwrap();
    let index = data.exact(path).unwrap();
    let markdown = cell(&data, &Value::File(index));
    assert!(markdown.contains("资料/中 文(a)[b]%23%3F%25%7C&amp;copy;%60.md"));
    let urls = markdown_urls(&format!("| Source |\n| --- |\n| {markdown} |\n"));
    assert_eq!(urls, [path]);
    let result = read(
        &queries,
        "filters: 'file.inFolder(\"资料\")'\nviews: [{type: table, order: [file.path]}]",
        100000,
        1,
    )
    .unwrap();
    assert!(result.content.contains(&escape_cell(path)));
    assert!(!result.content.contains("| file.path | file.path |"));
}
