# obsidian-vault-mcp

面向 Obsidian 风格 Markdown vault 的只读 MCP server。

这个服务适合独立出来：它的职责是把本地 Markdown vault 以受控、可引用、低噪音的方式暴露给 LLM agent。它不依赖数据库、向量索引或后台 watcher。文件路径、Markdown 标题、本地链接、标签和源码行号就是它的主要语义边界。

## 特性

- 只读访问 vault，不写入笔记。
- 每次工具调用都从磁盘读取当前文件状态。
- 使用内存级 Markdown 解析缓存，按路径、文件大小和修改时间失效。
- 默认忽略隐藏路径，例如 `.obsidian/`、`.git/`、`.agents/` 和 `.hidden.md`。
- 遵守 `.gitignore`、`.git/info/exclude`、父目录规则，以及每个子目录自己的 `.gitignore`。
- 返回源码位置，包括 vault-relative path、行号范围和最近标题，便于 agent 引用证据。
- 支持 Obsidian wikilink、安全的相对 Markdown link、alias、heading、block id、tag、backlink、outlink 和 note graph。

它不会推断“人物”“组织”“章节”等业务领域类型。目录名、文件路径和 Markdown 标题才是语义来源；agent 应该基于这些证据继续推理，而不是让 MCP server 替它猜。

## 构建与测试

```sh
cargo check
cargo test
```

## CLI 示例

```sh
cargo run -- --vault /path/to/vault list-notes --json
cargo run -- --vault /path/to/vault list-vault-files --json
cargo run -- --vault /path/to/vault read-note "人物/林动.md" --json
cargo run -- --vault /path/to/vault parse-note "人物/林动.md" --json
cargo run -- --vault /path/to/vault get-note-outline "人物/林动.md" --json
cargo run -- --vault /path/to/vault read-section "人物/林动.md" --heading "身体" --json
cargo run -- --vault /path/to/vault resolve '[[林动#身体]]' --json
cargo run -- --vault /path/to/vault backlinks '[[林动]]' --json
cargo run -- --vault /path/to/vault search "求生本能" --json
cargo run -- --vault /path/to/vault search-regex "林动.{0,20}代偿" --path-glob "正文/**/*.md" --json
cargo run -- --vault /path/to/vault get-tags --tag "状态/身体" --json
cargo run -- --vault /path/to/vault query-frontmatter phase --mode equals --value active --json
cargo run -- --vault /path/to/vault query-frontmatter arc --mode regex --value "引擎.*" --json
cargo run -- --vault /path/to/vault find-unresolved-links --json
cargo run -- --vault /path/to/vault find-ambiguous-links --json
cargo run -- --vault /path/to/vault get-note-graph --json
cargo run -- --vault /path/to/vault get-graph-neighborhood "林动" --depth 1 --direction both --json
```

启动 MCP server：

```sh
cargo run -- --vault /path/to/vault serve
```

常用缓存参数：

```sh
cargo run -- --vault /path/to/vault \
  --parse-cache-ttl-secs 600 \
  --parse-cache-max-entries 1024 \
  serve
```

## MCP 配置

```json
{
  "context_servers": {
    "obsidian-vault": {
      "command": "/path/to/obsidian-vault-mcp",
      "args": ["--vault", "/path/to/vault", "serve"]
    }
  }
}
```

协议输出只写 stdout；日志必须留在 stderr。

## 可观测性

服务通过 `tracing` 把日志写到 stderr，stdout 仍然只用于 MCP 协议或 CLI JSON。用
`--log-level` 控制日志级别：

```sh
cargo run -- --vault /path/to/vault --log-level info serve
```

OpenTelemetry 导出是可选的。通过 `--otel-endpoint` 或
`OTEL_EXPORTER_OTLP_ENDPOINT` 设置 OTLP HTTP endpoint 后，会同时导出 traces 和 logs：

```sh
cargo run -- --vault /path/to/vault \
  --log-level info \
  --otel-endpoint http://localhost:4318 \
  serve
```

service name 默认是 `obsidian-vault-mcp`，可用 `--otel-service-name` 或
`OTEL_SERVICE_NAME` 覆盖。

每次 MCP tool 调用都会创建一个 `mcp.tool` span，记录 `tool.name`、耗时以及
成功/失败事件。同一批 tracing event 也会作为 OTEL logs 导出。默认不会记录工具参数、
note 内容、搜索文本或 regex pattern。正常退出时，进程会先 force flush 待发送 span
和 logs，然后最多等待 5 秒完成 OTEL shutdown。

## list_vault_files 语义

`list_vault_files` 返回适合 LLM 读取的平铺文件清单，而不是真正的目录树。

- 每个条目都有 vault-relative `path`。
- 目录不作为独立节点返回。
- 空目录不返回。
- Markdown 文件在 `include_files` 为 true 时返回。
- 非 Markdown 文件计入 attachment；传 `include_attachments: true` 才返回 attachment 条目。
- 扫描时遵守根目录和每个子目录的 `.gitignore`。

例如：

```text
资料库/技术设定/
  README.md
  001-发动机.md
```

会返回：

```text
资料库/技术设定/README.md
资料库/技术设定/001-发动机.md
```

路径本身携带目录上下文。`README.md` 不会被提升成目录说明，而是作为普通 Markdown 文件返回；默认只带首个标题。需要 README 标题列表时，使用 `include_readme_outline: true`。

## 工具列表

详细 input/output 契约见 [docs/tools.md](docs/tools.md)。

- `list_notes`：列出可见 Markdown note。
- `list_vault_files`：返回可见文件路径清单。
- `read_note`：读取一个 Markdown note 全文。
- `parse_note`：解析 note 的标题、本地链接、embed、tag、block id、frontmatter 和源码范围。
- `get_note_outline`：返回一个 note 的标题树。
- `read_section`：读取一个标题、block id 或行号范围。
- `search_text`：字面量搜索。
- `search_regex`：Rust regex 搜索，可用 path glob 限定范围。
- `resolve_ref`：解析 Obsidian reference，例如 `[[Note#Heading]]`。
- `get_outlinks`：获取一个 note 的出链。
- `get_backlinks`：获取一个 note 或 reference 的反链。
- `get_tags`：列出正文 tag 节点和 frontmatter `tag` / `tags`，或列出某个标签下的 note。
- `query_frontmatter`：按顶层 frontmatter 字段查询 note，模式必须显式指定为 `exists`、`equals` 或 `regex`。
- `collect_note_context`：收集一个 note 的当前内容、出链和反链上下文。
- `collect_reference_context`：先解析 reference，再收集上下文。
- `find_unresolved_links`：查找无法解析的本地链接。
- `find_ambiguous_links`：查找解析到多个 note 的本地链接。
- `get_graph_neighborhood`：围绕一个 note 或 reference 返回限定深度的 graph 邻域；常规 agent 上下文优先用它。
- `get_note_graph`：返回全量本地链接 note graph；用于审计、可视化、调试或全局健康检查。

## 推荐使用顺序

1. 用 `list_vault_files` 先看可见文件路径。
2. 用 `list_notes` 获取 note 路径和标题的轻量清单。
3. 用 `get_note_outline` 定位要读取的标题。
4. 用 `read_section` 读取需要的局部内容，避免整篇塞进上下文。
5. 用 `get_tags` 查询正文标签和 frontmatter 标签；用 `query_frontmatter` 查询类似 `phase: active` 的元数据条件。
6. 用 `resolve_ref`、`get_outlinks`、`get_backlinks` 和 `get_graph_neighborhood` 处理明确的本地链接关系。
7. 大范围分析前，先用 `find_unresolved_links` 和 `find_ambiguous_links` 做 vault 健康检查。

搜索工具默认返回轻量片段：路径、行号范围、最近标题和简短预览。除非显式传 `context_lines`，否则不会把大量上下文塞回给 agent。

vault 较大时，优先用 `get_graph_neighborhood`，不要直接取全量 `get_note_graph`。它支持 `depth`、`direction` 和 `include_unresolved`，适合围绕一个 note 拉近邻上下文。这里的链接包括 Obsidian wikilink，以及相对路径没有越出 vault 的 Markdown link。只有在需要全图审计、可视化、调试或全局健康检查时，才使用 `get_note_graph`。

## 安全边界

这个 server 是只读的。它没有数据库、没有向量索引、没有文件 watcher，也没有落盘缓存。每次工具调用都会检查文件元数据；文件大小或修改时间变化时会重新解析。

隐藏路径和 gitignore 命中的路径默认不会进入可见上下文，避免 Obsidian 配置、git 数据、agent skill 文件或生成物污染 agent 的工作记忆。

## 独立发布建议

如果把它从 Papilio monorepo 拆出去，建议保持以下边界：

- 保留纯 MCP + CLI，不引入 Papilio 业务概念。
- 把工具契约和 README 作为主要公共接口，避免依赖外部产品文档。
- 为 gitignore、本地链接解析、section 读取和搜索结果源码范围保留测试。
- 发布前补齐安装方式，例如 `cargo install --path .`、预编译二进制或包管理器说明。
- 如果后续需要兼容旧版本，可以增加旧工具名的兼容别名，但主工具名保持 `list_vault_files`。
