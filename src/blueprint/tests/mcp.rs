use crate::blueprint::mcp::BlueprintMcp;

#[test]
fn exposes_the_twenty_blueprint_protocol_tools() {
    let names = BlueprintMcp::tool_definitions()
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect::<Vec<_>>();
    assert_eq!(names.len(), 20);
    for name in [
        "blueprint_create",
        "blueprint_get",
        "blueprint_list",
        "blueprint_update",
        "blueprint_status",
        "blueprint_close",
        "blueprint_cancel",
        "dod_update",
        "evidence_submit",
        "evidence_list",
        "revision_append",
        "todo_create",
        "todo_get",
        "todo_list",
        "todo_update",
        "todo_assign",
        "todo_start",
        "todo_complete",
        "todo_block",
        "todo_cancel",
    ] {
        assert!(names.iter().any(|actual| actual == name), "missing {name}");
    }
    assert!(
        !names
            .iter()
            .any(|name| name == "blueprint_init" || name == "blueprint_discover")
    );
}

#[test]
fn tool_schemas_expose_concrete_inputs_and_outputs() {
    let tools = BlueprintMcp::tool_definitions();
    let create = tools
        .iter()
        .find(|tool| tool.name == "blueprint_create")
        .expect("blueprint_create schema");
    let properties = create
        .input_schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .expect("create schema properties");
    for field in [
        "title",
        "created_by",
        "intent",
        "definition_of_done",
        "plan",
        "rubric",
    ] {
        assert!(
            properties.contains_key(field),
            "missing create field {field}"
        );
    }
    let required = create
        .input_schema
        .get("required")
        .and_then(serde_json::Value::as_array)
        .expect("create required fields");
    assert!(
        required.iter().any(|value| value == "rubric"),
        "rubric must be required"
    );
    let output = create.output_schema.as_ref().expect("create output schema");
    assert!(
        output.get("properties").is_some_and(|properties| {
            properties.as_object().is_some_and(|properties| {
                properties.contains_key("id") && !properties.contains_key("data")
            })
        }),
        "create output schema must be concrete instead of a generic data wrapper"
    );

    for tool_name in ["todo_update", "todo_complete", "todo_block"] {
        let tool = tools
            .iter()
            .find(|tool| tool.name == tool_name)
            .unwrap_or_else(|| panic!("{tool_name} schema"));
        let properties = tool
            .input_schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .expect("todo mutation properties");
        assert!(
            properties.contains_key("expected_blueprint_etag"),
            "{tool_name} must expose the graph ETag"
        );
        assert!(
            properties.contains_key("expected_todo_etag"),
            "{tool_name} must expose the detail ETag"
        );
    }
}

#[test]
fn v3_tool_descriptions_explain_agent_owned_semantics() {
    let tools = BlueprintMcp::tool_definitions();
    let description = |name: &str| {
        tools
            .iter()
            .find(|tool| tool.name == name)
            .and_then(|tool| tool.description.as_deref())
            .unwrap_or_else(|| panic!("{name} description"))
    };

    assert!(description("blueprint_create").contains("Agent"));
    assert!(description("blueprint_create").contains("Rubric"));
    assert!(description("blueprint_update").contains("Rubric"));
    assert!(description("blueprint_update").contains("Agent"));
    assert!(description("evidence_submit").contains("structural"));
    assert!(description("revision_append").contains("append-only"));
}
