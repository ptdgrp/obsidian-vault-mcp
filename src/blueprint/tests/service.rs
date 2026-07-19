use std::fs;

use camino::Utf8PathBuf;
use tempfile::{TempDir, tempdir};

use super::super::{
    model::{BlueprintPatch, TodoCreateRequest, TodoPatch, TodoStatus},
    service::{BlueprintCreateRequest, BlueprintCreated, BlueprintService, CheckUpdate},
};

fn create_blueprint() -> (TempDir, BlueprintService, BlueprintCreated) {
    let directory = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).expect("UTF-8 path");
    let service = BlueprintService::new(root);
    let blueprint = service
        .blueprint_create(BlueprintCreateRequest {
            title: "协议测试".into(),
            created_by: "creator".into(),
            intent: "验证 Blueprint 协议".into(),
            constraints: vec!["保留 Markdown".into()],
            definition_of_done: vec!["完成协议验证".into()],
            plan: "按状态转换验证".into(),
            rubric: "按目标和约束进行评估".into(),
        })
        .expect("create Blueprint");
    (directory, service, blueprint)
}

#[test]
fn todo_create_writes_graph_link_and_independent_detail_document() {
    let (_directory, service, blueprint) = create_blueprint();
    let todo = service
        .todo_create(TodoCreateRequest {
            blueprint_id: blueprint.id.clone(),
            title: "检查连续性".into(),
            created_by: "planner".into(),
            intent: "检查人物行为变化".into(),
            plan: "运行连续性评估".into(),
            completion_criteria: vec!["引用关键段落".into()],
            ..Default::default()
        })
        .unwrap();

    let blueprint_source = service.blueprint_get(&blueprint.id).unwrap().source;
    assert!(blueprint_source.contains(&format!(
        "[检查连续性](todos/{}.md) ^{}",
        todo.graph.id, todo.graph.id
    )));
    let fetched = service.todo_get(&blueprint.id, &todo.graph.id).unwrap();
    assert_eq!(fetched.detail.intent, "检查人物行为变化");
    assert_eq!(fetched.graph.status, TodoStatus::Pending);
    assert_eq!(fetched.blueprint_etag, todo.blueprint_etag);
    assert!(!fetched.todo_etag.is_empty());
}

#[test]
fn todo_patch_checks_both_etags_syncs_title_and_records_semantic_revision() {
    let (_directory, service, blueprint) = create_blueprint();
    let todo = service
        .todo_create(TodoCreateRequest {
            blueprint_id: blueprint.id.clone(),
            title: "原始标题".into(),
            created_by: "planner".into(),
            intent: "原始目标".into(),
            plan: "原始计划".into(),
            ..Default::default()
        })
        .unwrap();

    let error = service
        .todo_update(
            &blueprint.id,
            &todo.graph.id,
            TodoPatch {
                intent: Some("新目标".into()),
                ..Default::default()
            },
            Some("planner"),
            Some("目标调整"),
            Some("stale"),
            Some(&todo.todo_etag),
        )
        .unwrap_err();
    assert!(error.to_string().contains("Blueprint ETag"));

    let updated = service
        .todo_update(
            &blueprint.id,
            &todo.graph.id,
            TodoPatch {
                title: Some("更新标题".into()),
                intent: Some("新目标".into()),
                plan: Some("新计划".into()),
                ..Default::default()
            },
            Some("planner"),
            Some("目标调整"),
            Some(&todo.blueprint_etag),
            Some(&todo.todo_etag),
        )
        .unwrap();
    assert_eq!(updated.graph.title, "更新标题");
    assert_eq!(updated.detail.title, "更新标题");
    assert_eq!(updated.detail.intent, "新目标");
    assert!(
        updated
            .detail
            .revisions
            .iter()
            .any(|entry| entry.markdown.contains("目标调整"))
    );
}

#[test]
fn todo_create_invalid_graph_request_leaves_no_detail_document() {
    let (directory, service, blueprint) = create_blueprint();
    assert!(
        service
            .todo_create(TodoCreateRequest {
                blueprint_id: blueprint.id.clone(),
                title: "不会留下半成品".into(),
                created_by: "planner".into(),
                intent: "验证回滚".into(),
                plan: "触发无效依赖".into(),
                depends_on: vec!["todo-missing".into()],
                ..Default::default()
            })
            .is_err()
    );
    let todos = Utf8PathBuf::from_path_buf(directory.path().to_path_buf())
        .unwrap()
        .join(".blueprint/blueprints")
        .join(&blueprint.id)
        .join("todos");
    assert!(fs::read_dir(todos).unwrap().next().is_none());
}

#[test]
fn stale_todo_etag_does_not_block_or_complete_any_document() {
    let (_directory, service, blueprint) = create_blueprint();
    let todo = service
        .todo_create(TodoCreateRequest {
            blueprint_id: blueprint.id.clone(),
            title: "受保护写入".into(),
            created_by: "planner".into(),
            intent: "验证 ETag".into(),
            plan: "先开始".into(),
            owner: Some("agent".into()),
            ..Default::default()
        })
        .unwrap();
    let started = service
        .todo_start(&blueprint.id, &todo.graph.id, Some(&todo.blueprint_etag))
        .unwrap();
    let before = service.todo_get(&blueprint.id, &todo.graph.id).unwrap();
    assert!(
        service
            .todo_update(
                &blueprint.id,
                &todo.graph.id,
                TodoPatch {
                    handoff: Some("would be lost".into()),
                    ..Default::default()
                },
                None,
                None,
                Some(&before.blueprint_etag),
                Some("stale-todo-etag"),
            )
            .is_err()
    );
    let after_update = service.todo_get(&blueprint.id, &todo.graph.id).unwrap();
    assert_eq!(after_update.todo_etag, before.todo_etag);
    assert_eq!(after_update.blueprint_etag, before.blueprint_etag);
    assert!(
        service
            .todo_block(
                &blueprint.id,
                &todo.graph.id,
                "external",
                "resume later",
                Some(&before.blueprint_etag),
                Some("stale-todo-etag"),
            )
            .is_err()
    );
    let after_block = service.todo_get(&blueprint.id, &todo.graph.id).unwrap();
    assert_eq!(after_block.graph.status, TodoStatus::InProgress);
    assert_eq!(after_block.todo_etag, before.todo_etag);

    assert!(
        service
            .todo_complete(
                &blueprint.id,
                &todo.graph.id,
                "agent",
                "finished",
                Some(&before.blueprint_etag),
                Some("stale-todo-etag"),
            )
            .is_err()
    );
    let after_complete = service.todo_get(&blueprint.id, &todo.graph.id).unwrap();
    assert_eq!(after_complete.graph.status, TodoStatus::InProgress);
    assert_eq!(after_complete.todo_etag, before.todo_etag);
    assert_ne!(started.blueprint_etag, todo.blueprint_etag);
}

#[test]
fn semantic_update_requires_and_appends_revision() {
    let (_directory, service, created) = create_blueprint();
    let error = service
        .blueprint_update_semantic(
            &created.id,
            BlueprintPatch {
                intent: Some("新目标".into()),
                ..Default::default()
            },
            None,
            None,
            None,
        )
        .unwrap_err();
    assert!(error.to_string().contains("changed_by"));
    let updated = service
        .blueprint_update_semantic(
            &created.id,
            BlueprintPatch {
                intent: Some("新目标".into()),
                ..Default::default()
            },
            Some("agent"),
            Some("用户调整方向"),
            Some(&created.etag),
        )
        .unwrap();
    assert!(updated.source.contains("## Revision History"));
    assert!(updated.source.contains("- Reason: 用户调整方向"));
}

#[test]
fn todo_details_are_the_source_for_handoff_criteria_and_results() {
    let (_directory, service, blueprint) = create_blueprint();
    let todo = service
        .todo_create_legacy(
            &blueprint.id,
            "detail-backed",
            "creator",
            None,
            Some("agent"),
            &[],
            &["verify detail".into()],
            None,
        )
        .unwrap();
    service
        .todo_update_legacy(
            &blueprint.id,
            &todo.id,
            None,
            None,
            Some(&[CheckUpdate {
                text: "verify detail".into(),
                completed: true,
            }]),
            Some(&["resume from detail".into()]),
            Some("detail result"),
            None,
        )
        .unwrap();
    let loaded = service.todo_get(&blueprint.id, &todo.id).unwrap();
    assert!(loaded.completion_criteria[0].completed);
    assert_eq!(loaded.handoff, vec!["resume from detail"]);
    assert_eq!(loaded.result_summary.as_deref(), Some("detail result"));
    let full = service.blueprint_view(&blueprint.id, Some("full")).unwrap();
    assert_eq!(full.todo_index.len(), 1);
    let graph = full.source.unwrap_or_default();
    assert!(graph.contains("todos/"));
    assert!(!graph.contains("Completion Criteria:"));
    assert!(!graph.contains("Handoff:"));
    assert!(!graph.contains("Result Summary:"));
}

#[test]
fn todo_detail_write_failure_does_not_commit_central_graph_change() {
    let (directory, service, blueprint) = create_blueprint();
    let todo = service
        .todo_create_legacy(
            &blueprint.id,
            "atomic completion",
            "creator",
            None,
            Some("agent"),
            &[],
            &[],
            None,
        )
        .unwrap();
    assert!(
        service
            .todo_update_legacy(
                &blueprint.id,
                &todo.id,
                Some("renamed only in graph if ordering is wrong"),
                None,
                Some(&[CheckUpdate {
                    text: " ".into(),
                    completed: false,
                }]),
                None,
                None,
                None,
            )
            .is_err()
    );
    let graph = fs::read_to_string(
        Utf8PathBuf::from_path_buf(directory.path().to_path_buf())
            .unwrap()
            .join(".blueprint/blueprints")
            .join(&blueprint.id)
            .join("blueprint.md"),
    )
    .unwrap();
    assert!(graph.contains(&format!(
        "[atomic completion](todos/{}.md) ^{}",
        todo.id, todo.id
    )));
    assert!(!graph.contains("renamed only in graph if ordering is wrong"));
}

#[test]
fn complete_close_rejects_dangling_evidence_reference() {
    let (directory, service, blueprint) = create_blueprint();
    service
        .dod_update(
            &blueprint.id,
            &first_dod_id(&blueprint.source),
            true,
            None,
            None,
        )
        .unwrap();
    let path = Utf8PathBuf::from_path_buf(directory.path().to_path_buf())
        .unwrap()
        .join(".blueprint/blueprints")
        .join(&blueprint.id)
        .join("blueprint.md");
    let source = fs::read_to_string(&path).unwrap().replace(
        "## Results\n\n## Evidence",
        "## Results\n\n- Evidence: [missing](#^evidence-missing)\n\n## Evidence\n\n### Present ^evidence-present",
    );
    fs::write(path, source).unwrap();

    let error = service
        .blueprint_close(&blueprint.id, "closer", None, None)
        .unwrap_err();
    assert!(error.to_string().contains("Evidence"), "{error:#}");
}

#[test]
fn close_and_cancel_append_revisions() {
    let (_directory, service, complete) = create_blueprint();
    service
        .blueprint_close(&complete.id, "closer", Some("stopped"), None)
        .unwrap();
    let closed = service.blueprint_get(&complete.id).unwrap();
    assert!(closed.source.contains("^revision-"));
    assert!(closed.source.contains("- Reason: stopped"));

    let (_directory, service, cancelled) = create_blueprint();
    service
        .blueprint_cancel(&cancelled.id, "canceller", "obsolete", None)
        .unwrap();
    let cancelled = service.blueprint_get(&cancelled.id).unwrap();
    assert!(cancelled.source.contains("^revision-"));
    assert!(cancelled.source.contains("- Reason: obsolete"));
}

#[test]
fn mutations_reject_closed_blueprints_and_failed_create_leaves_no_orphan() {
    let (directory, service, blueprint) = create_blueprint();
    let stale = "stale";
    assert!(
        service
            .todo_create_legacy(
                &blueprint.id,
                "will fail",
                "creator",
                None,
                None,
                &[],
                &[],
                Some(stale)
            )
            .is_err()
    );
    let todos = Utf8PathBuf::from_path_buf(directory.path().to_path_buf())
        .unwrap()
        .join(".blueprint/blueprints")
        .join(&blueprint.id)
        .join("todos");
    assert!(fs::read_dir(todos).unwrap().next().is_none());
    service
        .blueprint_cancel(&blueprint.id, "agent", "stopped", None)
        .unwrap();
    assert!(
        service
            .dod_update(&blueprint.id, "dod-missing", true, None, None)
            .is_err()
    );
    assert!(
        service
            .todo_create_legacy(&blueprint.id, "nope", "creator", None, None, &[], &[], None)
            .is_err()
    );
    assert!(
        service
            .blueprint_cancel(&blueprint.id, "agent", "again", None)
            .is_err()
    );
}

fn first_dod_id(source: &str) -> String {
    source
        .lines()
        .find_map(|line| {
            line.split_once("^dod-")
                .map(|(_, suffix)| format!("dod-{suffix}"))
        })
        .expect("generated DoD ID")
}

#[test]
fn creating_a_blueprint_automatically_creates_workspace_and_generates_protocol_document() {
    let directory = tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
    let service = BlueprintService::new(root.clone());
    let created = service
        .blueprint_create(BlueprintCreateRequest {
            title: "实现 Blueprint".to_string(),
            created_by: "agent".to_string(),
            intent: "完成协议".to_string(),
            constraints: vec!["保留 Markdown".to_string()],
            definition_of_done: vec!["完成实现".to_string()],
            plan: "逐步完成".to_string(),
            rubric: "核对完成条件".to_string(),
        })
        .unwrap();
    assert!(created.id.starts_with("bp-"));
    assert!(created.source.contains("- Created By: agent"));
    assert!(created.source.contains("^dod-"));
    assert!(created.path.is_file());
}

#[test]
fn get_list_and_status_return_the_active_blueprint_and_derived_empty_todo_state() {
    let directory = tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
    let service = BlueprintService::new(root);
    let created = service
        .blueprint_create(BlueprintCreateRequest {
            title: "实现 Blueprint".to_string(),
            created_by: "agent".to_string(),
            intent: "完成协议".to_string(),
            constraints: vec![],
            definition_of_done: vec!["完成实现".to_string()],
            plan: "逐步完成".to_string(),
            rubric: "核对完成条件".to_string(),
        })
        .unwrap();
    assert_eq!(
        service.blueprint_list("active").unwrap(),
        vec![created.id.clone()]
    );
    assert_eq!(
        service.blueprint_get(&created.id).unwrap().etag,
        created.etag
    );
    let status = service.blueprint_status(&created.id).unwrap();
    assert!(status.ready_todos.is_empty());
    assert!(status.not_ready_todos.is_empty());
}

#[test]
fn blueprint_update_preserves_unknown_markdown() {
    let directory = tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
    let service = BlueprintService::new(root.clone());
    let created = service
        .blueprint_create(BlueprintCreateRequest {
            title: "实现 Blueprint".to_string(),
            created_by: "agent".to_string(),
            intent: "完成协议".to_string(),
            constraints: vec![],
            definition_of_done: vec!["完成实现".to_string()],
            plan: "逐步完成".to_string(),
            rubric: "核对完成条件".to_string(),
        })
        .unwrap();
    let path = root
        .join(".blueprint/blueprints")
        .join(&created.id)
        .join("blueprint.md");
    let source = created
        .source
        .replace("## Notes", "## Extra\n\n> [!note] keep\n\n## Notes");
    fs::write(path, source).unwrap();

    let updated = service
        .blueprint_update(
            &created.id,
            None,
            None,
            None,
            None,
            Some("### Current Outcome\n\n完成。"),
            None,
            None,
        )
        .unwrap();

    assert!(updated.source.contains("## Extra\n\n> [!note] keep"));
    assert!(
        updated
            .source
            .contains("## Results\n\n### Current Outcome\n\n完成。")
    );
}

#[test]
fn blueprint_create_rejects_blank_required_fields_and_empty_definition_of_done() {
    let directory = tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
    let service = BlueprintService::new(root);
    let request = |title: &str, created_by: &str, intent: &str, plan: &str, dod: Vec<String>| {
        BlueprintCreateRequest {
            title: title.into(),
            created_by: created_by.into(),
            intent: intent.into(),
            constraints: vec![],
            definition_of_done: dod,
            plan: plan.into(),
            rubric: "核对完成条件".into(),
        }
    };

    for (input, expected) in [
        (
            request(" ", "agent", "intent", "plan", vec!["done".into()]),
            "title",
        ),
        (
            request("title", " ", "intent", "plan", vec!["done".into()]),
            "created_by",
        ),
        (
            request("title", "agent", " ", "plan", vec!["done".into()]),
            "intent",
        ),
        (
            request("title", "agent", "intent", " ", vec!["done".into()]),
            "plan",
        ),
    ] {
        let error = service
            .blueprint_create(input)
            .expect_err("blank required field must fail");
        assert!(error.to_string().contains(expected), "{error:#}");
    }
    let error = service
        .blueprint_create(request("title", "agent", "intent", "plan", vec![]))
        .expect_err("empty DoD must fail");
    assert!(error.to_string().contains("definition_of_done"));
}

#[test]
fn blueprint_update_changes_allowed_sections_and_rejects_blank_required_text() {
    let (directory, service, blueprint) = create_blueprint();
    let path = Utf8PathBuf::from_path_buf(directory.path().to_path_buf())
        .unwrap()
        .join(".blueprint/blueprints")
        .join(&blueprint.id)
        .join("blueprint.md");
    fs::write(
        &path,
        blueprint
            .source
            .replace("## Notes", "## Extra\n\nkeep me\n\n## Notes"),
    )
    .expect("add unknown section");

    let updated = service
        .blueprint_update(
            &blueprint.id,
            Some(" renamed "),
            Some("revised intent"),
            Some(&[" first ".into(), "second".into()]),
            Some("revised plan"),
            Some("### Current Outcome\n\nworking"),
            Some("note text"),
            None,
        )
        .expect("update Blueprint sections");
    assert!(updated.source.contains("# renamed\n"));
    assert!(updated.source.contains("- Created By: creator"));
    assert!(updated.source.contains("## Intent\n\nrevised intent"));
    assert!(
        updated
            .source
            .contains("## Constraints\n\n- first\n- second")
    );
    assert!(updated.source.contains("## Plan\n\nrevised plan"));
    assert!(
        updated
            .source
            .contains("## Results\n\n### Current Outcome\n\nworking")
    );
    assert!(updated.source.contains("## Notes\n\nnote text"));
    assert!(updated.source.contains("## Extra\n\nkeep me"));

    for (title, intent, plan, expected) in [
        (Some(" "), None, None, "title"),
        (None, Some(" "), None, "intent"),
        (None, None, Some(" "), "plan"),
    ] {
        let before = service
            .blueprint_get(&blueprint.id)
            .expect("current Blueprint");
        let error = service
            .blueprint_update(
                &blueprint.id,
                title,
                intent,
                None,
                plan,
                None,
                None,
                Some(&before.etag),
            )
            .expect_err("blank required update must fail");
        assert!(error.to_string().contains(expected), "{error:#}");
        assert_eq!(
            service.blueprint_get(&blueprint.id).unwrap().etag,
            before.etag
        );
    }
}

#[test]
fn todo_create_and_start_write_protocol_state_after_owner_assignment() {
    let directory = tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
    let service = BlueprintService::new(root);
    let blueprint = service
        .blueprint_create(BlueprintCreateRequest {
            title: "实现".into(),
            created_by: "agent".into(),
            intent: "完成".into(),
            constraints: vec![],
            definition_of_done: vec!["完成".into()],
            plan: "执行".into(),
            rubric: "核对完成条件".into(),
        })
        .unwrap();
    let todo = service
        .todo_create_legacy(
            &blueprint.id,
            "实现存储",
            "agent",
            None,
            Some("agent"),
            &[],
            &[],
            None,
        )
        .unwrap();
    assert_eq!(todo.status, super::super::model::TodoStatus::Pending);
    let started = service.todo_start(&blueprint.id, &todo.id, None).unwrap();
    assert_eq!(started.status, super::super::model::TodoStatus::InProgress);
}

#[test]
fn todo_completion_requires_criteria_then_close_moves_the_document() {
    let directory = tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
    let service = BlueprintService::new(root);
    let blueprint = service
        .blueprint_create(BlueprintCreateRequest {
            title: "实现".into(),
            created_by: "agent".into(),
            intent: "完成".into(),
            constraints: vec![],
            definition_of_done: vec!["完成".into()],
            plan: "执行".into(),
            rubric: "核对完成条件".into(),
        })
        .unwrap();
    let todo = service
        .todo_create_legacy(
            &blueprint.id,
            "实现存储",
            "agent",
            None,
            Some("agent"),
            &[],
            &["测试通过".into()],
            None,
        )
        .unwrap();
    service.todo_start(&blueprint.id, &todo.id, None).unwrap();
    assert!(
        service
            .todo_complete_legacy(&blueprint.id, &todo.id, "agent", "完成", None)
            .is_err()
    );
    service
        .todo_update_legacy(
            &blueprint.id,
            &todo.id,
            None,
            None,
            Some(&[super::super::service::CheckUpdate {
                text: "测试通过".into(),
                completed: true,
            }]),
            None,
            None,
            None,
        )
        .unwrap();
    let completed = service
        .todo_complete_legacy(&blueprint.id, &todo.id, "agent", "完成", None)
        .unwrap();
    assert_eq!(completed.status, super::super::model::TodoStatus::Completed);
    let closed = service
        .blueprint_close(&blueprint.id, "agent", Some("DoD 尚未更新"), None)
        .unwrap();
    assert_eq!(closed.state, "closed");
    assert_eq!(
        service.blueprint_list("closed").unwrap(),
        vec![blueprint.id]
    );
}

#[test]
fn creates_child_todos_in_the_parent_children_list() {
    let directory = tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
    let service = BlueprintService::new(root);
    let blueprint = service
        .blueprint_create(BlueprintCreateRequest {
            title: "实现".into(),
            created_by: "agent".into(),
            intent: "完成".into(),
            constraints: vec![],
            definition_of_done: vec!["完成".into()],
            plan: "执行".into(),
            rubric: "核对完成条件".into(),
        })
        .unwrap();
    let parent = service
        .todo_create_legacy(&blueprint.id, "父任务", "agent", None, None, &[], &[], None)
        .unwrap();
    let child = service
        .todo_create_legacy(
            &blueprint.id,
            "子任务",
            "agent",
            Some(&parent.id),
            None,
            &[],
            &[],
            None,
        )
        .unwrap();
    let loaded = service.todo_get(&blueprint.id, &parent.id).unwrap();
    assert_eq!(loaded.children.len(), 1);
    assert_eq!(loaded.children[0].id, child.id);
}

#[test]
fn blueprint_views_return_full_source_or_resume_context() {
    let (_directory, service, blueprint) = create_blueprint();
    let ready = service
        .todo_create_legacy(
            &blueprint.id,
            "ready",
            "creator",
            None,
            None,
            &[],
            &[],
            None,
        )
        .expect("create ready Todo");
    let running = service
        .todo_create_legacy(
            &blueprint.id,
            "running",
            "creator",
            None,
            Some("agent-a"),
            &[],
            &[],
            None,
        )
        .expect("create running Todo");
    service
        .todo_start(&blueprint.id, &running.id, None)
        .expect("start Todo");
    let blocked = service
        .todo_create_legacy(
            &blueprint.id,
            "blocked",
            "creator",
            None,
            Some("agent-b"),
            &[],
            &[],
            None,
        )
        .expect("create blocked Todo");
    service
        .todo_start(&blueprint.id, &blocked.id, None)
        .expect("start blocked Todo");
    service
        .todo_block_legacy(
            &blueprint.id,
            &blocked.id,
            "waiting for user",
            "resume after decision",
            None,
        )
        .expect("block Todo");

    let full = service
        .blueprint_view(&blueprint.id, None)
        .expect("full view");
    let stored = service
        .blueprint_get(&blueprint.id)
        .expect("stored Blueprint");
    assert_eq!(full.source.as_deref(), Some(stored.source.as_str()));
    assert!(full.resume.is_none());

    let resume = service
        .blueprint_view(&blueprint.id, Some("resume"))
        .expect("resume view")
        .resume
        .expect("resume payload");
    assert_eq!(resume.intent, "验证 Blueprint 协议");
    assert_eq!(resume.constraints, "- 保留 Markdown");
    assert_eq!(resume.plan, "按状态转换验证");
    assert_eq!(resume.ready_todos, vec![ready.id.clone()]);
    assert_eq!(resume.open_definition_of_done.len(), 1);
    let mut active_ids = resume
        .active_todos
        .iter()
        .map(|todo| todo.id.as_str())
        .collect::<Vec<_>>();
    active_ids.sort_unstable();
    let mut expected_ids = vec![ready.id.as_str(), running.id.as_str(), blocked.id.as_str()];
    expected_ids.sort_unstable();
    assert_eq!(active_ids, expected_ids);

    let error = service
        .blueprint_view(&blueprint.id, Some("summary"))
        .expect_err("unsupported view must fail");
    assert!(error.to_string().contains("view must be full or resume"));
}

#[test]
fn blueprint_status_classifies_ready_blocked_unassigned_and_open_work() {
    let (_directory, service, blueprint) = create_blueprint();
    let prerequisite = service
        .todo_create_legacy(
            &blueprint.id,
            "prerequisite",
            "creator",
            None,
            None,
            &[],
            &[],
            None,
        )
        .expect("create prerequisite");
    let dependent = service
        .todo_create_legacy(
            &blueprint.id,
            "dependent",
            "creator",
            None,
            None,
            std::slice::from_ref(&prerequisite.id),
            &[],
            None,
        )
        .expect("create dependent");
    let blocked = service
        .todo_create_legacy(
            &blueprint.id,
            "blocked",
            "creator",
            None,
            Some("agent"),
            &[],
            &[],
            None,
        )
        .expect("create blocked Todo");
    service
        .todo_start(&blueprint.id, &blocked.id, None)
        .expect("start Todo");
    service
        .todo_block_legacy(
            &blueprint.id,
            &blocked.id,
            "external decision",
            "ask the user",
            None,
        )
        .expect("block Todo");
    let cancelled = service
        .todo_create_legacy(
            &blueprint.id,
            "cancelled",
            "creator",
            None,
            None,
            &[],
            &[],
            None,
        )
        .expect("create cancelled Todo");
    service
        .todo_cancel(&blueprint.id, &cancelled.id, "no longer needed", None)
        .expect("cancel Todo");

    let status = service
        .blueprint_status(&blueprint.id)
        .expect("Blueprint status");
    assert_eq!(status.ready_todos, vec![prerequisite.id.clone()]);
    assert_eq!(status.not_ready_todos.len(), 1);
    assert_eq!(status.not_ready_todos[0].id, dependent.id);
    assert_eq!(
        status.not_ready_todos[0].unsatisfied_dependencies[0].id,
        prerequisite.id
    );
    assert_eq!(status.blocked_todos, vec![blocked.id.clone()]);
    assert_eq!(status.unassigned_todos.len(), 2);
    assert!(status.unassigned_todos.contains(&prerequisite.id));
    assert!(status.unassigned_todos.contains(&dependent.id));
    assert_eq!(status.open_todos.len(), 3);
    assert!(status.open_todos.contains(&prerequisite.id));
    assert!(status.open_todos.contains(&dependent.id));
    assert!(status.open_todos.contains(&blocked.id));
    assert!(!status.open_todos.contains(&cancelled.id));
    assert_eq!(status.open_definition_of_done.len(), 1);
}

#[test]
fn incomplete_and_complete_closure_record_their_distinct_outcomes() {
    let (_directory, service, incomplete) = create_blueprint();
    let open = service
        .todo_create_legacy(
            &incomplete.id,
            "open",
            "creator",
            None,
            None,
            &[],
            &[],
            None,
        )
        .expect("create open Todo");
    let error = service
        .blueprint_close(&incomplete.id, "closer", None, None)
        .expect_err("incomplete close without reason must fail");
    assert!(error.to_string().contains("reason is required"));
    let open_dod = first_dod_id(&incomplete.source);
    let closed = service
        .blueprint_close(&incomplete.id, " closer ", Some(" user stopped "), None)
        .expect("close incomplete Blueprint");
    assert_eq!(closed.state, "closed");
    assert!(closed.source.contains("- Closed By: closer"));
    assert!(closed.source.contains("- Outcome: incomplete"));
    assert!(closed.source.contains("- Reason: user stopped"));
    assert!(closed.source.contains(&format!("  - {open_dod}")));
    assert!(closed.source.contains(&format!("  - {}", open.id)));

    let (directory, service, complete) = create_blueprint();
    let todo = service
        .todo_create_legacy(
            &complete.id,
            "finish",
            "creator",
            None,
            Some("agent"),
            &[],
            &[],
            None,
        )
        .expect("create Todo");
    service
        .todo_start(&complete.id, &todo.id, None)
        .expect("start Todo");
    service
        .todo_complete_legacy(&complete.id, &todo.id, "agent", "finished", None)
        .expect("complete Todo");
    service
        .dod_update(
            &complete.id,
            &first_dod_id(&complete.source),
            true,
            None,
            None,
        )
        .expect("complete DoD");
    let stored = service.blueprint_get(&complete.id).unwrap();
    fs::write(
        Utf8PathBuf::from_path_buf(directory.path().to_path_buf())
            .unwrap()
            .join(".blueprint/blueprints")
            .join(&complete.id)
            .join("blueprint.md"),
        stored.source.replace(
            "## Results\n\n## Evidence",
            "## Results\n\n- Evidence: [completion](#^evidence-close)\n\n## Evidence\n\n### Completion ^evidence-close",
        ),
    )
    .unwrap();
    let closed = service
        .blueprint_close(&complete.id, "closer", None, None)
        .expect("close complete Blueprint");
    assert!(closed.source.contains("- Outcome: complete"));
    assert!(!closed.source.contains("- Open Definition of Done:"));
    assert!(!closed.source.contains("- Open Todos:"));
}

#[test]
fn blueprint_cancel_records_actor_and_reason_then_moves_the_document() {
    let (_directory, service, blueprint) = create_blueprint();
    let cancelled = service
        .blueprint_cancel(&blueprint.id, " agent ", " abandoned direction ", None)
        .expect("cancel Blueprint");

    assert_eq!(cancelled.state, "cancelled");
    assert!(cancelled.source.contains("- Cancelled By: agent"));
    assert!(cancelled.source.contains("### Cancellation"));
    assert!(cancelled.source.contains("- Reason: abandoned direction"));
    assert!(service.blueprint_list("active").unwrap().is_empty());
    assert_eq!(
        service.blueprint_list("cancelled").unwrap(),
        vec![blueprint.id]
    );
}

#[test]
fn todo_assignment_requires_handoff_for_active_reassignment_and_rejects_terminal_work() {
    let (_directory, service, blueprint) = create_blueprint();
    let todo = service
        .todo_create_legacy(
            &blueprint.id,
            "assign",
            "creator",
            None,
            Some("agent-a"),
            &[],
            &[],
            None,
        )
        .expect("create Todo");
    let assigned = service
        .todo_assign(&blueprint.id, &todo.id, " agent-b ", None)
        .expect("assign pending Todo");
    assert_eq!(assigned.owner.as_deref(), Some("agent-b"));
    service
        .todo_start(&blueprint.id, &todo.id, None)
        .expect("start Todo");

    let error = service
        .todo_assign(&blueprint.id, &todo.id, "agent-c", None)
        .expect_err("active reassignment without Handoff must fail");
    assert!(error.to_string().contains("requires Handoff"));
    service
        .todo_update_legacy(
            &blueprint.id,
            &todo.id,
            None,
            None,
            None,
            Some(&["continue from checkpoint".into()]),
            None,
            None,
        )
        .expect("write Handoff");
    let reassigned = service
        .todo_assign(&blueprint.id, &todo.id, "agent-c", None)
        .expect("reassign with Handoff");
    assert_eq!(reassigned.owner.as_deref(), Some("agent-c"));
    service
        .todo_complete_legacy(&blueprint.id, &todo.id, "agent-c", "done", None)
        .expect("complete Todo");
    let error = service
        .todo_assign(&blueprint.id, &todo.id, "agent-d", None)
        .expect_err("completed Todo cannot be assigned");
    assert!(error.to_string().contains("cannot be assigned"));

    let cancelled = service
        .todo_create_legacy(
            &blueprint.id,
            "cancel",
            "creator",
            None,
            None,
            &[],
            &[],
            None,
        )
        .expect("create cancellable Todo");
    service
        .todo_cancel(&blueprint.id, &cancelled.id, "obsolete", None)
        .expect("cancel Todo");
    assert!(
        service
            .todo_assign(&blueprint.id, &cancelled.id, "agent", None)
            .is_err()
    );
}

#[test]
fn todo_start_requires_owner_pending_status_and_completed_dependencies() {
    let (_directory, service, blueprint) = create_blueprint();
    let unowned = service
        .todo_create_legacy(
            &blueprint.id,
            "unowned",
            "creator",
            None,
            None,
            &[],
            &[],
            None,
        )
        .expect("create unowned Todo");
    let error = service
        .todo_start(&blueprint.id, &unowned.id, None)
        .expect_err("unowned Todo must not start");
    assert!(error.to_string().contains("Owner"));

    let prerequisite = service
        .todo_create_legacy(
            &blueprint.id,
            "prerequisite",
            "creator",
            None,
            Some("agent"),
            &[],
            &[],
            None,
        )
        .expect("create prerequisite");
    let dependent = service
        .todo_create_legacy(
            &blueprint.id,
            "dependent",
            "creator",
            None,
            Some("agent"),
            std::slice::from_ref(&prerequisite.id),
            &[],
            None,
        )
        .expect("create dependent");
    let error = service
        .todo_start(&blueprint.id, &dependent.id, None)
        .expect_err("unsatisfied dependency must prevent start");
    assert!(error.to_string().contains("not ready"));
    service
        .todo_start(&blueprint.id, &prerequisite.id, None)
        .expect("start prerequisite");
    service
        .todo_complete_legacy(&blueprint.id, &prerequisite.id, "agent", "done", None)
        .expect("complete prerequisite");
    let started = service
        .todo_start(&blueprint.id, &dependent.id, None)
        .expect("start ready dependent");
    assert_eq!(started.status, TodoStatus::InProgress);
    let error = service
        .todo_start(&blueprint.id, &dependent.id, None)
        .expect_err("non-pending Todo cannot start again");
    assert!(error.to_string().contains("only pending"));
}

#[test]
fn todo_block_and_cancel_enforce_transition_fields_and_states() {
    let (_directory, service, blueprint) = create_blueprint();
    let todo = service
        .todo_create_legacy(
            &blueprint.id,
            "block",
            "creator",
            None,
            Some("agent"),
            &[],
            &[],
            None,
        )
        .expect("create Todo");
    assert!(
        service
            .todo_block_legacy(&blueprint.id, &todo.id, "reason", "handoff", None)
            .expect_err("pending Todo cannot be blocked")
            .to_string()
            .contains("only in_progress")
    );
    service
        .todo_start(&blueprint.id, &todo.id, None)
        .expect("start Todo");
    assert!(
        service
            .todo_block_legacy(&blueprint.id, &todo.id, " ", "handoff", None)
            .expect_err("empty reason must fail")
            .to_string()
            .contains("reason must not be empty")
    );
    assert!(
        service
            .todo_block_legacy(&blueprint.id, &todo.id, "reason", " ", None)
            .expect_err("empty Handoff must fail")
            .to_string()
            .contains("handoff must not be empty")
    );
    let blocked = service
        .todo_block_legacy(
            &blueprint.id,
            &todo.id,
            " external decision ",
            " resume after answer ",
            None,
        )
        .expect("block Todo");
    assert_eq!(blocked.status, TodoStatus::Blocked);
    assert_eq!(blocked.block_reason.as_deref(), Some("external decision"));
    assert_eq!(blocked.handoff, vec!["resume after answer"]);
    let cancelled = service
        .todo_cancel(&blueprint.id, &todo.id, " no longer needed ", None)
        .expect("cancel blocked Todo");
    assert_eq!(cancelled.status, TodoStatus::Cancelled);
    assert_eq!(cancelled.cancel_reason.as_deref(), Some("no longer needed"));
    assert!(
        service
            .todo_cancel(&blueprint.id, &todo.id, "again", None)
            .is_err()
    );
}

#[test]
fn todo_completion_requires_checked_criteria_and_terminal_children() {
    let (_directory, service, blueprint) = create_blueprint();
    let parent = service
        .todo_create_legacy(
            &blueprint.id,
            "parent",
            "creator",
            None,
            Some("agent"),
            &[],
            &["verified".into()],
            None,
        )
        .expect("create parent");
    let child = service
        .todo_create_legacy(
            &blueprint.id,
            "child",
            "creator",
            Some(&parent.id),
            None,
            &[],
            &[],
            None,
        )
        .expect("create child");
    assert!(
        service
            .todo_complete_legacy(&blueprint.id, &parent.id, "agent", "done", None)
            .expect_err("pending Todo cannot complete")
            .to_string()
            .contains("only in_progress")
    );
    service
        .todo_start(&blueprint.id, &parent.id, None)
        .expect("start parent");
    assert!(
        service
            .todo_complete_legacy(&blueprint.id, &parent.id, "agent", "done", None)
            .expect_err("unchecked criteria must prevent completion")
            .to_string()
            .contains("Completion Criteria")
    );
    service
        .todo_update_legacy(
            &blueprint.id,
            &parent.id,
            None,
            None,
            Some(&[CheckUpdate {
                text: "verified".into(),
                completed: true,
            }]),
            None,
            None,
            None,
        )
        .expect("check criterion");
    assert!(
        service
            .todo_complete_legacy(&blueprint.id, &parent.id, "agent", "done", None)
            .expect_err("open child must prevent completion")
            .to_string()
            .contains("child Todos")
    );
    service
        .todo_cancel(&blueprint.id, &child.id, "covered elsewhere", None)
        .expect("cancel child");
    let completed = service
        .todo_complete_legacy(&blueprint.id, &parent.id, " agent ", " finished ", None)
        .expect("complete parent");
    assert_eq!(completed.status, TodoStatus::Completed);
    assert_eq!(completed.completed_by.as_deref(), Some("agent"));
    assert_eq!(completed.result_summary.as_deref(), Some("finished"));
    assert_eq!(completed.owner.as_deref(), Some("agent"));
}

#[test]
fn todo_update_persists_allowed_fields_and_rejects_invalid_dependencies() {
    let (_directory, service, blueprint) = create_blueprint();
    let dependency = service
        .todo_create_legacy(
            &blueprint.id,
            "dependency",
            "creator",
            None,
            None,
            &[],
            &[],
            None,
        )
        .expect("create dependency");
    let todo = service
        .todo_create_legacy(
            &blueprint.id,
            "original",
            "creator",
            None,
            None,
            &[],
            &[],
            None,
        )
        .expect("create Todo");
    let updated = service
        .todo_update_legacy(
            &blueprint.id,
            &todo.id,
            Some(" renamed "),
            Some(std::slice::from_ref(&dependency.id)),
            Some(&[
                CheckUpdate {
                    text: "first".into(),
                    completed: true,
                },
                CheckUpdate {
                    text: "second".into(),
                    completed: false,
                },
            ]),
            Some(&["one".into(), "two".into()]),
            Some(" draft result "),
            None,
        )
        .expect("update Todo");
    assert_eq!(updated.title, "renamed");
    assert_eq!(updated.depends_on, vec![dependency.id.clone()]);
    assert_eq!(updated.handoff, vec!["one", "two"]);
    assert_eq!(updated.result_summary.as_deref(), Some("draft result"));
    assert_eq!(updated.completion_criteria.len(), 2);
    assert!(updated.completion_criteria[0].completed);
    assert!(!updated.completion_criteria[1].completed);

    let before = service
        .blueprint_get(&blueprint.id)
        .expect("before invalid update");
    let error = service
        .todo_update_legacy(
            &blueprint.id,
            &todo.id,
            None,
            Some(std::slice::from_ref(&todo.id)),
            None,
            None,
            None,
            Some(&before.etag),
        )
        .expect_err("self-dependency must fail");
    assert!(error.to_string().contains("cannot depend on itself"));
    assert_eq!(
        service.blueprint_get(&blueprint.id).unwrap().etag,
        before.etag
    );
    let error = service
        .todo_update_legacy(
            &blueprint.id,
            &todo.id,
            None,
            Some(&["todo-missing".into()]),
            None,
            None,
            None,
            None,
        )
        .expect_err("unknown dependency must fail");
    assert!(error.to_string().contains("unknown Todo"));
}

#[test]
fn todo_update_invalid_dependency_keeps_detail_and_graph_titles() {
    let (_directory, service, blueprint) = create_blueprint();
    let todo = service
        .todo_create_legacy(
            &blueprint.id,
            "original title",
            "creator",
            None,
            None,
            &[],
            &[],
            None,
        )
        .unwrap();

    for dependency in [todo.id.clone(), "todo-missing".into()] {
        assert!(
            service
                .todo_update_legacy(
                    &blueprint.id,
                    &todo.id,
                    Some("new title"),
                    Some(&[dependency]),
                    None,
                    None,
                    None,
                    None,
                )
                .is_err()
        );
        let blueprint_source = service.blueprint_get(&blueprint.id).unwrap();
        assert!(
            blueprint_source
                .source
                .contains(&format!("[original title](todos/{}.md)", todo.id))
        );
        let detail = blueprint_source
            .todos
            .into_iter()
            .find(|index| index.id == todo.id)
            .unwrap();
        assert!(
            fs::read_to_string(detail.path)
                .unwrap()
                .contains("# original title\n")
        );
    }
}

#[test]
fn todo_list_combines_status_owner_and_readiness_filters() {
    let (_directory, service, blueprint) = create_blueprint();
    let ready_a = service
        .todo_create_legacy(
            &blueprint.id,
            "ready a",
            "creator",
            None,
            Some("agent-a"),
            &[],
            &[],
            None,
        )
        .expect("create ready A");
    let prerequisite = service
        .todo_create_legacy(
            &blueprint.id,
            "prerequisite",
            "creator",
            None,
            Some("agent-b"),
            &[],
            &[],
            None,
        )
        .expect("create prerequisite");
    let waiting = service
        .todo_create_legacy(
            &blueprint.id,
            "waiting",
            "creator",
            None,
            Some("agent-a"),
            std::slice::from_ref(&prerequisite.id),
            &[],
            None,
        )
        .expect("create waiting Todo");
    let blocked = service
        .todo_create_legacy(
            &blueprint.id,
            "blocked",
            "creator",
            None,
            Some("agent-b"),
            &[],
            &[],
            None,
        )
        .expect("create blocked Todo");
    service
        .todo_start(&blueprint.id, &blocked.id, None)
        .expect("start blocked Todo");
    service
        .todo_block_legacy(&blueprint.id, &blocked.id, "reason", "handoff", None)
        .expect("block Todo");

    let filtered = service
        .todo_list(
            &blueprint.id,
            Some(TodoStatus::Pending),
            Some("agent-a"),
            Some(true),
        )
        .expect("combined filter");
    assert_eq!(
        filtered.iter().map(|todo| &todo.id).collect::<Vec<_>>(),
        vec![&ready_a.id]
    );
    let not_ready = service
        .todo_list(&blueprint.id, None, None, Some(false))
        .expect("not-ready filter");
    let not_ready_ids = not_ready
        .iter()
        .map(|todo| todo.id.as_str())
        .collect::<Vec<_>>();
    assert!(not_ready_ids.contains(&waiting.id.as_str()));
    assert!(not_ready_ids.contains(&blocked.id.as_str()));
    let blocked_only = service
        .todo_list(&blueprint.id, Some(TodoStatus::Blocked), None, None)
        .expect("status filter");
    assert_eq!(blocked_only[0].id, blocked.id);
}

#[test]
fn dod_update_toggles_only_the_selected_item_and_preserves_note() {
    let (_directory, service, blueprint) = create_blueprint();
    let dod_id = first_dod_id(&blueprint.source);
    let completed = service
        .dod_update(
            &blueprint.id,
            &dod_id,
            true,
            Some(" evidence recorded "),
            None,
        )
        .expect("complete DoD");
    assert!(
        completed
            .source
            .contains(&format!("- [x] 完成协议验证 ^{dod_id}"))
    );
    assert!(completed.source.contains("- Note: evidence recorded"));
    let reopened = service
        .dod_update(&blueprint.id, &dod_id, false, None, None)
        .expect("reopen DoD");
    assert!(
        reopened
            .source
            .contains(&format!("- [ ] 完成协议验证 ^{dod_id}"))
    );
    assert!(reopened.source.contains("- Note: evidence recorded"));

    let before = reopened.etag;
    let error = service
        .dod_update(&blueprint.id, "dod-missing", true, None, Some(&before))
        .expect_err("unknown DoD must fail");
    assert!(error.to_string().contains("unknown Definition of Done"));
    assert_eq!(service.blueprint_get(&blueprint.id).unwrap().etag, before);
}
