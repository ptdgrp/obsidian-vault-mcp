# obsidian-vault-mcp

[English README](./README.md)

面向 Obsidian vault 的 MCP server，提供紧凑的 Markdown 查询、只读 Bases 和 Canvas 内容理解，以及结构化章节编辑。

这个服务刻意保持机械、可验证：

- 每次工具调用都从磁盘读取当前 vault 状态。
- 使用内存 Markdown 解析缓存（按路径、文件大小和修改时间失效）及内存编辑历史。
- 解析缓存默认 10 分钟未使用即过期，最多保留 1024 篇已解析 note。
- 只通过显式修改工具编辑 note；每个笔记的单次写入都是原子替换。
- 默认忽略隐藏路径，例如 `.obsidian/`、`.git/`、`.agents/` 和 `.hidden.md`。
- 扫描可见文件时遵守 `.gitignore`、`.git/info/exclude` 和父目录规则。
- 扫描可见文件时遵守 Obsidian `.obsidian/app.json` 的
  `userIgnoreFilters`；精确 path 读取仍然可用。
- 返回 vault-relative path、Obsidian 风格行引用和最近标题，便于 LLM 引用证据。

它不会推断“人物”“组织”“章节”等业务领域类型。文件路径、Markdown 标题、本地链接、标签和 frontmatter 才是语义来源；agent 应该基于这些证据继续推理，而不是让 MCP server 替它猜。

排除规则只限制自动索引和探索，不是文件访问权限。扫描会跳过匹配排除规则的目录；
`archive/**` 可排除整个目录树，`archive/**/*.md` 则只过滤匹配的文件。
已知路径仍可用于读取正文、章节、结构、统计和出链，也可查询可见笔记指向该路径的反向链接。
例如 `read_note("archive/记录.md")` 和 `[[archive/记录#摘要]]` 都可访问被排除的目标。
列表、搜索、标签、分类及图谱探索仍限于可见笔记；不会进入排除目录搜索短文件名或别名。
按需读取不会让文件重新进入索引，单文件大小限制仍然有效。

## 构建与测试

### 通过 npm 安装

预编译包支持 Linux x64/arm64、macOS arm64 和 Windows x64；启动器需要 Node.js 18 或更新版本：

```sh
npm install -g @ptdgrp/obsidian-vault-mcp
obsidian-vault-mcp serve
```

配置 MCP 客户端时，命令可设为 `npx`，参数设为
`["-y", "@ptdgrp/obsidian-vault-mcp", "--vault", "/笔记库的绝对路径", "serve"]`。
npm 包包含默认的 Markdown、Bases 和 Canvas 功能；附件读取和 OCR 需要从源码构建。
启动包通过可选平台依赖安装对应的二进制包；不要禁用 npm 的可选依赖。

### 从源码构建

```sh
cargo check
cargo test
```

从本仓库安装 CLI，供下面的示例使用：

```sh
cargo install --path .
```

默认构建支持 Markdown、Bases 查询和 Canvas 转 Mermaid 读取。使用 `cargo install --path . --features attachments`
启用 PDF、DOCX 附件读取及 `read_attachment` MCP 工具。
需要图片和扫描 PDF 的本地 OCR 时，使用 `--features ocr-local`，它会自动启用
`attachments`；OCR 运行库和模型文件仍需单独安装。
工具文档按当前启用的 features 生成；使用
`cargo run --features attachments -- generate-docs` 可包含附件工具。

## CLI 快速开始

在 Obsidian vault 或其任意子目录中运行已安装的 CLI。
`--vault` 是可选参数：程序会从进程的当前工作目录向上查找，选择最近的包含
`.obsidian/` 的目录作为 vault。从第一页 notes 开始，再逐步进入精确读取和关系检查：

```sh
obsidian-vault-mcp get-note-outline ./README.md
obsidian-vault-mcp read-note ./README.zh-CN.md --heading "构建与测试"
obsidian-vault-mcp list-notes --page 1
obsidian-vault-mcp list-files --extension base --extension canvas --page 1
obsidian-vault-mcp query-base-views "项目.base"
obsidian-vault-mcp read-base "项目.base" --view "进行中" --max-chars 8192
obsidian-vault-mcp read-canvas "规划.canvas" --max-chars 8192
obsidian-vault-mcp get-note-outline "人物/林动.md" --page 1
obsidian-vault-mcp read-note "人物/林动.md#身体" --max-chars 4096
obsidian-vault-mcp get-note-structure "人物/林动.md"
obsidian-vault-mcp get-note-stats "人物/林动.md"
obsidian-vault-mcp set-frontmatter-field "人物/林动.md" status '"active"'
obsidian-vault-mcp set-frontmatter-field "人物/林动.md" score '7' --dry-run false
obsidian-vault-mcp resolve-ref '[[林动#身体]]'
obsidian-vault-mcp get-outlinks "人物/林动.md" --page 1
obsidian-vault-mcp get-backlinks '[[林动]]' --page 1
obsidian-vault-mcp get-note-neighborhood "林动" --depth 1 --direction both
obsidian-vault-mcp audit-links --page 1
obsidian-vault-mcp search-text "求生本能" --include "正文/**/*.md" --include "资料/**/*.md" --exclude "**/草稿/**" --page 1
obsidian-vault-mcp search-regex "林动.{0,20}代偿" --include "正文/**/*.md" --exclude "**/草稿/**" --page 1
obsidian-vault-mcp list-tags --page 1
obsidian-vault-mcp get-tag "状态/身体" --page 1
obsidian-vault-mcp list-categories --page 1
obsidian-vault-mcp get-category "人物" --page 1
obsidian-vault-mcp query-frontmatter phase --mode equals --value active --page 1
obsidian-vault-mcp append-section "人物/林动.md" "新增内容" --heading "身体"
obsidian-vault-mcp replace-section "人物/林动.md" "替换内容" --heading "身体"
obsidian-vault-mcp delete-section "人物/林动.md" --heading "旧设定"
obsidian-vault-mcp rename-heading "人物/林动.md" --old-heading "身体" --new-heading "身体状态"
```

启动 MCP server：

```sh
obsidian-vault-mcp serve
```

需要从其他目录显式选择 vault 时，可使用
`obsidian-vault-mcp --vault /path/to/vault serve`，或设置
`OBSIDIAN_VAULT_MCP_ROOT`。显式配置优先于自动发现，`--vault` 优先于环境变量，
并支持 `~` 和 `~/...` 路径。相对配置 root 会以进程当前工作目录为基准绝对化并规范化。
自动发现只查找当前目录及其祖先，不搜索子目录或整台电脑。

如果 `serve` 没有发现 vault，会提供当前项目目录下的 `list_files`、`read_canvas` 及 Markdown 文件工具：
`read_note`、`get_note_outline`、`get_note_structure`、`get_note_stats`、
`audit_links`、`create_note`、`delete_note`、`edit_note`、`set_frontmatter_field`、`apply_patch`、`append_section`、`replace_section`、`delete_section`，以及编辑历史、撤销与重做工具。
此时 `note` 使用相对于当前项目目录的路径，绝对路径和越界路径会被拒绝；
`audit_links` 只检查 `[text](../target.md)` 等标准 Markdown 相对链接的目标文件是否存在。
Bases 配置发现与全库查询（`query_base_views`、`read_base`）、搜索、元数据和链接图谱工具不可用。
`read-note`、`get-note-outline`、`get-note-structure`、`get-note-stats`、
`read-canvas` 和 `list-files` CLI 命令也支持此项目模式。其他 CLI 命令仍需要显式配置
或自动发现的有效 vault；未发现 vault 或显式配置的路径无效时会报错。

常用缓存参数：

```sh
obsidian-vault-mcp \
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

编辑工具包括 `create_note`、`delete_note`、`edit_note`、`apply_patch`、`append_section`、`replace_section`、`delete_section`、`rename_heading`、`rename_note`、`set_block_id` 和 `set_frontmatter_field`。
section 编辑仅支持 heading 和 block-id selector；line selector 仅保留给 `read_note`。

`set_frontmatter_field` 修改既有 `.md` 精确路径中的单个顶层 YAML 字段，与
`edit_note` 一样不接受简称、别名或 selector。`field` 中的点是字面字符。
`value` 必须是 JSON：字符串需要 JSON 引号，数字、布尔值、数组、对象和 `null`
保留类型；null 写入 YAML null，不删除字段。缺少字段或 frontmatter 时会创建。
默认预览，设置 `dry_run=false`（CLI `--dry-run false`）才写入。结果包含修改前后值、
`previous_exists` 和 `changed`；语义未变化不会增加历史。CLI 修改仍要求 vault，
MCP 的 project 模式也提供此文件局部工具。

编辑器保留正文、BOM、换行风格和其他字段的原文及注释；被替换值内部的格式和注释
可能改变。新增 `yaml-edit` 0.3.2 用于保留语法格式的局部编辑，随后严格校验 YAML、
重复键、目标值、其他元数据和既有引用。语法错误、未闭合、非 mapping 的 header，
以及影响其他字段的 alias/merge 修改均拒绝写入。特殊 YAML 若无法安全局部修改，
会明确报错，不会整块重新序列化。实际写入复用原子写、解析缓存失效和撤销/重做历史。

`edit_history` 列出可撤销和可重做操作的触发命令、时间与受影响文件，不返回笔记正文。`undo_edit` 撤销最近一次成功修改，`redo_edit` 重做最近一次撤销；两者都支持可选的 `steps`（默认 1）。写入前会检查请求中的所有步骤，若有冲突则整次请求都不执行，并返回冲突路径。执行新的修改会清空重做栈。

编辑历史只保存在当前服务进程的内存中，最多保留 100 次操作和约 64 MiB 的序列化记录数据；即使最新一条超过容量限制，也会保留它。重启服务会清空撤销与重做历史。dry-run 和失败操作不会进入历史。进程运行期间，多文件写入失败会回滚；若进程在写入中途突然终止，由于没有落盘恢复日志，仍可能留下部分已写入的文件。

建议先调用 `get_note_outline` 查看目标文件大纲，再用 `read_note` 读取标题或行范围。
`read_note`、`read_base` 和 `read_canvas` 共享默认 8192 个 Unicode 字符的预算。
可用 `--max-read-note-chars` 修改默认值，或用单次请求的 `max_chars`（命令行
`--max-chars`）覆盖。
`max_chars` 按 Unicode 字符计数，到达预算后会补齐当前行，因此返回内容可能超过预算；
可使用 `next_line` 无遗漏地继续读取。
全文和行范围读取跳过 AST 解析；大纲和标题读取在扫描块结构后只解析标题的行内内容；
block ID 读取保留完整解析。这些路径仍读取源文件，并遵守 `max_note_bytes`。

## Bases 与 Canvas 内容理解

`list_files` 发现可见的 `.base`、`.canvas` 和附件路径，不解析文件内容；
`list_notes` 继续只列出 Markdown。`read_base` 执行带类型的公式、全局及视图过滤、
展示字段、排序、分组、limit 和汇总，将 table、cards、list、kanban 及官方 Maps
视图统一表达为 Markdown 表格。先用 `query_base_views` 按文件顺序列出视图名称和类型，
它不执行公式或插件视图，忠实保留无名称与重复名称。`read_base.view`（CLI `--view`）
只接受精确且唯一的名称，名称缺失或重复会报错；省略时选择第一视图。`context_note` 指定嵌入宿主作为
`this`，省略时 `this` 指向 Base 自身。响应仅包含 `path`、`view`、`view_type`、
`output_format: "markdown"`、`content` 和 `pagination`（`page`、`total_pages`）。
每个数据行自身携带完整的 vault 相对路径：
`file.name`、`file.basename` 链接到该路径，未配置身份列的视图会补充 `file.path` 列，
保留原有列显示名称。已解析的笔记链接使用完整路径，Unicode 保持可读。

`read_canvas` 输出完整 Mermaid，保留全部文本、明确类型的文件/图片引用、URL、
带标签箭头和显式分组成员关系。响应仅包含 `path`、`output_format: "mermaid"`、
`content` 和 `pagination`（`page`、`total_pages`），分组推断提示保留为 Mermaid 注释。
路径及 subpath 均保留，不需要打开图片或复现坐标。
两项读取均不依赖正在运行的 Obsidian 桌面，只读且保持 `read_note` 原始 Markdown 契约。

渲染分页按 Unicode 字符预算 `max_chars` 保留完整语义单元，因此单行、节点及必要上下文
可能超过预算。在 `page < total_pages` 时请求 `page + 1`，后续页保持相同预算。
汇总针对应用视图 limit 后的
全部结果，不仅当前页。渲染内容不提供源文件行号引用。详细范围与限制见
[语义读取说明](docs/semantic-readers.md)，接口见[生成的工具契约](docs/tools.md)。

## 分页与过滤

分页工具接受固定数字 `page`。页码从 1 开始，`page: 1` 是第一页。没有 cursor，也没有不透明翻页 token。如果 `page` 超过总页数，响应会保留请求的页码，返回空结果列表，并同时返回分页总数。

请求级路径过滤使用 vault-relative Markdown note 路径。省略或传入空 `include` 数组时不额外限制范围；否则 note 至少要匹配一个 include。多个 include 是并集，多个 exclude 也是并集；exclude 命中时总是优先排除。

`get_note_neighborhood` 刻意不分页。它返回由 `depth` 和 `direction` 控制的有界已解析链接邻域；如果结果太宽，应缩小目标或深度。需要分页的关系健康检查时，使用 `audit_links --page 1`。

## reference 与 path

path 是 vault-relative Markdown note 路径，例如 `人物/林动.md`。当 `list_notes` 或其他工具已经返回具体路径时，直接传 path 最清晰。

CLI 的 note/reference/file 参数若明确以 `./` 或 `../` 开头，则相对于进程当前工作目录。
例如在 `vault/人物/草稿` 中，`read-note ./林动.md` 读取 `人物/草稿/林动.md`，
`read-note ../林动.md` 读取 `人物/林动.md`。裸路径 `人物/林动.md`、stem 和 alias
保持原有 vault-relative 解析。heading、block、line selector 和 wikilink 包裹语法均保留，
例如 `read-note '[[../林动.md#身体|林动]]'` 和 `read-note './林动.md#L3-L10'`。

同一 CLI 规则适用于 Base/Canvas 路径、Base `--context-note`，以及编辑、创建和重命名的
源/目标路径。精确 file-path 字段保留文件名中的 `#` 和 `|`，仅 note/reference 字段解析
selector 和 alias。正文、patch、glob、tag 和 category 不会被当作路径改写。
明确的 cwd 相对路径必须留在选定 root 内，symlink 解析也不能越界；路径不存在时直接失败，
不会搜索另一篇同名 note。项目模式的 root 是进程当前目录，因此 `../` 不能逃出该目录。
已配置的 vault 仍然优先，显式参数或环境变量指定无效 root 时会报错，不回退到项目模式。
MCP 参数保持 workspace-relative 语义，不采用 CLI 的进程 cwd 相对规则。

reference 是 Obsidian 风格目标，例如 `[[林动#身体]]`、`林动#身体` 或 `人物/林动.md#L1-L20`。reference 可以指向 note、标题、block id 或行范围。人类写法需要先校验时，用 `resolve_ref`。

歧义 reference 不会被猜测。工具要么要求唯一解析的目标，要么报告歧义，让调用方选择具体 path 或 selector。

## 工具契约

详细 MCP input/output 契约见 [docs/tools.md](docs/tools.md)。

主要读取与查询工具：

- `list_notes`
- `list_files`
- `query_base_views`
- `read_base`
- `read_canvas`
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

- `create_note`
- `delete_note`
- `edit_note`
- `set_frontmatter_field`
- `apply_patch`
- `append_section`
- `replace_section`
- `delete_section`
- `rename_heading`
- `rename_note`
- `set_block_id`
- `edit_history`
- `undo_edit`
- `redo_edit`

## 推荐使用顺序

1. 先运行 `list_notes --page 1`。
2. 如果问题属于已知目录或子集，先加路径过滤。
3. 用 `get_note_outline` 选择要读取的标题。
4. 用 `read_note` 读取最小有用证据范围：path、heading、block id 或行范围。
5. 用 `resolve_ref`、`get_outlinks`、`get_backlinks` 或 `get_note_neighborhood` 处理明确的本地链接问题。
6. 用 `search_text`、`search_regex`、标签、分类和 frontmatter 查询处理尚未锚定到单篇 note 的召回问题。
7. 大范围重构或审计依赖链接可靠性时，先用 `audit_links`。

## MCP 配置

MCP 客户端在目标 vault 内启动服务时，无需传入 vault 参数：

```json
{
  "context_servers": {
    "obsidian-vault": {
      "command": "/path/to/obsidian-vault-mcp",
      "args": ["serve"]
    }
  }
}
```

自动发现依据服务进程的工作目录，而不是可执行文件所在目录。
请让客户端在目标 vault 或其子目录中启动服务；如果客户端从其他目录启动，
可在 `serve` 前显式添加 `--vault` 和 vault 路径，或设置 `OBSIDIAN_VAULT_MCP_ROOT`。

协议输出只写 stdout；日志留在 stderr。

## 可观测性

服务通过 `tracing` 把日志写到 stderr，stdout 只用于 MCP 协议或 CLI JSON。`--log-level` 默认是 `debug`：

```sh
obsidian-vault-mcp --log-level debug serve
```

生命周期事件可关联 CLI 和 MCP 的工作：`logging.initialized` 表示日志已初始化；每条 CLI command 都会发出 `cli.command.start`，随后发出 `cli.command.ok` 或 `cli.command.error`。每次 MCP tool 调用仍会创建 `mcp.tool` span 并保留 `tool.call.*` events。start 事件会在 `input.preview` 字段记录 CLI 与 MCP 输入参数的预览，最多 1 KiB；`input.truncated=true` 表示已截断。error 事件会保留完整错误链，成功命令和工具的输出正文不会复制到日志中。

## 安全边界

这个 server 没有数据库、向量索引、文件 watcher、落盘解析缓存或编辑日志；编辑历史只保存在内存中。默认操作读取 vault；写入只通过显式编辑工具完成。每次工具调用都会检查文件元数据，变化后的文件会在使用前重新解析。

隐藏路径和 gitignore 命中的路径默认不会进入可见 notes，避免 Obsidian 配置、git 数据、agent skill 文件或生成物进入常规 note 结果。
