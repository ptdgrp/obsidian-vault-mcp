use crate::blueprint::mcp::BlueprintMcp;

#[test]
fn exposes_only_the_seventeen_blueprint_protocol_tools() {
    let names = BlueprintMcp::tool_definitions()
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect::<Vec<_>>();
    assert_eq!(names.len(), 17);
    for name in [
        "blueprint_create",
        "blueprint_get",
        "blueprint_list",
        "blueprint_update",
        "blueprint_status",
        "blueprint_close",
        "blueprint_cancel",
        "dod_update",
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
    assert!(
        create
            .input_schema
            .get("properties")
            .is_some_and(|properties| {
                properties
                    .as_object()
                    .is_some_and(|properties| properties.contains_key("created_by"))
            }),
        "create schema must expose named request fields"
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
}
