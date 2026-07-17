# Blueprint MCP v0.1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在当前 Obsidian Vault 根目录中实现完整的 Blueprint v0.1 MCP 协议，并在首次使用时自动建立工作区。

**Architecture:** `src/blueprint` 封装协议的所有领域模型、AST 映射、校验、存储和用例。`source.rs` 复用现有 Obsidian Markdown AST 定位节点及其源范围，不实现第二个解析器；`store.rs` 在当前 Vault 根目录自动确保工作区存在，并负责锁、ETag、原子写入和生命周期路径；`service.rs` 负责全部状态机与图规则。`server.rs` 只承载 MCP schema 和委托调用。

**Tech Stack:** Rust 2024、现有 `markdown` AST、rmcp 2.2、serde/schemars、`fs2` 文件锁、`ulid` ID、tempfile。

## Global Constraints

- Blueprint 专用生产代码必须位于 `src/blueprint/`。
- 复用现有 `markdown` AST 识别标题、任务、Block ID 和节点层级；不得重写 Markdown 解析器。
- 修改只能替换或插入目标 AST 节点的源文本范围，必须保留未知 Markdown 内容及其格式。
- 每次读写都验证；写入必须锁定单个 Blueprint、重读、校验可选 ETag、临时写入并原子替换。
- Blueprint、Todo、DoD ID 分别采用 `bp-`、`todo-`、`dod-` 前缀的 ULID。
- DoD 与 Todo 状态不自动互相更新；readiness 仅从 pending Todo 的依赖派生。
- 按 TDD 执行：先写失败测试、确认失败、最小实现、确认通过；每个任务独立提交。

---

## File Structure

- Create: `src/blueprint/mod.rs` — 对外导出 `BlueprintService`、请求/响应模型以及测试模块。
- Create: `src/blueprint/model.rs` — 协议领域类型、序列化响应、请求共享枚举。
- Create: `src/blueprint/source.rs` — 由 `markdown::Parser` AST 构建 Blueprint 语义模型，并生成定点源补丁。
- Create: `src/blueprint/validate.rs` — Blueprint、ID、Todo 图与状态不变量校验。
- Create: `src/blueprint/store.rs` — 工作区发现、文件读写、ETag、锁与生命周期移动。
- Create: `src/blueprint/service.rs` — 17 项用例和派生执行状态。
- Create: `src/blueprint/tests/mod.rs` — 共用 fixture 和子模块入口。
- Create: `src/blueprint/tests/source.rs` — AST 映射与保真补丁测试。
- Create: `src/blueprint/tests/validate.rs` — 结构、依赖和状态校验测试。
- Create: `src/blueprint/tests/service.rs` — 生命周期、Todo 状态机、ETag 和并发语义测试。
- Modify: `Cargo.toml` — 添加 `fs2` 与 `ulid`。
- Modify: `src/main.rs` — 注册模块，并删除不再需要的 `blueprint` 占位命令。
- Modify: `src/server.rs` — 定义 17 个请求 schema 与薄路由。
- Modify: `src/server/tests.rs` — 扩展公开工具列表和调用测试。
- Modify: `docs/tools.md` — 由工具生成器重新生成。

## Task 1: 建立 Blueprint 类型、依赖与测试夹具

**Files:**
- Modify: `Cargo.toml`
- Create: `src/blueprint/mod.rs`
- Create: `src/blueprint/model.rs`
- Create: `src/blueprint/tests/mod.rs`
- Test: `src/blueprint/tests/validate.rs`

**Consumes:** 现有 `Vault` 的 vault 根目录与 `schemars`/`serde` 约定。

**Produces:** `BlueprintState`、`TodoStatus`、`Blueprint`、`Todo`、`DefinitionOfDone`、`ExecutionState` 及所有操作所需的强类型字段。

- [ ] **Step 1: 写失败测试，锁定协议状态字符串与 DoD/Todo 模型**

```rust
#[test]
fn serializes_protocol_statuses() {
    assert_eq!(serde_json::to_string(&TodoStatus::Pending).unwrap(), "\"pending\"");
    assert_eq!(serde_json::to_string(&TodoStatus::InProgress).unwrap(), "\"in_progress\"");
    assert_eq!(TodoStatus::Blocked.marker(), "?");
}
```

- [ ] **Step 2: 运行测试，确认因模块不存在而失败**

运行：`cargo test blueprint::tests::validate::serializes_protocol_statuses -- --exact`

预期：失败，提示 `blueprint` 模块或 `TodoStatus` 未定义。

- [ ] **Step 3: 添加依赖和最小领域模型**

在 `Cargo.toml` 加入：

```toml
fs2 = "0.4"
ulid = "1.2"
```

在 `model.rs` 定义以下公共类型，所有枚举以 `snake_case` 序列化：

```rust
pub enum BlueprintState { Active, Closed, Cancelled }
pub enum TodoStatus { Pending, InProgress, Completed, Blocked, Cancelled }
pub struct DefinitionOfDone { pub id: String, pub text: String, pub completed: bool, pub note: Option<String> }
pub struct Todo { pub id: String, pub title: String, pub status: TodoStatus, pub created_by: String, pub owner: Option<String>, pub completed_by: Option<String>, pub depends_on: Vec<String>, pub completion_criteria: Vec<CheckItem>, pub handoff: Vec<String>, pub result: Option<TodoResult>, pub block_reason: Option<String>, pub cancel_reason: Option<String>, pub children: Vec<Todo> }
pub struct ExecutionState { pub blueprint_id: String, pub state: BlueprintState, pub definition_of_done: ProgressCounts, pub todos: TodoCounts, pub ready_todos: Vec<String>, pub not_ready_todos: Vec<NotReadyTodo>, pub blocked_todos: Vec<String>, pub unassigned_todos: Vec<String>, pub open_todos: Vec<String> }
```

`TodoStatus::marker()` 返回 `" "`、`"/"`、`"x"`、`"?"` 或 `"-"`。

- [ ] **Step 4: 运行新增测试**

运行：`cargo test blueprint::tests::validate::serializes_protocol_statuses -- --exact`

预期：通过。

- [ ] **Step 5: 提交类型基础**

```bash
git add Cargo.toml Cargo.lock src/blueprint
git commit -m "feat: add blueprint domain models"
```

## Task 2: 用现有 AST 构建 Blueprint 源文件模型

**Files:**
- Create: `src/blueprint/source.rs`
- Test: `src/blueprint/tests/source.rs`

**Consumes:** `model.rs` 的 Blueprint/DoD/Todo 类型及 `markdown::Parser`。

**Produces:** `ParsedBlueprintSource::parse(text, path, state)`、`SourcePatch`、`apply_patch(text, patch)`；解析结果保存各固定章节、任务和字段的 AST 驱动源范围。

- [ ] **Step 1: 写失败测试，验证 H2、任务、Block ID 和 Children 由 AST 正确映射**

```rust
#[test]
fn parses_todos_but_excludes_completion_criteria_from_graph() {
    let source = valid_blueprint("- [/] 父任务 ^todo-parent\n  - Created By: a\n  - Completion Criteria:\n    - [ ] 局部条件\n  - Children:\n    - [ ] 子任务 ^todo-child\n      - Created By: a\n");
    let parsed = ParsedBlueprintSource::parse("bp-01.md", &source, BlueprintState::Active).unwrap();
    assert_eq!(parsed.blueprint.todos.len(), 1);
    assert_eq!(parsed.blueprint.todos[0].id, "todo-parent");
    assert_eq!(parsed.blueprint.todos[0].children[0].id, "todo-child");
    assert_eq!(parsed.blueprint.todos[0].completion_criteria.len(), 1);
}
```

- [ ] **Step 2: 运行测试，确认失败**

运行：`cargo test blueprint::tests::source::parses_todos_but_excludes_completion_criteria_from_graph -- --exact`

预期：失败，提示 `ParsedBlueprintSource` 未定义。

- [ ] **Step 3: 实现 AST 映射和保真补丁**

以 `ParserOptions::default().enabled_gfm().enabled_ofm()` 解析文本。遍历 `Document.tree`，以 `MarkdownNode::Heading` 的级别和文本确定八个固定 H2 的范围；以 `MarkdownNode::ListItem(ListItem::Task(..))`、`Node.id` 和 AST 祖先关系确定 DoD、根 Todo、`Children` Todo 及 Completion Criteria。由 AST 节点起止行列建立 UTF-8 行索引，转换为 `Range<usize>`。

```rust
pub struct SourcePatch { pub range: Range<usize>, pub replacement: String }

pub fn apply_patch(source: &str, patch: SourcePatch) -> String {
    format!("{}{}{}", &source[..patch.range.start], patch.replacement, &source[patch.range.end..])
}
```

字段只从所属 Todo 的直接字段列表读取；`Children:` 下的 Task 递归成为孩子；`Completion Criteria:` 下的 Task 仅成为 `CheckItem`。未知节点不写入领域模型且不被重排。

- [ ] **Step 4: 写并运行保真修改测试**

```rust
#[test]
fn replacing_result_preserves_unknown_markdown() {
    let source = valid_blueprint("<!-- keep -->\n\n## Extra\n\n> [!note] keep\n");
    let parsed = ParsedBlueprintSource::parse("bp-01.md", &source, BlueprintState::Active).unwrap();
    let updated = apply_patch(&source, parsed.replace_results("### Current Outcome\n\n完成。\n").unwrap());
    assert!(updated.contains("<!-- keep -->\n\n## Extra\n\n> [!note] keep"));
}
```

运行：`cargo test blueprint::tests::source -- --nocapture`

预期：全部通过。

- [ ] **Step 5: 提交源文件模型**

```bash
git add src/blueprint/source.rs src/blueprint/tests/source.rs
git commit -m "feat: parse blueprint markdown with existing ast"
```

## Task 3: 实现协议校验和执行状态派生

**Files:**
- Create: `src/blueprint/validate.rs`
- Modify: `src/blueprint/model.rs`
- Test: `src/blueprint/tests/validate.rs`

**Consumes:** `ParsedBlueprintSource` 输出的 `Blueprint`。

**Produces:** `validate_blueprint(&Blueprint, state)`、`derive_execution_state(&Blueprint, state)`，以及可读的 `BlueprintError`。

- [ ] **Step 1: 写失败测试，锁定依赖环和完成不变量**

```rust
#[test]
fn rejects_dependency_cycles_and_incomplete_completed_todos() {
    let mut blueprint = blueprint_with_todos(&[("todo-a", vec!["todo-b"]), ("todo-b", vec!["todo-a"])]);
    assert!(validate_blueprint(&blueprint, BlueprintState::Active).unwrap_err().to_string().contains("cycle"));
    blueprint.todos[0].depends_on.clear();
    blueprint.todos[1].depends_on.clear();
    blueprint.todos[0].status = TodoStatus::Completed;
    assert!(validate_blueprint(&blueprint, BlueprintState::Active).unwrap_err().to_string().contains("Completed By"));
}
```

- [ ] **Step 2: 运行测试，确认失败**

运行：`cargo test blueprint::tests::validate::rejects_dependency_cycles_and_incomplete_completed_todos -- --exact`

预期：失败，提示 `validate_blueprint` 未定义。

- [ ] **Step 3: 实现全部校验和派生规则**

实现必须覆盖：八个固定章节、合法 `bp-*.md` 路径状态、Created By、ID 前缀/唯一性/节点尾部、依赖存在与非自身/无环、Children 关系、in_progress Owner、completed Completed By/Result Summary/局部条件/未取消孩子、blocked Block Reason、cancelled Cancel Reason。

`derive_execution_state` 扁平化 Todo 树后，以 `status == Pending` 和所有 `depends_on` 状态均为 `Completed` 判断 ready；非 pending 绝不进入 ready 或 not_ready。

- [ ] **Step 4: 写并运行 readiness 测试**

```rust
#[test]
fn cancelled_dependency_keeps_pending_dependent_not_ready() {
    let blueprint = blueprint_with_statuses(&[("todo-a", TodoStatus::Cancelled), ("todo-b", TodoStatus::Pending)]).with_dependency("todo-b", "todo-a");
    let state = derive_execution_state(&blueprint, BlueprintState::Active);
    assert!(state.ready_todos.is_empty());
    assert_eq!(state.not_ready_todos[0].id, "todo-b");
    assert_eq!(state.not_ready_todos[0].unsatisfied_dependencies[0].status, TodoStatus::Cancelled);
}
```

运行：`cargo test blueprint::tests::validate -- --nocapture`

预期：全部通过。

- [ ] **Step 5: 提交校验层**

```bash
git add src/blueprint/model.rs src/blueprint/validate.rs src/blueprint/tests/validate.rs
git commit -m "feat: validate blueprint todo graphs"
```

## Task 4: 实现工作区存储、ETag 和锁

**Files:**
- Create: `src/blueprint/store.rs`
- Test: `src/blueprint/tests/service.rs`

**Consumes:** `Vault.root`、`ParsedBlueprintSource` 和 `validate_blueprint`。

**Produces:** `BlueprintStore::{ensure_workspace, read, write, list, move_lifecycle}`，以及 `StoredBlueprint { path, etag, parsed }`。

- [ ] **Step 1: 写失败测试，锁定初始化和过期 ETag 拒绝**

```rust
#[test]
fn automatically_creates_protocol_layout_and_rejects_stale_etag() {
    let store = fixture_store();
    let stored = store.create(active_blueprint()).unwrap();
    assert_eq!(fs::read_to_string(store.workspace_root().join("manifest.md")).unwrap(), "---\nschema: blueprint/v1\n---\n\n# Blueprint Workspace\n");
    let error = store.write(&stored.id, BlueprintState::Active, Some("stale"), |source| Ok(source.to_string())).unwrap_err();
    assert!(error.to_string().contains("etag"));
}
```

- [ ] **Step 2: 运行测试，确认失败**

运行：`cargo test blueprint::tests::service::automatically_creates_protocol_layout_and_rejects_stale_etag -- --exact`

预期：失败，提示 `BlueprintStore` 未定义。

- [ ] **Step 3: 实现工作区和写入协议**

每个公开存储操作均先调用 `ensure_workspace`：使用 `create_dir_all` 创建 `.blueprint/{active,closed,cancelled,.locks,.tmp}`，只在 manifest 不存在时写入准确的固定内容。工作区固定为当前 Vault 根目录下的 `.blueprint`，不实现向上发现。

`read` 验证路径与源文本，使用 SHA-256 等价的稳定内容哈希作为 ETag。`write` 使用 `.locks/<id>.lock` 的 `fs2::FileExt::lock_exclusive`，在锁内重读、比较 ETag、修改、解析并校验候选内容、写入 `.tmp` 的 `NamedTempFile`、`persist` 到同目录目标。所有锁均通过 RAII 释放。

- [ ] **Step 4: 写并运行发现、原子移动与独立锁路径测试**

```rust
#[test]
fn automatically_creates_workspace_and_uses_per_blueprint_lock_paths() {
    let store = fixture_store();
    store.create(active_blueprint()).unwrap();
    assert!(store.workspace_root().join("active").is_dir());
    assert_ne!(store.lock_path("bp-01"), store.lock_path("bp-02"));
}
```

运行：`cargo test blueprint::tests::service -- --nocapture`

预期：全部通过。

- [ ] **Step 5: 提交存储层**

```bash
git add src/blueprint/store.rs src/blueprint/tests/service.rs
git commit -m "feat: store blueprints with etags and locks"
```

## Task 5: 实现 Blueprint、DoD 和 Todo 用例

**Files:**
- Create: `src/blueprint/service.rs`
- Modify: `src/blueprint/mod.rs`
- Test: `src/blueprint/tests/service.rs`

**Consumes:** model/source/validate/store 的公开接口。

**Produces:** `BlueprintService` 的全部 17 个工具等价方法及完整、resume、状态视图。

- [ ] **Step 1: 写失败测试，锁定完整 Todo 生命周期与显式 DoD**

```rust
#[test]
fn todo_lifecycle_requires_explicit_dod_update() {
    let service = fixture_service();
    let created = service.blueprint_create(create_request()).unwrap();
    let todo = service.todo_create(TodoCreateRequest { blueprint_id: created.id.clone(), title: "实现".into(), created_by: "agent".into(), parent_id: None, owner: Some("agent".into()), depends_on: vec![], completion_criteria: vec!["测试通过".into()], expected_etag: Some(created.etag) }).unwrap();
    service.todo_start(&todo.id, "agent", todo.etag).unwrap();
    service.todo_update_criteria(&todo.id, vec![true], None).unwrap();
    service.todo_complete(&todo.id, "agent", TodoResultInput { summary: "完成".into(), references: vec![] }, None).unwrap();
    assert!(!service.blueprint_status(&created.id).unwrap().definition_of_done.is_complete());
}
```

- [ ] **Step 2: 运行测试，确认失败**

运行：`cargo test blueprint::tests::service::todo_lifecycle_requires_explicit_dod_update -- --exact`

预期：失败，提示 `BlueprintService` 未定义。

- [ ] **Step 3: 实现 Blueprint 与 DoD 方法**

实现 `blueprint_create`、`blueprint_get`、`blueprint_list`、`blueprint_update`、`blueprint_status`、`blueprint_close`、`blueprint_cancel`、`dod_update`。所有方法均通过 store 自动确保当前 Vault 根目录工作区存在。创建生成 ID、初始 Markdown 和 Record；resume 限制返回字段；关闭追加 `### Closure` 并记录未完成条目；取消追加 `### Cancellation` 并移动目录。所有写方法接受可选 `expected_etag`。

- [ ] **Step 4: 实现 Todo 方法和状态机**

实现 `todo_create`、`todo_get`、`todo_list`、`todo_update`、`todo_assign`、`todo_start`、`todo_complete`、`todo_block`、`todo_cancel`。每个方法先由 store 在锁内读取，再由 source 生成最小补丁，随后验证候选文本。`todo_update` 不改变 Created By、Completed By 或 checkbox；`todo_assign` 对 in_progress/blocked 变更 Owner 前检查 Handoff；`todo_complete` 写入 Completed By、Result、清理 Block Reason 并保留 Owner。

- [ ] **Step 5: 写并运行边界测试**

```rust
#[test]
fn blocked_todo_needs_reason_and_handoff_and_close_records_open_work() {
    let service = fixture_service_with_started_todo();
    assert!(service.todo_block("todo-01", "agent", "需要决定", vec![], None).is_err());
    service.todo_block("todo-01", "agent", "需要决定", vec!["等待用户".into()], None).unwrap();
    let closed = service.blueprint_close("bp-01", "agent", Some("用户停止".into()), None).unwrap();
    assert_eq!(closed.state, BlueprintState::Closed);
    assert!(closed.results.contains("Open Todos"));
}
```

运行：`cargo test blueprint::tests::service -- --nocapture`

预期：全部通过。

- [ ] **Step 6: 提交服务层**

```bash
git add src/blueprint/service.rs src/blueprint/mod.rs src/blueprint/tests/service.rs
git commit -m "feat: add blueprint lifecycle services"
```

## Task 6: 接入 MCP、移除 CLI 占位和生成文档

**Files:**
- Modify: `src/main.rs`
- Modify: `src/server.rs`
- Modify: `src/server/tests.rs`
- Modify: `docs/tools.md`
- Test: `tests/cli.rs`

**Consumes:** `BlueprintService` 的请求/响应类型。

**Produces:** 17 个具有对象 schema 的 MCP 工具，以及更新的工具文档。

- [ ] **Step 1: 写失败测试，锁定所有 Blueprint MCP 工具已注册**

```rust
#[test]
fn public_tools_include_all_blueprint_v1_operations() {
    let names = ObsidianVaultMcp::tool_definitions().into_iter().map(|tool| tool.name.to_string()).collect::<Vec<_>>();
    for expected in ["blueprint_create", "blueprint_get", "blueprint_list", "blueprint_update", "blueprint_status", "blueprint_close", "blueprint_cancel", "dod_update", "todo_create", "todo_get", "todo_list", "todo_update", "todo_assign", "todo_start", "todo_complete", "todo_block", "todo_cancel"] {
        assert!(names.contains(&expected.to_string()), "missing {expected}");
    }
    assert!(!names.contains(&"blueprint_init".to_string()));
    assert!(!names.contains(&"blueprint_discover".to_string()));
}
```

- [ ] **Step 2: 运行测试，确认失败**

运行：`cargo test server::tests::public_tools_include_all_blueprint_v1_operations -- --exact`

预期：失败，报告缺失 `blueprint_create`。

- [ ] **Step 3: 定义请求 schema 与薄路由**

在 `server.rs` 为每个工具增加 `Deserialize + JsonSchema` 请求类型。`ObsidianVaultMcp` 在 `AppState` 中保存 `BlueprintService::new(vault.clone())`，每个 `#[tool]` 方法调用 `run_tool` 后仅解构请求并委托服务。工具描述写明 vault 相对路径、ETag 可选性与状态前置条件。

- [ ] **Step 4: 实现 CLI 初始化与端到端测试**

删除 `Command::Blueprint` 变体、其 telemetry 分支和 `todo!("")` 占位实现。新增 MCP 服务测试：首次 `blueprint_create` 后，断言当前 Vault 根目录的 manifest 及五个目录存在；第二次创建保持 manifest 不变。

- [ ] **Step 5: 重新生成文档并验证**

运行：

```bash
cargo run -- generate-docs
cargo run -- generate-docs --check
cargo test server::tests blueprint::tests::service -- --nocapture
```

预期：工具文档更新，所有指定测试通过。

- [ ] **Step 6: 提交集成**

```bash
git add src/main.rs src/server.rs src/server/tests.rs tests/cli.rs docs/tools.md
git commit -m "feat: expose blueprint mcp tools"
```

## Task 7: 全量验证与协议回归

**Files:**
- Modify: 必要的 Blueprint 实现或测试文件
- Test: 全部 Rust 测试与文档检查

**Consumes:** 完整集成后的工作树。

**Produces:** 对设计稿所有行为的最终验证记录。

- [ ] **Step 1: 运行格式化与静态检查**

运行：

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

预期：均以退出码 0 完成。

- [ ] **Step 2: 运行全部测试和文档检查**

运行：

```bash
cargo test
cargo run -- generate-docs --check
```

预期：全部测试通过，工具文档无差异。

- [ ] **Step 3: 对照设计稿复核**

逐项核对自动工作区创建、固定章节、Record、DoD、Todo 字段/状态/readiness、校验、锁/ETag、17 个工具和 Agent 协议。对每个缺口补测试并以失败—通过循环修正。

- [ ] **Step 4: 提交最终修正**

```bash
git add Cargo.lock src/blueprint src/main.rs src/server.rs src/server/tests.rs tests/cli.rs docs/tools.md
git commit -m "test: verify blueprint v0.1 protocol"
```
