use super::*;
use crate::vault::{Vault, VaultConfig};
use serde_json::json;
use std::{fs, sync::Arc};
fn node(id: &str, kind: &str, x: f64, y: f64, width: f64, height: f64) -> serde_json::Value {
    json!({"id":id,"type":kind,"x":x,"y":y,"width":width,"height":height})
}
fn fixture(source: &serde_json::Value) -> (tempfile::TempDir, VaultQueries) {
    let dir = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(dir.path().into()).unwrap();
    fs::write(root.join("test.canvas"), source.to_string()).unwrap();
    let queries = VaultQueries::new(Arc::new(
        Vault::open(&root, VaultConfig::default()).unwrap(),
    ));
    (dir, queries)
}
#[test]
fn complete_text_references_arrows_self_loops_and_safe_ids() {
    let mut text = node("end\"; n99", "text", 0., 0., 10., 10.);
    text["text"] = json!("中文\n<em>x</em> & \"quotes\" | `code`\n```\nend\n```\n%% injection");
    let mut image = node("image", "file", 20., 0., 10., 10.);
    image["file"] = json!("images/x.png");
    image["subpath"] = json!("#^block");
    let mut url = node("URL", "link", 40., 0., 10., 10.);
    url["url"] = json!("https://example.com/?a=1&b=2");
    let mut edges = Vec::new();
    for (i, (from, to)) in [
        ("none", "none"),
        ("none", "arrow"),
        ("arrow", "none"),
        ("arrow", "arrow"),
    ]
    .iter()
    .enumerate()
    {
        edges.push(json!({"id":i.to_string(),"fromNode":"end\"; n99","toNode":"image","fromEnd":from,"toEnd":to,"label":"边|标签\""}));
    }
    edges.push(json!({"id":"self","fromNode":"URL","toNode":"URL"}));
    let (_dir, queries) = fixture(&json!({"nodes":[text,image,url],"edges":edges}));
    let result = queries
        .read_canvas(ReadCanvasRequest {
            path: "test.canvas".into(),
            page: 1,
            max_chars: Some(100000),
        })
        .unwrap();
    for expected in [
        "中文",
        "#60;em#62;x#60;/em#62;",
        "#96;#96;#96;",
        "#34;quotes#34;",
        "Image file",
        "Path: images/x.png",
        "Subpath: #35;^block",
        "URL",
        "n0 ---",
        "n0 -->",
        "n1 -->",
        "n0 <-->",
        "n2 --> n2",
    ] {
        assert!(
            result.content.contains(expected),
            "{expected}: {}",
            result.content
        );
    }
    assert_eq!(result.pagination.total_units, 8);
    assert!(!result.content.contains("end\"; n99"));
    assert!(!result.content.contains("```"));
    let first = queries
        .read_canvas(ReadCanvasRequest {
            path: "test.canvas".into(),
            page: 1,
            max_chars: Some(0),
        })
        .unwrap();
    assert_eq!(first.pagination.total_pages, 8);
    assert!(first.content.contains("中文"));
    let edge = queries
        .read_canvas(ReadCanvasRequest {
            path: "test.canvas".into(),
            page: 4,
            max_chars: Some(0),
        })
        .unwrap();
    assert!(edge.content.contains("中文"));
    assert!(edge.content.contains("Image file"));
    assert!(edge.content.contains("n0 ---"));
    assert_eq!(edge.pagination.returned_units, 1);
}
#[test]
fn nested_overlapping_empty_and_identical_groups() {
    let mut outer = node("outer", "group", 0., 0., 100., 100.);
    outer["label"] = json!("Outer");
    outer["background"] = json!("bg.jpg");
    let mut inner = node("inner", "group", 10., 10., 50., 50.);
    inner["label"] = json!("Inner");
    let mut overlap = node("overlap", "group", 15., 15., 50., 50.);
    overlap["label"] = json!("Overlap");
    let mut text = node("child", "text", 20., 20., 10., 10.);
    text["text"] = json!("child");
    let mut empty = node("empty", "group", 200., 200., 20., 20.);
    empty["label"] = json!("Empty");
    let identical = empty.clone();
    let mut identical = identical;
    identical["id"] = json!("coincident");
    let canvas: Canvas =
        serde_json::from_value(json!({"nodes":[outer,inner,overlap,text,empty,identical]}))
            .unwrap();
    let (units, warnings, ids) = validate(&canvas).unwrap();
    let result = render(&canvas, &ids, &units);
    for expected in [
        "n0 -. contains .-> n1",
        "n1 -. contains .-> n3",
        "n2 -. contains .-> n3",
        "Empty",
        "Background image: bg.jpg",
    ] {
        assert!(result.contains(expected));
    }
    assert_eq!(warnings.len(), 1);
    assert!(!result.contains("n4 -. contains .-> n5"));
}
#[test]
fn malformed_structures_are_errors_and_empty_pages_are_complete() {
    for source in [
        json!({"nodes":[{"id":"x","type":"text"}]}),
        json!({"nodes":[],"edges":[{"id":"e","fromNode":"missing","toNode":"missing"}]}),
        json!({"nodes":[{"id":"x","type":"alien","x":0,"y":0,"width":1,"height":1}]}),
    ] {
        let (_dir, queries) = fixture(&source);
        assert!(
            queries
                .read_canvas(ReadCanvasRequest {
                    path: "test.canvas".into(),
                    page: 1,
                    max_chars: None
                })
                .is_err()
        );
    }
    let (_dir, queries) = fixture(&json!({}));
    for page in [1, 10] {
        let result = queries
            .read_canvas(ReadCanvasRequest {
                path: "test.canvas".into(),
                page,
                max_chars: None,
            })
            .unwrap();
        assert!(result.content.ends_with("flowchart LR\n"));
        assert_eq!(result.pagination.returned_units, 0);
    }
    assert!(
        queries
            .read_canvas(ReadCanvasRequest {
                path: "test.canvas".into(),
                page: 0,
                max_chars: None
            })
            .is_err()
    );
}
#[test]
fn duplicate_ids_negative_rectangles_and_invalid_subpaths_fail() {
    let mut text = node("x", "text", 0., 0., 1., 1.);
    text["text"] = json!("x");
    for source in [
        json!({"nodes":[text.clone(),text.clone()]}),
        json!({"nodes":[{"id":"x","type":"text","text":"x","x":0,"y":0,"width":-1,"height":1}]}),
        json!({"nodes":[{"id":"x","type":"file","file":"a.md","subpath":"bad","x":0,"y":0,"width":1,"height":1}]}),
    ] {
        let (_dir, queries) = fixture(&source);
        assert!(
            queries
                .read_canvas(ReadCanvasRequest {
                    path: "test.canvas".into(),
                    page: 1,
                    max_chars: None
                })
                .is_err()
        );
    }
}
