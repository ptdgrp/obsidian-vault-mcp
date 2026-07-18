# Task 4 报告：Blueprint v2 Service 迁移

## 实现摘要

- Blueprint 创建现在生成 `blueprint/v2` frontmatter、稳定聚合路径、Rubric、Evidence 和 Revision History Section；创建请求和 CLI 都要求 Rubric。
- Service 已不再调用 Store 的 v1 `read_active`、`write` 或 `move_to` 入口；Blueprint 写入改用 `read`、`write_blueprint` 与 `set_state`，关闭和取消只更新 frontmatter state，不移动目录。
- Todo 创建使用 v2 中央图标准 Markdown 文档链接，并创建 `todos/<todo-id>.md` 详情文档。
- 增加 `BlueprintPatch` 与语义更新路径；Intent、Constraints、Plan、Rubric 的更新要求 `changed_by` 和 `change_reason`，并追加 Revision History。MCP update 已使用该路径。
- 已迁移现有 Service fixture 到 v2 路径和必需 Rubric，并新增 Revision 行为测试。

## 测试证据

2026-07-19 本 worktree 中执行：

- `cargo fmt --all`
- `cargo test blueprint::tests -- --nocapture`：48 passed, 0 failed
- `cargo check`：成功（仅有 Store 过渡 API 的 dead-code warning）
- `git diff --check`：成功

## 风险 / 遗留

- 当前 Todo 的生命周期字段仍同时保留在中央图的旧 Service 投影中；独立 Todo 文档已经创建，但后续 Todo 更新尚未完全改为以详情文件的 Completion Criteria、Handoff 与 Results 为唯一读取/写入来源。该协议收敛需要在合并前继续完成，不能将其误认为已完成的 v2 Todo 双 ETag 实现。
- `todo_create` 已使用 v2 文档链接和详情文件，但创建详情后再写中央图的失败清理以及完整单 guard 多文档事务仍需收敛。

## 审查修复补充

- Todo 创建现在在同一 Blueprint guard 中创建详情文件并提交中央图；中央图写失败时删除刚创建的详情文件，覆盖 stale ETag 无 orphan 回归。
- `todo_get` 会把中央图状态/依赖与 TodoDetail 的 Completion Criteria、Handoff、Results 合并；`todo_update` 实际写回 Todo 详情文档；完成会写入详情 Results 和 Evidence。
- Service mutation 在 closed/cancelled Blueprint 上拒绝执行，重复 close/cancel 也被拒绝；完整 close 需要非空 Results 和 Evidence 引用。
- full view 返回 Todo 文档索引，resume 返回 Rubric 且从详情文件载入活动 Todo 的 Handoff；CLI update 走 BlueprintPatch 语义更新，支持 rubric/changed_by/change_reason。
- 最终验证：`cargo fmt --all && cargo test blueprint::tests -- --nocapture` 为 50 passed, 0 failed；`cargo check` 和 `git diff --check` 成功。

## 第二轮复审修复

- 中央 Todo 创建和更新不再写入 Completion Criteria、Handoff 或 Result Summary；这些字段由 TodoDetail 读取和写入。Graph validator 也不再把详情字段当作中央图不变量。
- 更新了详情与 Graph 分层的 validate / Service 回归测试，保持 `cargo test blueprint::tests` 50 个测试通过。

## 原子性收尾修复

- 所有受 Blueprint 锁保护的中央图和 Todo 详情写入现在在锁内重新确认 `active`；Store 的锁内写入口也强制该 guard，避免先读后写的 TOCTOU。
- `todo_update`、`todo_complete` 与 `todo_block` 在一个锁内先写并验证 TodoDetail，再提交中央图；完成操作永远不会在详情无效时推进中央 marker。
- `blueprint_close` / `blueprint_cancel` 合并为一次锁内写入，同时更新 Record、Results、Revision History 和 frontmatter state；两者均追加 Revision。
- 完整关闭严格解析 Results 中的标准 Markdown Evidence 链接，拒绝 malformed、dangling 与重复 Evidence ID。
- 回归覆盖：详情写失败不提交中央图、锁内 stale active guard、关闭/取消 Revision、dangling Evidence，以及中央图不含详情字段。

验证：`cargo fmt --all`、`cargo test blueprint::tests -- --nocapture`（54 passed）、`cargo check`、`git diff --check`。

## 原子性复审补充

- `todo_update` 现会在同一锁内先构造并解析 TodoDetail 候选，再构造并验证中央图候选（含依赖/readiness）；两份候选均通过后才按详情、中央图顺序写入，避免无效依赖留下已改标题的详情文档。
- 回归覆盖同时改标题并提交 self / unknown dependency：失败后 Todo H1 与中央 Markdown 链接文本均保持原值。
