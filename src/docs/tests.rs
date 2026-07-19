use rmcp::model::Tool;
use schemars::JsonSchema;
use serde::Serialize;

use super::render_docs;

#[derive(JsonSchema)]
struct ExampleInput {
    #[schemars(description = "selector | note")]
    note: String,
    detail: Detail,
    mode: Mode,
}

#[derive(Serialize, JsonSchema)]
struct Detail {
    label: String,
}

#[derive(JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Preview,
    Apply,
}

#[derive(Serialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Outcome {
    Renamed { detail: Detail },
    Skipped { reason: String },
}

#[derive(Serialize, JsonSchema)]
struct ExampleOutput {
    result: Outcome,
}

#[test]
fn render_docs_escapes_markdown_tables_and_shows_nested_types() {
    let tool = Tool::new("demo", "Line one\nvalue | column", serde_json::Map::new())
        .with_input_schema::<ExampleInput>()
        .with_output_schema::<ExampleOutput>();

    let rendered = render_docs(&[tool]).expect("render docs");

    assert!(rendered.contains("| `demo` | Line one value \\| column |"));
    assert!(rendered.contains("| `note` | `string` | yes | selector \\| note |"));
    assert!(rendered.contains("Nested types:\n\n### `Detail`"));
    assert!(rendered.contains("| `preview` |"));
    assert!(rendered.contains("| `renamed` | `detail: Detail` |"));
    assert!(rendered.contains("| `skipped` | `reason: string` |"));
    assert!(!rendered.ends_with("\n\n"));
}
