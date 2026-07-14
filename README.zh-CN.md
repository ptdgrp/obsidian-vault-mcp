# obsidian-vault-mcp

[English README](./README.md)

面向 Obsidian 风格 Markdown vault 的 MCP server，提供紧凑、面向任务的查询工具和结构化章节编辑。

这个服务刻意保持机械、可验证：

- 每次工具调用都从磁盘读取当前 vault 状态。
- 只使用内存 Markdown 解析缓存，按路径、文件大小和修改时间失效。
- 解析缓存默认 10 分钟未使用即过期，最多保留 1024 篇已解析 note。
- 只通过显式章节操作编辑 note，每次写入都是原子替换。
- 默认忽略隐藏路径，例如 `.obsidian/`、`.git/`、`.agents/` 和 `.hidden.md`。
- 扫描可见 Markdown note 时遵守 `.gitignore`、`.git/info/exclude` 和父目录规则。
- 返回 vault-relative path、Obsidian 风格行引用和最近标题，便于 LLM 引用证据。

它不会推断“人物”“组织”“章节”等业务领域类型。文件路径、Markdown 标题、本地链接、标签和 frontmatter 才是语义来源；agent 应该基于这些证据继续推理，而不是让 MCP server 替它猜。

## 构建与测试

```sh
cargo check
cargo test
```

## CLI 快速开始

从第一页 notes 开始，再逐步进入精确读取和关系检查：

```sh
cargo run -- --vault /path/to/vault list-notes --page 1
cargo run -- --vault /path/to/vault get-note-outline "人物/林动.md" --page 1
cargo run -- --vault /path/to/vault read-note "人物/林动.md#身体" --max-chars 4096
cargo run -- --vault /path/to/vault get-note-structure "人物/林动.md"
cargo run -- --vault /path/to/vault get-note-stats "人物/林动.md"
cargo run -- --vault /path/to/vault resolve-ref '[[林动#身体]]'
cargo run -- --vault /path/to/vault get-outlinks "人物/林动.md" --page 1
cargo run -- --vault /path/to/vault get-backlinks '[[林动]]' --page 1
cargo run -- --vault /path/to/vault get-note-neighborhood "林动" --depth 1 --direction both
cargo run -- --vault /path/to/vault audit-links --page 1
cargo run -- --vault /path/to/vault search-text "求生本能" --include "正文/**/*.md" --include "资料/**/*.md" --exclude "**/草稿/**" --page 1
cargo run -- --vault /path/to/vault search-regex "林动.{0,20}代偿" --include "正文/**/*.md" --exclude "**/草稿/**" --page 1
cargo run -- --vault /path/to/vault list-tags --page 1
cargo run -- --vault /path/to/vault get-tag "状态/身体" --page 1
cargo run -- --vault /path/to/vault list-categories --page 1
cargo run -- --vault /path/to/vault get-category "人物" --page 1
cargo run -- --vault /path/to/vault query-frontmatter phase --mode equals --value active --page 1
cargo run -- --vault /path/to/vault append-section "人物/林动.md" "新增内容" --heading "身体"
cargo run -- --vault /path/to/vault replace-section "人物/林动.md" "替换内容" --heading "身体"
cargo run -- --vault /path/to/vault delete-section "人物/林动.md" --heading "旧设定"
cargo run -- --vault /path/to/vault rename-heading "人物/林动.md" --old-heading "身体" --new-heading "身体状态"
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

## 四层工具模型

公开工具围绕 agent 正在回答的问题组织：

1. notes 枚举：`list_notes` 分页列出可见 Markdown notes。需要候选路径、标题和大小时先用它。
2. note 内部结构：`get_note_outline`、`read_note`、`get_note_structure` 和 `get_note_stats` 检查单篇 note，或单个标题、block、行范围。
3. note 外部关系：`resolve_ref`、`get_outlinks`、`get_backlinks`、`get_note_neighborhood`、`list_tags`、`get_tag`、`list_categories`、`get_category`、`query_frontmatter`、`search_text` 和 `search_regex` 回答聚焦的跨 note 问题。
4. 问题关系审计：`audit_links` 分页报告可见 notes 中无法解析和解析歧义的本地链接，让大范围工作先确认链接健康状态。

编辑工具使用同一套结构化 selector：`append_section`、`replace_section`、`delete_section`、`rename_heading`、`rename_note` 和 `rename_block_id`。

## 分页与过滤

分页工具接受固定数字 `page`。页码从 1 开始，`page: 1` 是第一页。没有 cursor，也没有不透明翻页 token。如果 `page` 超过总页数，响应会保留请求的页码，返回空结果列表，并同时返回分页总数。

请求级路径过滤使用 vault-relative Markdown note 路径。省略或传入空 `include` 数组时不额外限制范围；否则 note 至少要匹配一个 include。多个 include 是并集，多个 exclude 也是并集；exclude 命中时总是优先排除。

`get_note_neighborhood` 刻意不分页。它返回由 `depth` 和 `direction` 控制的有界已解析链接邻域；如果结果太宽，应缩小目标或深度。需要分页的关系健康检查时，使用 `audit_links --page 1`。

## reference 与 path

path 是 vault-relative Markdown note 路径，例如 `人物/林动.md`。当 `list_notes` 或其他工具已经返回具体路径时，直接传 path 最清晰。

reference 是 Obsidian 风格目标，例如 `[[林动#身体]]`、`林动#身体` 或 `人物/林动.md#L1-L20`。reference 可以指向 note、标题、block id 或行范围。人类写法需要先校验时，用 `resolve_ref`。

歧义 reference 不会被猜测。工具要么要求唯一解析的目标，要么报告歧义，让调用方选择具体 path 或 selector。

## 工具契约

详细 MCP input/output 契约见 [docs/tools.md](docs/tools.md)。

主要读取与查询工具：

- `list_notes`
- `get_note_outline`
- `read_note`
- `get_note_structure`
- `get_note_stats`
- `resolve_ref`
- `get_outlinks`
- `get_backlinks`
- `get_note_neighborhood`
- `audit_links`
- `search_text`
- `search_regex`
- `list_tags`
- `get_tag`
- `list_categories`
- `get_category`
- `query_frontmatter`

主要编辑工具：

- `append_section`
- `replace_section`
- `delete_section`
- `rename_heading`
- `rename_note`
- `rename_block_id`

## 推荐使用顺序

1. 先运行 `list_notes --page 1`。
2. 如果问题属于已知目录或子集，先加路径过滤。
3. 用 `get_note_outline` 选择要读取的标题。
4. 用 `read_note` 读取最小有用证据范围：path、heading、block id 或行范围。
5. 用 `resolve_ref`、`get_outlinks`、`get_backlinks` 或 `get_note_neighborhood` 处理明确的本地链接问题。
6. 用 `search_text`、`search_regex`、标签、分类和 frontmatter 查询处理尚未锚定到单篇 note 的召回问题。
7. 大范围重构或审计依赖链接可靠性时，先用 `audit_links`。

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

协议输出只写 stdout；日志留在 stderr。

## 可观测性

服务通过 `tracing` 把日志写到 stderr，stdout 只用于 MCP 协议或 CLI JSON。用 `--log-level` 控制日志级别：

```sh
cargo run -- --vault /path/to/vault --log-level info serve
```

OpenTelemetry 导出是可选的。通过 `--otel-endpoint` 或 `OTEL_EXPORTER_OTLP_ENDPOINT` 设置 OTLP HTTP endpoint 后，会同时导出 traces 和 logs：

```sh
cargo run -- --vault /path/to/vault \
  --log-level info \
  --otel-endpoint http://localhost:4318 \
  serve
```

service name 默认是 `obsidian-vault-mcp`，可用 `--otel-service-name` 或 `OTEL_SERVICE_NAME` 覆盖。

每次 MCP tool 调用都会创建一个 `mcp.tool` span，记录 `tool.name`、耗时以及成功/失败事件。同一批 tracing event 也会作为 OTEL logs 导出。默认不会记录工具参数、note 内容、搜索文本或 regex pattern。正常退出时，进程会先 force flush 待发送 span 和 logs，然后最多等待 5 秒完成 OTEL shutdown。

## 安全边界

这个 server 没有数据库、向量索引、文件 watcher 或落盘缓存。默认操作读取 vault；写入只通过显式结构化编辑工具完成。每次工具调用都会检查文件元数据，变化后的文件会在使用前重新解析。

隐藏路径和 gitignore 命中的路径默认不会进入可见 notes，避免 Obsidian 配置、git 数据、agent skill 文件或生成物进入常规 note 结果。
