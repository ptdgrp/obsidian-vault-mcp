# Blueprint MCP v0.1 设计

## 目标

将 Blueprint v0.1 协议完整实现为面向本地 Markdown 工作区的 MCP 工具。Blueprint 文件是唯一事实来源；协议之外的文件历史由 Git 保存。

## 范围

本次实现涵盖工作区目录、Blueprint 生命周期、完成定义（Definition of Done，以下简称 DoD）更新、Todo 图生命周期、派生执行状态、结构校验、ETag 并发控制、单文件锁、原子写入，以及设计稿列出的全部 21 个工具。

所有 Blueprint 生产代码均放在 `src/blueprint`。既有 MCP 服务仅定义请求结构并将调用委托给 Blueprint 服务。现有的 `blueprint` CLI 占位命令改为安全的工作区初始化入口。

## 架构

### 基于现有 AST 的保留源文本编辑

`src/blueprint/document.rs` 复用项目既有的 `markdown` 依赖，以 Obsidian 模式解析原始 Markdown。该 AST 已提供 H1/H2 标题、列表、Task List Item、Block ID、节点父子关系和起止行列；因此 Blueprint 不实现第二个 Markdown 解析器。

文档层只执行三件事：从 AST 映射固定 H2 章节、任务与 `Children` 关系为 Blueprint 领域节点；将 AST 行列位置转换为原始文本字节范围；基于这些范围进行局部替换或插入。它使用有限的字段文本解释 `Created By`、`Depends On` 等 Blueprint 专用内容，但不重新识别 Markdown 语法。

这不是“解析后重新格式化”的管线。未知章节和字段、HTML 注释、Wiki Link、Callout、普通文本和既有排版都会保留。修改只应用于已经由 AST 精确定位的目标节点或章节。

### 领域模型与校验

`src/blueprint/model.rs` 定义可序列化的对外模型：工作区和 Blueprint 状态、解析后的 Blueprint 字段、DoD、Todo、Todo 状态、readiness、生命周期结果，以及完整和恢复视图。

`src/blueprint/validate.rs` 在每次读取和候选写入时执行校验。校验要求：八个固定章节、合法的 `bp-*.md` 文件名及目录生命周期状态、`Created By`、唯一且前缀正确的 DoD/Todo Block ID、有效依赖引用、无自依赖和依赖环、有效的 `Children` 包含关系，以及所有状态特有的不变量。

服务在读取时派生执行状态。只有 `pending` Todo 参与 readiness 计算；仅当全部依赖完成时才 ready。被阻塞或取消的依赖会使依赖方保持 pending 且 not ready。DoD 与 Todo 状态始终相互独立。

### 存储与并发

`src/blueprint/store.rs` 负责全部文件系统行为：创建和发现 `.blueprint` 工作区、读取 Blueprint 文件、计算文件内容 ETag、通过 `.blueprint/.locks` 中每个 Blueprint 独立的锁串行化写入、在锁内重新读取、可选的 `expected_etag` 校验、通过 `.blueprint/.tmp` 写入临时文件，以及对目标文件进行原子替换。

生命周期变更会先完成文档更新和校验，再通过 `active`、`closed`、`cancelled` 目录之间的原子重命名移动文件。不同 Blueprint ID 使用不同锁，可并发写入。

### 服务与 MCP 边界

`src/blueprint/service.rs` 提供全部工具的领域操作：

- 工作区：`blueprint_init`、`blueprint_discover`。
- Blueprint：`blueprint_create`、`blueprint_get`、`blueprint_list`、`blueprint_update`、`blueprint_status`、`blueprint_close`、`blueprint_cancel`。
- DoD：`dod_update`。
- Todo：`todo_create`、`todo_get`、`todo_list`、`todo_update`、`todo_assign`、`todo_start`、`todo_complete`、`todo_block`、`todo_cancel`。

`src/server.rs` 新增 `schemars` 请求类型和薄工具处理器，不包含 Markdown 编辑或图逻辑。`src/main.rs` 创建服务，并让现有 `blueprint` 命令调用 `blueprint_init`。

## 行为细节

### ID 与文件

新生成的 Blueprint、Todo 和 DoD ID 使用可按时间排序的 ULID 兼容标识符，前缀分别为 `bp-`、`todo-`、`dod-`。ID 在同一 `.blueprint` 工作区内唯一。Blueprint 只能存放在 `.blueprint/{active,closed,cancelled}/bp-<id>.md`；所在目录即其生命周期状态。

`blueprint_init` 创建 `manifest.md`、`active`、`closed`、`cancelled`、`.locks` 与 `.tmp`。manifest 只含设计稿规定的 `blueprint/v1` frontmatter 和工作区 H1。`blueprint_discover` 从给定的 vault 相对目录向上查找，直到发现有效工作区。

### 修改规则

普通 Blueprint 更新只能修改标题、Intent、Constraints、Plan、Results 与 Notes，不能修改 Record 字段或 Todo 状态。只有专用的生命周期工具能修改 `Closed By`、`Cancelled By`、`Completed By`、任务 checkbox 状态或生命周期目录。

Todo 修改严格遵守设计稿中的状态机。开始时必须已分配 Owner 且依赖全部完成。完成时必须处于 in_progress、所有完成条件已勾选、所有未取消子 Todo 已完成，并包含非空 Result Summary。阻塞时必须同时提供原因和 Handoff。取消时必须提供原因。重新分配 in_progress 或 blocked Todo 前必须已有 Handoff。

只要提供 `closed_by`，关闭操作始终允许；未完整关闭时还必须提供原因，并在 Results 中记录未完成 DoD/Todo。取消操作记录原因和执行者，更新 Record，追加取消结果，并将文件移至 `cancelled`。

### 返回视图

每次读取文件都返回 ETag 与派生执行状态。`blueprint_get(view: resume)` 只返回 Intent、Constraints、未完成 DoD、Plan、进行中/ready/blocked Todo（含 Handoff）和当前 Results。状态工具返回计数、ready Todo、带未满足依赖的 not-ready Todo、blocked/unassigned Todo 与未完成 Todo。

## 错误处理

格式错误的文件会返回可定位的校验错误，绝不发生部分重写。过期的 `expected_etag` 会在获得锁并重新读取后被拒绝。非法生命周期、状态变更、字段修改、依赖或父子关系都会在写入前被拒绝。I/O 与锁错误沿用现有 MCP 错误处理链返回。

## 测试策略

`src/blueprint/tests/` 下的测试覆盖：

- 初始化、发现、manifest 校验和 Blueprint 创建；
- 完整解析，以及保留未知 Markdown 的定点修改；
- 所有结构和状态不变量、重复 ID、非法 Block ID 位置、父子环、依赖环与缺失引用；
- readiness、恢复/状态派生，包含 blocked 和 cancelled 依赖；
- 每个生命周期工具及其必需的 Record/Results 输出；
- 乐观 ETag 冲突、独立 Blueprint 锁路径和原子存储行为；
- MCP schema/分发与 CLI 初始化命令。

文档生成测试还会确保 `docs/tools.md` 与新增工具集合一致。

## 明确不做的事

Blueprint 不维护修订历史，不持久化执行状态或 readiness，不自动同步 DoD 与 Todo checkbox，不解释协议结构之外的任意用户 Markdown，也不重写无关 Markdown 的格式。
