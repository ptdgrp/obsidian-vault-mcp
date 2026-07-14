# rmcp 2.2.0 升级设计

## 目标

将 `rmcp` 从 1.7.0 升至 2.2.0，并使当前 MCP server 在新的公开 API 下编译、提供相同的工具 schema，且能通过真实 stdio JSON-RPC 会话工作。

## 范围

- `Cargo.toml` 与 `Cargo.lock` 使用 `rmcp` 2.2.0。
- 仅迁移因 rmcp 2.2.0 不兼容而必须调整的 server、router、handler 包装器和 stdio transport 调用。
- 不保留 rmcp 1.x 兼容代码、条件编译或适配层。
- 不改变工具名称、输入字段、输出字段、默认值、业务查询或 mutation 逻辑。

## 实施方式

1. 先写一个 schema 回归断言，锁定 `tools/list` 所列工具的名称集合、输入/输出 schema 的对象根约束，以及现有的 `uint` format 禁令。升级前该断言在当前依赖版本通过；升级后以它约束迁移结果。
2. 升级依赖并执行 `cargo check`。根据 rmcp 2.2.0 的实际编译诊断，逐项替换已变更的导入、trait 实现、tool router、参数/结果 wrapper 与 stdio serve/wait 调用。迁移只限于编译 API 差异。
3. 在 `tests/cli.rs` 增加黑盒 stdio 测试：启动已编译二进制的 `serve` 子命令，按行写入 JSON-RPC `initialize`、`notifications/initialized`、`tools/list` 与 `tools/call`（以临时 vault 中的确定性笔记调用 `read_note`）；逐行解析响应，断言协议响应、工具清单和调用结果。测试将关闭 stdin 并等待子进程退出，防止遗留服务进程。

## 验收与回归

- 新增 schema 回归用例和 stdio 冒烟用例均纳入 `cargo test`。
- 运行 `cargo test --all-targets`，运行 `cargo run -- generate-docs --check`，并运行 `cargo build --release`。
- 明确检查工具集合和 schema 关键约束没有变化；stdio 测试验证实际传输层，而不是直接调用 Rust handler。

## 非目标

- 不重构 `ObsidianVaultMcp`、不改工具实现或公共 MCP 契约。
- 不同时升级无关依赖。
- 不为 rmcp 1.x 留下任何兼容路径。
