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
