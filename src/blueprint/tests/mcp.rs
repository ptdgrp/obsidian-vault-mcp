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
