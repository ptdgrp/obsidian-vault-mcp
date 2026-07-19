use std::fs;

use camino::Utf8PathBuf;
use serde_json::Value;
use tempfile::{TempDir, tempdir};

use super::super::{
    BlueprintPatch, EvidenceSubmitInput, ExternalBody, ResultsInput, TodoCreateRequest, TodoPatch,
    TodoStatus,
    service::{BlueprintCreateRequest, BlueprintCreated, BlueprintService, TodoUpdateOptions},
};

fn create_blueprint() -> (TempDir, BlueprintService, BlueprintCreated) {
    let directory = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).expect("UTF-8 path");
    let service = BlueprintService::new(root);
    let blueprint = service
        .blueprint_create(BlueprintCreateRequest {
            title: "协议测试".into(),
            created_by: "creator".into(),
            intent: "交付新协议".into(),
            constraints: vec!["不得泄露存储结构".into()],
            definition_of_done: vec!["协议可用".into()],
            plan: "按结构执行".into(),
            rubric: String::new(),
        })
        .expect("create Blueprint");
    (directory, service, blueprint)
}

fn assert_section_order_and_spacing(source: &str, sections: &[&str]) {
    let mut cursor = 0;
    for section in sections {
        let heading = format!("## {section}\n");
        let offset = source[cursor..]
            .find(&heading)
            .unwrap_or_else(|| panic!("missing section {section}"));
        let start = cursor + offset;
        if cursor != 0 {
            assert!(
                source[..start].ends_with("\n\n"),
                "section before {section} must end with a blank line"
            );
        }
        cursor = start + heading.len();
    }
    assert!(
        source.ends_with("\n\n"),
        "final section must end with a blank line"
    );
}

fn assert_no_internal_keys(value: &Value) {
    match value {
        Value::Object(map) => {
            for forbidden in ["source", "path", "document", "todo_index"] {
                assert!(
                    !map.contains_key(forbidden),
                    "public output leaked {forbidden}"
                );
            }
            for child in map.values() {
                assert_no_internal_keys(child);
            }
        }
        Value::Array(values) => values.iter().for_each(assert_no_internal_keys),
        _ => {}
    }
}

#[test]
fn create_uses_canonical_sections_frontmatter_and_blank_lines() {
    let (_directory, service, blueprint) = create_blueprint();
    let stored = service.blueprint_stored(&blueprint.id).unwrap();
    assert!(stored.source.contains("created_by: creator\n"));
    assert!(!stored.source.contains("## Record"));
    assert!(!stored.source.contains("## Evidence"));
    assert!(!stored.source.contains("## Todos"));
    assert_section_order_and_spacing(
        &stored.source,
        &[
            "Intent",
            "Constraints",
            "Definition of Done",
            "Plan",
            "Todo Graph",
            "Results",
            "Rubric",
            "Notes",
            "Revision History",
        ],
    );
}

#[test]
fn todo_uses_frontmatter_metadata_canonical_sections_and_nested_graph() {
    let (directory, service, blueprint) = create_blueprint();
    let parent = service
        .todo_create(TodoCreateRequest {
            blueprint_id: blueprint.id.clone(),
            title: "父任务".into(),
            created_by: "creator".into(),
            plan: "完成父任务".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(parent.detail.created_by, "creator");
    assert_eq!(parent.detail.owner, "creator");

    let child = service
        .todo_create(TodoCreateRequest {
            blueprint_id: blueprint.id.clone(),
            title: "子任务".into(),
            created_by: "creator".into(),
            plan: "完成子任务".into(),
            parent_id: Some(parent.graph.id.clone()),
            ..Default::default()
        })
        .unwrap();
    let parent = service.todo_get(&blueprint.id, &parent.graph.id).unwrap();
    assert_eq!(parent.graph.children[0].id, child.graph.id);
    let blueprint_source = service.blueprint_stored(&blueprint.id).unwrap().source;
    assert_section_order_and_spacing(
        &blueprint_source,
        &[
            "Intent",
            "Constraints",
            "Definition of Done",
            "Plan",
            "Todo Graph",
            "Results",
            "Rubric",
            "Notes",
            "Revision History",
        ],
    );

    let source = fs::read_to_string(directory.path().join(format!(
        ".blueprint/blueprints/{}/todos/{}.md",
        blueprint.id, child.graph.id
    )))
    .unwrap();
    assert!(source.contains("created_by: creator\nowner: creator\n"));
    assert!(!source.contains("## Intent"));
    assert!(!source.contains("## Results"));
    assert_section_order_and_spacing(
        &source,
        &[
            "Plan",
            "Completion Criteria",
            "Handoff",
            "Result",
            "Evidence",
            "Notes",
            "Revision History",
        ],
    );
}

#[test]
fn only_creator_or_owner_can_mutate_a_todo() {
    let (_directory, service, blueprint) = create_blueprint();
    let todo = service
        .todo_create(TodoCreateRequest {
            blueprint_id: blueprint.id.clone(),
            title: "受控任务".into(),
            created_by: "creator".into(),
            owner: Some("owner".into()),
            plan: "执行".into(),
            ..Default::default()
        })
        .unwrap();

    let error = service
        .todo_start(&blueprint.id, &todo.graph.id, "intruder", None)
        .unwrap_err();
    assert!(error.to_string().contains("Owner or Created By"));

    let started = service
        .todo_start(&blueprint.id, &todo.graph.id, "owner", None)
        .unwrap();
    assert_eq!(started.graph.status, TodoStatus::InProgress);

    let error = service
        .todo_update(
            &blueprint.id,
            &todo.graph.id,
            TodoPatch {
                notes: Some("unauthorized".into()),
                ..Default::default()
            },
            TodoUpdateOptions {
                changed_by: Some("intruder"),
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(error.to_string().contains("Owner or Created By"));

    let reassigned = service
        .todo_assign(&blueprint.id, &todo.graph.id, "next", "creator", None)
        .unwrap_err();
    assert!(reassigned.to_string().contains("Handoff"));
}

#[test]
fn child_creation_and_evidence_submission_enforce_todo_authority() {
    let (_directory, service, blueprint) = create_blueprint();
    let parent = service
        .todo_create(TodoCreateRequest {
            blueprint_id: blueprint.id.clone(),
            title: "父任务".into(),
            created_by: "creator".into(),
            owner: Some("owner".into()),
            plan: "执行".into(),
            ..Default::default()
        })
        .unwrap();

    let error = service
        .todo_create(TodoCreateRequest {
            blueprint_id: blueprint.id.clone(),
            title: "非法子任务".into(),
            created_by: "intruder".into(),
            plan: "执行".into(),
            parent_id: Some(parent.graph.id.clone()),
            ..Default::default()
        })
        .unwrap_err();
    assert!(error.to_string().contains("Owner or Created By"));

    let error = service
        .evidence_submit(EvidenceSubmitInput {
            blueprint_id: blueprint.id.clone(),
            todo_id: parent.graph.id.clone(),
            changed_by: "intruder".into(),
            title: "结果".into(),
            body: ExternalBody::from_text("通过"),
            expected_etag: None,
        })
        .unwrap_err();
    assert!(error.to_string().contains("Owner or Created By"));

    let evidence = service
        .evidence_submit(EvidenceSubmitInput {
            blueprint_id: blueprint.id.clone(),
            todo_id: parent.graph.id.clone(),
            changed_by: "owner".into(),
            title: "结果".into(),
            body: ExternalBody::from_text("通过"),
            expected_etag: None,
        })
        .unwrap();
    assert_eq!(evidence.todo_id, parent.graph.id);
    assert_eq!(
        service
            .evidence_list(&blueprint.id, &evidence.todo_id, 1)
            .unwrap()
            .evidence
            .len(),
        1
    );
}

#[test]
fn blueprint_get_is_structured_and_never_serializes_storage_fields() {
    let (_directory, service, blueprint) = create_blueprint();
    service
        .blueprint_update_semantic(
            &blueprint.id,
            BlueprintPatch {
                results: Some(ResultsInput {
                    body: ExternalBody::from_text("已完成一部分"),
                    evidence_ids: vec![],
                }),
                notes: Some("公开说明".into()),
                ..Default::default()
            },
            None,
            None,
            None,
        )
        .unwrap();
    let output = service.blueprint_get(&blueprint.id).unwrap();
    assert_eq!(output.created_by, "creator");
    assert_eq!(output.results, "已完成一部分");
    assert_no_internal_keys(&serde_json::to_value(output).unwrap());
    assert_no_internal_keys(&serde_json::to_value(blueprint).unwrap());
}
