# Blueprint v2 实施计划

> **供执行 Agent 使用：** 必须使用 `superpowers:subagent-driven-development`（推荐）或 `superpowers:executing-plans`，逐任务执行本计划。所有步骤使用 checkbox 跟踪。

**目标：** 将现有单文件 Blueprint v1 升级为 Blueprint v2：稳定聚合目录、frontmatter 生命周期、中央 Todo Graph、独立 Todo 文档、Rubric、Evidence 和追加式 Revision History。

**架构：** `blueprint.md` 保存 Blueprint 级 Section 和 Todo Graph，`todos/todo-<id>.md` 保存较大的 Todo 内容；Todo task marker 是执行状态唯一事实来源。Source 层先提供通用一等 Section/Frontmatter 定位，Store 层提供 Blueprint 级锁与文档级 ETag，Service 层在其上组合领域校验和 MCP 用例。

**技术栈：** Rust 2024、`markdown` OFM/GFM AST、`serde`/`serde_json`、`camino`、`fs2`、`tempfile`、`rmcp`、内置 Rust test harness。

## 全局约束

- 仅实现 `blueprint/v2`；遇到 `blueprint/v1` 明确报 unsupported-schema，不迁移旧数据。
- 协议生成的文档引用一律使用标准 Markdown 相对链接，不使用 Wiki Link。
- Blueprint 路径固定为 `.blueprint/blueprints/bp-<id>/`，生命周期变化不移动目录。
- `blueprint.md` 中的 Obsidian task marker 是 Todo 状态唯一事实来源。
- Todo 文件不得重复保存 status、Owner、dependency、parent、Block Reason 或 Cancel Reason。
- 同一 Blueprint 内所有写操作共用 `.blueprint/.locks/bp-<id>.lock`。
- 修改必须基于 AST/源范围定点完成，未知 Section、字段、注释和排版必须保留。
- 不增加通用公共 `section_replace` MCP 工具。
- Rubric 只保存评估程序，不由 Blueprint MCP 执行。
- Evidence 只验证结构存在和内部引用可解析，不判断其语义真假或充分性。
- Revision History 追加而不替换；普通执行状态变化不自动产生 Revision。
- 不修改与 Blueprint 无关的用户文件或提交。

---

### Task 1：建立一等 Markdown Document 与 Section 模型

**Files:**
- Create: `src/blueprint/document.rs`
- Modify: `src/blueprint.rs`
- Modify: `src/blueprint/source.rs`
- Create: `src/blueprint/tests/document.rs`
- Modify: `src/blueprint/tests/mod.rs`

**Interfaces:**
- Consumes: `markdown::{Document, MarkdownNode, Parser, ParserOptions}`。
- Produces: `DocumentSchema`、`ParsedDocument::parse`、`ParsedDocument::section`、`replace_section`、`append_section`、`replace_frontmatter_field`，供后续 Blueprint/Todo parser 和 Store 使用。

- [ ] **Step 1：写出 Section、frontmatter 和保留源文本的失败测试**

在 `src/blueprint/tests/document.rs` 添加：

```rust
use super::super::document::{DocumentSchema, ParsedDocument};

const SCHEMA: DocumentSchema = DocumentSchema {
    name: "test/v2",
    required_sections: &["Intent", "Results"],
};

#[test]
fn parses_frontmatter_and_ordered_sections_then_preserves_unknown_markdown() {
    let source = "---\nschema: test/v2\nid: doc-1\nstate: active\n---\n\n# 标题\n\n## Intent\n\n目标\n\n## Extra\n\n<!-- keep -->\n\n## Results\n\n结果\n";
    let parsed = ParsedDocument::parse("doc.md", source, SCHEMA).unwrap();
    assert_eq!(parsed.frontmatter_string("schema"), Some("test/v2"));
    assert_eq!(parsed.h1(), "标题");
    assert_eq!(parsed.section("Intent").unwrap().body(source).trim(), "目标");

    let next = parsed.replace_section(source, "Results", "新结果").unwrap();
    assert!(next.contains("## Extra\n\n<!-- keep -->"));
    assert!(next.contains("## Results\n\n新结果"));
}

#[test]
fn rejects_missing_duplicate_sections_and_wrong_schema() {
    let missing = "---\nschema: test/v2\n---\n\n# T\n\n## Intent\n\nx\n";
    assert!(ParsedDocument::parse("doc.md", missing, SCHEMA)
        .unwrap_err().to_string().contains("missing required section: Results"));
    let duplicate = "---\nschema: test/v2\n---\n\n# T\n\n## Intent\n\na\n\n## Intent\n\nb\n\n## Results\n\nr\n";
    assert!(ParsedDocument::parse("doc.md", duplicate, SCHEMA)
        .unwrap_err().to_string().contains("required section must occur exactly once: Intent"));
}
```

并在 `src/blueprint/tests/mod.rs` 增加 `mod document;`。

- [ ] **Step 2：运行测试并确认红灯**

Run: `cargo test blueprint::tests::document -- --nocapture`

Expected: FAIL，提示 `blueprint::document` 不存在。

- [ ] **Step 3：实现最小的一等 Document/Section API**

在 `src/blueprint.rs` 增加 `mod document;`。在 `document.rs` 定义：

```rust
#[derive(Clone, Copy)]
pub(crate) struct DocumentSchema {
    pub name: &'static str,
    pub required_sections: &'static [&'static str],
}

#[derive(Clone, Debug)]
pub(crate) struct Section {
    pub title: String,
    heading_start: usize,
    body_start: usize,
    body_end: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct ParsedDocument {
    h1: String,
    frontmatter: serde_json::Map<String, serde_json::Value>,
    frontmatter_range: std::ops::Range<usize>,
    sections: Vec<Section>,
}

impl ParsedDocument {
    pub(crate) fn parse(path: &str, source: &str, schema: DocumentSchema) -> anyhow::Result<Self>;
    pub(crate) fn h1(&self) -> &str;
    pub(crate) fn frontmatter_string(&self, field: &str) -> Option<&str>;
    pub(crate) fn section(&self, title: &str) -> anyhow::Result<&Section>;
    pub(crate) fn replace_section(&self, source: &str, title: &str, body: &str) -> anyhow::Result<String>;
    pub(crate) fn append_section(&self, source: &str, title: &str, body: &str) -> anyhow::Result<String>;
    pub(crate) fn replace_frontmatter_field(&self, source: &str, field: &str, value: &str) -> anyhow::Result<String>;
}

impl Section {
    pub(crate) fn body<'a>(&self, source: &'a str) -> &'a str;
}
```

复用 OFM AST 的 `FrontMatter`、H1/H2 节点和源码行列；将行列转换为 byte range。解析时验证 `.md`、H1、frontmatter schema、必需 Section 存在且唯一。将 `source.rs` 中重复的 H2 搜索迁移到该模块。

- [ ] **Step 4：运行聚焦测试并修正格式**

Run: `cargo fmt --all && cargo test blueprint::tests -- --nocapture`

Expected: 新 document 测试和原 source 测试全部 PASS。

- [ ] **Step 5：提交 Task 1**

```bash
git add src/blueprint.rs src/blueprint/document.rs src/blueprint/source.rs src/blueprint/tests/document.rs src/blueprint/tests/mod.rs
git commit -m "refactor(blueprint): model markdown sections"
```

---

### Task 2：定义 v2 Blueprint、Todo Document、Evidence 与 Revision 模型

**Files:**
- Modify: `src/blueprint/model.rs`
- Modify: `src/blueprint/source.rs`
- Modify: `src/blueprint/todo.rs`
- Modify: `src/blueprint.rs`
- Modify: `src/blueprint/tests/source.rs`
- Create: `src/blueprint/tests/todo.rs`
- Modify: `src/blueprint/tests/mod.rs`

**Interfaces:**
- Consumes: Task 1 的 `ParsedDocument` 和 `DocumentSchema`。
- Produces: `BlueprintState`、`BlueprintSource`、`TodoDetail`、`EvidenceItem`、`RevisionEntry`、`parse_blueprint_source`、`parse_todo_source`，供 Store/Service 使用。

- [ ] **Step 1：写出 v2 Blueprint 与 Todo 文档解析失败测试**

将 `source.rs` fixture 改为包含 frontmatter 和 11 个 Blueprint Section，并添加：

```rust
#[test]
fn parses_v2_graph_links_without_loading_todo_details() {
    let source = blueprint_v2(
        "- [/] [完成初稿](todos/todo-draft.md) ^todo-draft\n  - Created By: planner\n  - Owner: writer\n",
    );
    let parsed = BlueprintSource::parse("bp-01/blueprint.md", &source).unwrap();
    assert_eq!(parsed.state, BlueprintState::Active);
    assert_eq!(parsed.todos[0].id, "todo-draft");
    assert_eq!(parsed.todos[0].document, "todos/todo-draft.md");
    assert_eq!(parsed.todos[0].status, TodoStatus::InProgress);
}
```

在 `tests/todo.rs` 添加：

```rust
#[test]
fn parses_todo_sections_without_duplicating_graph_state() {
    let source = todo_v2("todo-review", "bp-01", "- [x] 已检查前后段落", "评估完成", evidence());
    let detail = TodoDetail::parse("todos/todo-review.md", &source).unwrap();
    assert_eq!(detail.id, "todo-review");
    assert!(detail.completion_criteria.iter().all(|item| item.completed));
    assert_eq!(detail.results, "评估完成");
    assert_eq!(detail.evidence.len(), 1);
}
```

- [ ] **Step 2：运行测试并确认旧 parser 无法满足新协议**

Run: `cargo test blueprint::tests -- --nocapture`

Expected: FAIL，缺少 `BlueprintSource`、`TodoDetail` 和 v2 frontmatter/链接解析。

- [ ] **Step 3：实现 v2 领域模型和两个 typed parser**

在 `model.rs` 增加并导出：

```rust
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BlueprintState { Active, Closed, Cancelled }

#[derive(Clone, Debug, Serialize, JsonSchema, PartialEq, Eq)]
pub struct TodoGraphNode {
    pub id: String,
    pub title: String,
    pub document: String,
    pub status: TodoStatus,
    pub created_by: Option<String>,
    pub owner: Option<String>,
    pub completed_by: Option<String>,
    pub depends_on: Vec<String>,
    pub block_reason: Option<String>,
    pub cancel_reason: Option<String>,
    pub children: Vec<TodoGraphNode>,
}

#[derive(Clone, Debug, Serialize, JsonSchema, PartialEq, Eq)]
pub struct EvidenceItem { pub id: String, pub markdown: String }

#[derive(Clone, Debug, Serialize, JsonSchema, PartialEq, Eq)]
pub struct RevisionEntry { pub id: String, pub markdown: String }

#[derive(Clone, Debug, Serialize, JsonSchema, PartialEq, Eq)]
pub struct TodoDetail {
    pub id: String,
    pub blueprint_id: String,
    pub title: String,
    pub intent: String,
    pub completion_criteria: Vec<CheckItem>,
    pub plan: String,
    pub handoff: String,
    pub results: String,
    pub evidence: Vec<EvidenceItem>,
    pub revisions: Vec<RevisionEntry>,
    pub notes: String,
}
```

`source.rs` 使用 Blueprint schema 解析 frontmatter、Rubric、Todo 标准 Markdown link 和中央字段；`todo.rs` 使用 Todo schema 解析独立文档。Evidence/Revision 通过 `^evidence-*`、`^revision-*` Block ID 识别，正文保留原始 Markdown。

- [ ] **Step 4：验证解析、旧未知 Markdown 保留和 model schema**

Run: `cargo fmt --all && cargo test blueprint::tests -- --nocapture`

Expected: PASS；旧 validate 测试按 `TodoGraphNode + TodoDetail` 新职责更新后仍覆盖依赖环和状态不变量。

- [ ] **Step 5：提交 Task 2**

```bash
git add src/blueprint.rs src/blueprint/model.rs src/blueprint/source.rs src/blueprint/todo.rs src/blueprint/tests/source.rs src/blueprint/tests/todo.rs src/blueprint/tests/mod.rs src/blueprint/tests/validate.rs
git commit -m "feat(blueprint): parse v2 aggregate documents"
```

---

### Task 3：将 Store 升级为稳定聚合目录和文档级 ETag

**Files:**
- Modify: `src/blueprint/store.rs`
- Modify: `src/blueprint/tests/store.rs`

**Interfaces:**
- Consumes: Task 2 的 `BlueprintState`、`BlueprintSource`、`TodoDetail`。
- Produces: `StoredBlueprint`、`StoredTodo`、`BlueprintStore::{create, read, list, write_blueprint, create_todo, read_todo, write_todo, set_state, with_lock}`。

- [ ] **Step 1：将 Store 测试改写为 v2 聚合布局**

添加以下核心断言：

```rust
#[test]
fn creates_v2_aggregate_and_keeps_path_stable_across_state_changes() {
    let (_dir, store) = store();
    let created = store.create("bp-01", &blueprint_source()).unwrap();
    assert!(store.workspace_root().join("blueprints/bp-01/blueprint.md").is_file());
    assert!(store.workspace_root().join("blueprints/bp-01/todos").is_dir());
    let closed = store.set_state("bp-01", BlueprintState::Closed, Some(&created.etag)).unwrap();
    assert_eq!(closed.state, BlueprintState::Closed);
    assert!(store.workspace_root().join("blueprints/bp-01/blueprint.md").is_file());
}

#[test]
fn stores_todo_documents_with_independent_etags_and_detects_orphans() {
    let (_dir, store) = store_with_blueprint();
    let todo = store.create_todo("bp-01", "todo-a", &todo_source()).unwrap();
    assert_ne!(store.read("bp-01").unwrap().etag, todo.etag);
    std::fs::write(store.todo_path("bp-01", "todo-orphan"), todo_source()).unwrap();
    assert!(store.validate_aggregate("bp-01").unwrap_err().to_string().contains("orphan Todo document"));
}
```

- [ ] **Step 2：运行 Store 测试确认红灯**

Run: `cargo test blueprint::tests::store -- --nocapture`

Expected: FAIL，旧 Store 仍创建 `active/bp-01.md`。

- [ ] **Step 3：实现 v2 路径、锁、ETag 和一致性检查**

将 manifest 改为 `blueprint/v2`，工作区只创建 `blueprints`、`.locks`、`.tmp`。定义：

```rust
pub struct StoredBlueprint {
    pub id: String,
    pub state: BlueprintState,
    pub path: Utf8PathBuf,
    pub etag: String,
    pub source: String,
    pub todos: Vec<StoredTodoIndex>,
}

pub struct StoredTodo {
    pub blueprint_id: String,
    pub id: String,
    pub path: Utf8PathBuf,
    pub etag: String,
    pub source: String,
}
```

路径固定为 `blueprints/{bp}/blueprint.md` 和 `blueprints/{bp}/todos/{todo}.md`。`list(state)` 遍历 Blueprint 目录并读取 frontmatter。`set_state` 只定点修改 frontmatter。`validate_aggregate` 比较中央图链接集合和 `todos/*.md` 集合，分别报告 missing/orphan。

- [ ] **Step 4：运行 Store 与 source 聚焦测试，并记录 Service 迁移断点**

Run: `cargo fmt --all && cargo test blueprint::tests -- --nocapture`

Expected: PASS；v1 manifest 测试改为期待 `unsupported Blueprint workspace schema: blueprint/v1`。

Run: `cargo test blueprint::tests -- --nocapture`

Expected: 在 Task 4 完成前，允许旧 Service 因拒绝 v1 写入而失败；必须记录失败测试名称和统一原因，禁止通过恢复 v1 写入使其通过。Task 4 的验收门槛是将这里记录的全部失败恢复为 PASS。

- [ ] **Step 5：提交 Task 3**

```bash
git add src/blueprint/store.rs src/blueprint/tests/store.rs
git commit -m "feat(blueprint): store v2 document aggregates"
```

---

### Task 4：实现 Blueprint 创建、读取、更新、Rubric 和生命周期

**Files:**
- Modify: `src/blueprint/model.rs`
- Modify: `src/blueprint/service.rs`
- Modify: `src/blueprint/todo.rs`
- Modify: `src/blueprint/validate.rs`
- Modify: `src/blueprint/tests/service.rs`
- Modify: `src/blueprint/tests/validate.rs`

**Interfaces:**
- Consumes: Task 1-3 的 Document parser、Blueprint parser 和 aggregate Store。
- Produces: v2 `BlueprintCreateInput`、`BlueprintUpdateInput`、`BlueprintGetOutput`，以及全部现有 `BlueprintService` Blueprint/DoD/Todo 方法基于 v2 Store 的可运行基线；Task 3 记录的 18 项 legacy Service 失败必须全部恢复。

- [ ] **Step 1：写 Blueprint v2 service 红灯测试**

更新 fixture，使 create 输入包含 `rubric`，并添加：

```rust
#[test]
fn create_writes_frontmatter_rubric_evidence_and_revision_sections() {
    let (_dir, service, created) = create_blueprint();
    assert!(created.source.starts_with("---\nschema: blueprint/v2\n"));
    assert!(created.source.contains("state: active"));
    assert!(created.source.contains("## Rubric\n\n按目标和约束进行评估"));
    assert!(created.source.contains("## Evidence\n"));
    assert!(created.source.contains("## Revision History\n"));
    assert!(created.path.ends_with(format!("blueprints/{}/blueprint.md", created.id)));
}

#[test]
fn semantic_update_requires_and_appends_revision() {
    let (_dir, service, created) = create_blueprint();
    let error = service.blueprint_update(&created.id, BlueprintPatch { intent: Some("新目标".into()), ..Default::default() }, None, None, None).unwrap_err();
    assert!(error.to_string().contains("changed_by and change_reason are required"));
    let updated = service.blueprint_update(&created.id, BlueprintPatch { intent: Some("新目标".into()), ..Default::default() }, Some("agent"), Some("用户调整方向"), Some(&created.etag)).unwrap();
    assert!(updated.source.contains("## Revision History"));
    assert!(updated.source.contains("- Reason: 用户调整方向"));
}
```

- [ ] **Step 2：运行 service 测试确认红灯**

Run: `cargo test blueprint::tests::service -- --nocapture`

Expected: FAIL，create 输入无 Rubric，service 仍渲染 v1。

- [ ] **Step 3：实现 v2 Blueprint 用例**

将创建输入改为：

```rust
pub struct BlueprintCreateInput {
    pub title: String,
    pub created_by: String,
    pub intent: String,
    #[serde(default)] pub constraints: Vec<String>,
    pub definition_of_done: Vec<String>,
    pub plan: String,
    pub rubric: String,
}
```

引入 `BlueprintPatch` 收纳可更新 Section，并将创建结果定义为 `BlueprintCreated { id: String, path: Utf8PathBuf, etag: String, source: String }`。Intent、Constraints、Plan、Rubric 变化必须同时追加 Revision；Results/Notes 普通更新不自动追加。close/cancel 在同一稳定路径更新 Record、Results、Revision History 和 frontmatter state。resume 增加 Rubric 和 Todo index，full 不拼接全部 Todo source。

同时迁移所有旧 Service 存储调用：不得继续调用 Store 的 v1 `read_active`、`write` 或 `move_to` 隔离入口。Todo 方法在此任务至少建立 v2 可运行基线：通过中央 `BlueprintSource` 读取状态/依赖，通过独立 Todo 文件读取 Completion Criteria、Handoff 和 Results，并使用 Blueprint 锁内 guard 完成写入。Task 5 再增加新 v2 专属请求模型、双 ETag 和更完整的跨文档更新测试，但不能把任何现有 Service 测试失败留到 Task 5。

- [ ] **Step 4：运行全部 Blueprint service 生命周期测试**

Run: `cargo fmt --all && cargo test blueprint::tests::service -- --nocapture`

Expected: 全部 Blueprint 测试 PASS，Task 3 记录的 18 项 legacy Service 失败全部恢复；不得把 Todo Service 失败推迟到 Task 5。

- [ ] **Step 5：提交 Task 4**

```bash
git add src/blueprint/model.rs src/blueprint/service.rs src/blueprint/tests/service.rs
git commit -m "feat(blueprint): add v2 rubric and lifecycle metadata"
```

---

### Task 5：实现中央 Todo Graph 与独立 Todo 文档联动

**Files:**
- Modify: `src/blueprint/model.rs`
- Modify: `src/blueprint/service.rs`
- Modify: `src/blueprint/todo.rs`
- Modify: `src/blueprint/validate.rs`
- Modify: `src/blueprint/tests/service.rs`
- Modify: `src/blueprint/tests/validate.rs`

**Interfaces:**
- Consumes: Task 2 的 `TodoGraphNode`/`TodoDetail` 和 Task 3 Store。
- Produces: `TodoView`、`TodoPatch`、`todo_create/get/list/update/assign/start/block/cancel` 的 v2 行为。

- [ ] **Step 1：写 Todo 文件创建与组合读取红灯测试**

```rust
#[test]
fn todo_create_writes_graph_link_and_independent_detail_document() {
    let (_dir, service, blueprint) = create_blueprint();
    let todo = service.todo_create(TodoCreateRequest {
        blueprint_id: blueprint.id.clone(),
        title: "检查连续性".into(),
        created_by: "planner".into(),
        intent: "检查人物行为变化".into(),
        plan: "运行连续性评估".into(),
        completion_criteria: vec!["引用关键段落".into()],
        ..Default::default()
    }).unwrap();
    let blueprint_source = service.blueprint_get(&blueprint.id).unwrap().source;
    assert!(blueprint_source.contains(&format!("[检查连续性](todos/{}.md) ^{}", todo.id, todo.id)));
    let fetched = service.todo_get(&blueprint.id, &todo.id).unwrap();
    assert_eq!(fetched.detail.intent, "检查人物行为变化");
    assert_eq!(fetched.graph.status, TodoStatus::Pending);
}
```

- [ ] **Step 2：运行聚焦测试确认红灯**

Run: `cargo test blueprint::tests::service::todo_create_writes_graph_link_and_independent_detail_document -- --exact`

Expected: FAIL，旧 `todo_create` 仍内嵌全部字段且不创建文件。

- [ ] **Step 3：实现 TodoView、双文档 patch 和安全写入顺序**

定义：

```rust
pub struct TodoView {
    pub graph: TodoGraphNode,
    pub detail: TodoDetail,
    pub blueprint_etag: String,
    pub todo_etag: String,
    pub ready: bool,
    pub unsatisfied_dependencies: Vec<DependencyStatus>,
}

#[derive(Default)]
pub struct TodoCreateRequest {
    pub blueprint_id: String,
    pub title: String,
    pub created_by: String,
    pub intent: String,
    pub plan: String,
    pub parent_id: Option<String>,
    pub owner: Option<String>,
    pub depends_on: Vec<String>,
    pub completion_criteria: Vec<String>,
    pub expected_blueprint_etag: Option<String>,
}

#[derive(Default)]
pub struct TodoPatch {
    pub title: Option<String>,
    pub depends_on: Option<Vec<String>>,
    pub intent: Option<String>,
    pub completion_criteria: Option<Vec<CheckUpdate>>,
    pub plan: Option<String>,
    pub handoff: Option<String>,
    pub results: Option<String>,
    pub notes: Option<String>,
}
```

`todo_create` 在 Blueprint 锁内先写详情文件，再插入中央链接；失败时删除新文件。`todo_get/list` 组合 graph/detail。修改 title 时同步中央 link label 和 Todo H1；修改 dependency/intent/criteria/plan 时要求 revision 元数据并追加 Todo Revision。assign/start/block/cancel 只改中央状态字段，block 同时写 Todo Handoff。

- [ ] **Step 4：运行 Todo graph、service 和 validate 测试**

Run: `cargo fmt --all && cargo test blueprint::tests -- --nocapture`

Expected: Todo 创建、读取、过滤、分配、start、block、cancel、依赖与父子校验全部 PASS。

- [ ] **Step 5：提交 Task 5**

```bash
git add src/blueprint/model.rs src/blueprint/service.rs src/blueprint/todo.rs src/blueprint/validate.rs src/blueprint/tests/service.rs src/blueprint/tests/validate.rs
git commit -m "feat(blueprint): split todo graph and detail documents"
```

---

### Task 6：实现 Evidence、Revision 追加操作与完成门槛

**Files:**
- Modify: `src/blueprint/model.rs`
- Modify: `src/blueprint/service.rs`
- Modify: `src/blueprint/source.rs`
- Modify: `src/blueprint/todo.rs`
- Modify: `src/blueprint/tests/service.rs`
- Modify: `src/blueprint/tests/source.rs`
- Modify: `src/blueprint/tests/todo.rs`

**Interfaces:**
- Consumes: Task 2 的 Evidence/Revision parser、Task 3 文档 ETag、Task 5 TodoView。
- Produces: `EvidenceAddInput`、`RevisionAppendInput`、`evidence_add`、`revision_append`、Evidence link resolver 和 v2 `todo_complete`。

- [ ] **Step 1：写跨 Todo Evidence 与完成门槛红灯测试**

```rust
#[test]
fn todo_complete_requires_results_and_resolved_evidence() {
    let scenario = started_todo_with_checked_criteria();
    let error = scenario.service.todo_complete(&scenario.blueprint_id, &scenario.todo_id, "agent", None, None).unwrap_err();
    assert!(error.to_string().contains("Evidence"));

    let evidence = scenario.service.evidence_add(EvidenceAddInput {
        blueprint_id: scenario.blueprint_id.clone(),
        todo_id: Some(scenario.todo_id.clone()),
        title: "测试输出".into(),
        markdown: "- Observation: 所有场景已检查".into(),
        expected_etag: None,
    }).unwrap();
    let completed = scenario.service.todo_complete(&scenario.blueprint_id, &scenario.todo_id, "agent", Some("检查完成"), None).unwrap();
    assert_eq!(completed.graph.status, TodoStatus::Completed);
    assert!(completed.detail.evidence.iter().any(|item| item.id == evidence.id));
}

#[test]
fn evidence_links_resolve_across_todo_documents_using_markdown_links() {
    let scenario = two_todos();
    let evidence = scenario.add_evidence_to_first();
    scenario.add_evidence_to_second_with_body(&format!("- Related: [前置证据](todo-a.md#^{})", evidence.id)).unwrap();
    scenario.service.blueprint_status(&scenario.blueprint_id).unwrap();
}
```

- [ ] **Step 2：运行测试确认红灯**

Run: `cargo test blueprint::tests::service -- --nocapture`

Expected: FAIL，缺少 evidence 操作与聚合引用解析。

- [ ] **Step 3：实现追加式 Evidence/Revision 和完成校验**

定义 scope 输入：

```rust
pub struct EvidenceAddInput {
    pub blueprint_id: String,
    pub todo_id: Option<String>,
    pub title: String,
    pub markdown: String,
    pub expected_etag: Option<String>,
}

pub struct RevisionAppendInput {
    pub blueprint_id: String,
    pub todo_id: Option<String>,
    pub changed_by: String,
    pub reason: String,
    pub change: String,
    pub affected: Vec<String>,
    pub evidence_impact: Option<String>,
    pub expected_etag: Option<String>,
}
```

`evidence_add` 生成 `evidence-{ULID}` 并向目标 Evidence Section 追加 H3 和正文；`revision_append` 生成 `revision-{ULID}` 并追加固定字段。扫描 Blueprint 和全部 Todo 文档保证 ID 全局唯一；仅解析 `.blueprint/blueprints/{bp}` 内的标准 Markdown Evidence 链接，校验目标文件与 Block ID。

`todo_complete` 从 Todo 文件读取 Criteria、Results、Evidence，先确保内容有效，再更新中央 marker 和 Completed By。完整 `blueprint_close` 额外要求 Blueprint Results 中至少一个可解析 Evidence 引用；不完整关闭继续要求 reason。

- [ ] **Step 4：运行 Evidence、Revision、完成与关闭测试**

Run: `cargo fmt --all && cargo test blueprint::tests -- --nocapture`

Expected: 全部 Blueprint 单元测试 PASS，包括 Evidence 全局重复、内部坏链接、Revision 追加不可覆盖和完成门槛。

- [ ] **Step 5：提交 Task 6**

```bash
git add src/blueprint/model.rs src/blueprint/service.rs src/blueprint/source.rs src/blueprint/todo.rs src/blueprint/tests/service.rs src/blueprint/tests/source.rs src/blueprint/tests/todo.rs
git commit -m "feat(blueprint): add evidence and revision history"
```

---

### Task 7：升级 MCP、CLI 输入输出与工具文档

**Files:**
- Modify: `src/blueprint.rs`
- Modify: `src/blueprint/model.rs`
- Modify: `src/blueprint/mcp.rs`
- Modify: `src/blueprint/tests/mcp.rs`
- Modify: `src/cli/commands.rs`
- Modify: `docs/tools.md`

**Interfaces:**
- Consumes: Task 4-6 的 v2 Service API。
- Produces: 19 个 Blueprint MCP tools、具体 JSON schema、对应 CLI direct-operation 参数和生成文档。

- [ ] **Step 1：先更新 MCP 契约测试**

将工具集合断言更新为 19 项，并增加：

```rust
for expected in ["evidence_add", "revision_append"] {
    assert!(names.iter().any(|name| name == expected));
}
let create = tool(&tools, "blueprint_create");
assert!(properties(create).contains_key("rubric"));
let todo_update = tool(&tools, "todo_update");
assert!(properties(todo_update).contains_key("expected_blueprint_etag"));
assert!(properties(todo_update).contains_key("expected_todo_etag"));
```

- [ ] **Step 2：运行 MCP 测试确认红灯**

Run: `cargo test blueprint::tests::mcp -- --nocapture`

Expected: FAIL，工具仍为 17 项且 schema 缺少 v2 字段。

- [ ] **Step 3：更新 request/response、router 和 CLI**

在 `blueprint.rs` 导出新增 DTO；在 `mcp.rs` 添加 `evidence_add`、`revision_append` 薄转发。所有触及 Todo 图和详情的输入分别暴露 `expected_blueprint_etag`、`expected_todo_etag`。工具描述明确：Rubric 由 Agent 执行、Evidence 只做结构校验、Revision 追加不可覆盖。

同步 `src/cli/commands.rs` 的 Blueprint direct operation 参数和转发。不得在 CLI 中重复领域逻辑。

- [ ] **Step 4：生成并校验工具文档**

Run: `cargo run -- generate-docs --output docs/tools.md`

Expected: 命令成功，`docs/tools.md` 包含 19 个 Blueprint tools、Rubric/Evidence/Revision 和双 ETag schema。

Run: `cargo test blueprint::tests::mcp docs::tests -- --nocapture`

Expected: PASS。

- [ ] **Step 5：提交 Task 7**

```bash
git add src/blueprint.rs src/blueprint/model.rs src/blueprint/mcp.rs src/blueprint/tests/mcp.rs src/cli/commands.rs docs/tools.md
git commit -m "feat(blueprint): expose v2 mcp contract"
```

---

### Task 8：全量验证、语义复核和清理

**Files:**
- Review: `src/blueprint.rs`
- Review: `src/blueprint/document.rs`
- Review: `src/blueprint/model.rs`
- Review: `src/blueprint/source.rs`
- Review: `src/blueprint/todo.rs`
- Review: `src/blueprint/store.rs`
- Review: `src/blueprint/service.rs`
- Review: `src/blueprint/validate.rs`
- Review: `src/blueprint/mcp.rs`
- Review: `src/blueprint/tests/*.rs`
- Review: `src/cli/commands.rs`
- Review: `docs/tools.md`
- Review: `docs/superpowers/specs/2026-07-19-blueprint-v2-design.md`

**Interfaces:**
- Consumes: Task 1-7 的完整 v2 实现。
- Produces: 无警告、全测试通过、设计与实现一致的最终分支。

- [ ] **Step 1：运行格式和编译检查**

Run: `cargo fmt --all -- --check`

Expected: exit 0，无格式差异。

Run: `cargo check --all-targets`

Expected: exit 0，无编译错误。

- [ ] **Step 2：运行严格 Clippy**

Run: `cargo clippy --all-targets -- -D warnings`

Expected: exit 0，无 warning。

- [ ] **Step 3：运行完整测试**

Run: `cargo test --no-fail-fast`

Expected: 全部测试 PASS，0 failed。

- [ ] **Step 4：验证生成文档无漂移**

Run: `cargo run -- generate-docs --output docs/tools.md --check`

Expected: exit 0，生成文档与工具 schema 一致。

- [ ] **Step 5：按设计做人工语义矩阵复核**

逐项确认：

```text
稳定 Blueprint 目录                         已有测试
frontmatter lifecycle                      已有测试
中央 Todo marker 是唯一执行状态              已有测试
Todo 独立详情文档且不重复图字段                已有测试
Rubric 非空并进入 resume                     已有测试
标准 Markdown links，无协议 Wiki Link         已有测试或 source 断言
Evidence 跨 Todo 引用及完成门槛                已有测试
Revision History 追加且语义修改必填原因         已有测试
Blueprint/Todo 独立 ETag                      已有测试
未知 Markdown 定点保留                        已有测试
```

发现缺口时，先添加能观察协议行为的失败测试，再修复实现；不得只增加覆盖率断言。

- [ ] **Step 6：提交验证阶段必要修正**

若 Step 1-5 产生修正：

```bash
git add src/blueprint src/cli/commands.rs docs/tools.md
git commit -m "test(blueprint): verify v2 protocol"
```

若没有文件变化，不创建空提交。
