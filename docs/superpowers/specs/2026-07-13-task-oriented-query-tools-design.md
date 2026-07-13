# 面向任务的查询工具设计

## 摘要

将 8 个功能重叠的知识库查询工具收敛为 3 个面向任务的工具：

| 新公开工具 | 替代或删除的工具 |
| --- | --- |
| `audit_links` | 合并 `find_unresolved_links` 和 `find_ambiguous_links` |
| `get_note_neighborhood` | 替代 `get_graph_neighborhood`、`collect_note_context` 和 `collect_reference_context` |
| `list_notes` | 重做现有 `list_notes` 契约，并删除 `list_vault_files` |

删除 `get_vault_graph`，因为当前没有全图使用需求。MCP 与 CLI 必须使用同一套名称和语义，不提供兼容别名。

最终的发现模型是：

- `list_notes`：发现有哪些 note。
- `get_note_structure`：查看一篇 note 内部有什么。
- `get_note_neighborhood`：查看一篇 note 与哪些 note 相连。
- `audit_links`：检查本地链接的完整性问题。

## 设计原则

1. 一个工具只表达一个清晰任务，不以多模式万能工具追求最少数量。
2. 删除不会改变 agent 下一步决策的字段。
3. 返回面向任务的 DTO，不直接暴露 parser 或 resolver 内部类型。
4. vault-relative path 必须完整、稳定，并能直接用于后续调用。
5. 关系结果不完整时必须显式说明，不能把部分索引伪装成完整结果。
6. MCP 与 CLI 的名称、过滤、分页、错误和输出保持一致。

## 公开 API 变化

以下 MCP 工具和 CLI 命令直接删除，不保留别名：

- `find_unresolved_links`
- `find_ambiguous_links`
- `get_vault_graph`
- `get_graph_neighborhood`
- `collect_note_context`
- `collect_reference_context`
- `list_vault_files`

保留 `list_notes` 名称，但修改其输入和输出契约。新增：

- `audit_links`
- `get_note_neighborhood`

## `list_notes`

### 目的

分页返回可见 Markdown note 的轻量导航清单。它不枚举附件，不统计目录，不返回修改时间或 note outline。

### 输入

```json
{
  "include": ["人物/**"],
  "exclude": ["**/草稿/**"],
  "page": 1
}
```

- `include` 默认为空数组。多个 vault-relative glob 取并集；空数组不增加限制。
- `exclude` 默认为空数组，并且优先级高于 `include`。
- 请求过滤只能进一步缩小 vault 全局配置定义的可见范围。
- `page` 默认 1，且必须大于等于 1。
- 页大小固定为 100，不作为输入或输出字段。
- 先过滤，再按 vault-relative path 自然排序，最后分页。

### 输出

```json
{
  "notes": [
    {
      "path": "人物/林动.md",
      "title": "林动"
    }
  ],
  "pagination": {
    "page": 1,
    "total_pages": 3,
    "total_notes": 243
  }
}
```

- 每条 note 只有 `path` 和可选的 `title`。
- title 优先级保持为：首个 H1、frontmatter title、文件名。
- note 可枚举但 title 提取失败时省略 `title`。
- 展示 title 最多 200 个字符，超出后以省略号截断；path 永不截断。
- 空结果返回 `total_notes: 0`、`total_pages: 0`。
- page 超出末页时成功返回空 `notes`，并保留请求 page 与真实 totals。
- 不返回 size、modified、目录统计、附件统计或 page size。

CLI 使用同一契约：

```text
list_notes --include '人物/**' --exclude '**/草稿/**' --page 2
```

## `audit_links`

### 目的

一次执行全 vault 本地链接健康检查，同时返回 unresolved 和 ambiguous 链接。调用方不需要为同一个“哪些链接不安全”问题扫描两次 vault。

### 输入

```json
{
  "page": 1
}
```

- `page` 默认 1，且必须大于等于 1。
- 两类结果分别按 source 的自然路径和行范围排序。
- 每类固定每页 50 条，同一个 page 同时作用于两类。

### 输出

```json
{
  "unresolved": [
    {
      "source": "剧情/第一章.md#L42",
      "target": "不存在的人物"
    }
  ],
  "ambiguous": [
    {
      "source": "剧情/第二章.md#L18",
      "target": "林动",
      "candidates": [
        "人物/林动.md",
        "旧稿/林动.md"
      ]
    }
  ],
  "totals": {
    "unresolved": 31,
    "ambiguous": 4
  },
  "pagination": {
    "page": 1,
    "total_pages": 2
  }
}
```

- `source` 使用可直接定位的 `path#Lx` 或 `path#Lx-Ly`。
- unresolved 条目只含 source 和 target。
- ambiguous 条目额外包含 candidate paths。
- 单条 ambiguous link 最多返回 20 个 candidates；超出时增加 `omitted_candidates`，否则省略该字段。
- `total_pages` 取两类页数的较大值。
- 某一类在当前页没有结果时返回空数组。
- page 同时超出两类末页时成功返回两个空数组和真实 totals。
- 不返回 alias、snippet、嵌套 resolution、raw reference 或 candidate match kind。
- 需要原文时，调用方可将紧凑行引用传给 `read_note`。

## `get_note_neighborhood`

### 目的

返回一篇 note 周围受限且可靠的关系邻域。它同时接受 note 标识和 Obsidian reference，替代旧 context collectors 与 graph neighborhood 入口。

### 输入

```json
{
  "target": "[[林动#身体]]",
  "depth": 1,
  "direction": "both"
}
```

- `target` 接受 vault-relative path、stem、alias、wikilink、heading reference 或 block reference。
- `depth` 默认 1，允许范围为 1 到 3。
- `direction` 为 `out`、`in` 或 `both`，默认 `both`。
- 只有唯一解析成功的本地链接参与遍历。
- 同一 source note 到 target note 的多次链接 occurrence 合并为一条有向邻域 link；精确 occurrence 由 `get_outlinks` / `get_backlinks` 提供。
- 问题链接不作为邻域 mode；它们统一由 `audit_links` 负责。
- 固定最多返回 50 个邻居 note 和 100 条 link，不暴露限额参数。
- 邻域不分页，因为分页会破坏关系图的完整语义。

### 输出

```json
{
  "center": {
    "path": "人物/林动.md",
    "title": "林动",
    "heading": "身体"
  },
  "notes": [
    {
      "path": "设定/境界体系.md",
      "title": "境界体系",
      "distance": 1
    },
    {
      "path": "剧情/第一章.md",
      "title": "第一章",
      "distance": 1
    }
  ],
  "links": [
    {
      "from": "人物/林动.md",
      "to": "设定/境界体系.md"
    },
    {
      "from": "剧情/第一章.md",
      "to": "人物/林动.md"
    }
  ]
}
```

- center 不在 notes 中重复出现。
- 输入选择 heading 或 block 时，center 才分别包含 `heading` 或 `block_id`。
- 邻居只包含 path、可选 title 和到 center 的最短 distance。
- notes 按 distance、自然 path 排序。
- title 使用与 `list_notes` 相同的 200 字符限制。
- links 只包含 resolved `from` / `to`，并按 from、to 自然排序。
- links 是唯一有向 note 对，不返回 status、alias、raw target、snippet 或 source span。
- direction 控制 BFS 可沿哪些 link 扩展；返回 links 则包含已保留节点之间的所有 resolved link，以保留交叉关系。
- `truncated`、`omitted_notes`、`omitted_links` 只在实际省略时出现。
- 先计算完整请求深度的邻域，再按 distance/path 保留前 50 个非 center notes。
- `omitted_notes` 是其余合格 notes 数；在保留节点组成的子图内，再保留前 100 条唯一 links，`omitted_links` 只统计该子图内被省略的 links，避免与 omitted notes 重复计数。

## 内部架构

引入内部、非持久化的 `LinkIndex`：

```text
可见 Markdown notes
  -> 读取并解析
  -> 解析全部本地链接
  -> LinkIndex {
       notes,
       resolved_links,
       unresolved_links,
       ambiguous_links
     }
  -> audit_links / get_note_neighborhood
```

`LinkIndex` 是每次查询基于当前 vault 构建的视图，不是数据库、watcher 或落盘索引。它复用现有 parse cache，使两个公共工具共享完全一致的可见性和解析规则。

公共返回 DTO 保持面向任务，不能仅因为内部类型数据更多就将 parser/resolver 结构泄漏到 MCP 或 CLI。

## 错误与部分结果策略

### `list_notes`

- glob 无效或 `page < 1` 时失败。
- page 越界时返回空成功结果。
- title 提取失败时省略 title，但保留可见 path。

### `audit_links`

- `page < 1` 时失败；page 越界时返回空成功结果。
- 任一可见 note 无法读取或解析时，整个调用失败并指出 path。
- 不允许返回可能制造错误健康结论的部分审计结果。

### `get_note_neighborhood`

- target unresolved 时失败，并在有可靠候选时提供最近路径建议。
- target ambiguous 时失败并列出候选路径。
- depth 越界时失败。
- 正确反链依赖完整可见 note 集，因此任一可见 note 无法读取或解析时整个调用失败。
- 达到固定节点或 link 限额不是错误；返回显式 omission 字段。

任何工具都不能在全局输出字节边界直接截断 JSON。可计数的省略使用上述契约表达，否则调用显式失败。

## 文档与发现

- MCP description 与 CLI help 使用相同的一句话任务定义。
- README、README.zh-CN、`docs/tools.md`、示例和推荐流程删除全部旧名称。
- 文档统一介绍四层模型：枚举 notes、检查内部结构、检查外部关系、审计问题关系。
- `list_notes` 文档使用固定数字分页，不使用游标。

## 验证

### `list_notes`

- include 取并集，exclude 优先。
- 过滤发生在自然排序和固定分页之前。
- 空 vault、最后一页和越界页符合契约。
- 输出不存在 size、page size、modified、目录或附件字段。
- MCP 与 CLI 序列化结构一致。

### `audit_links`

- 一次调用同时返回 unresolved 和 ambiguous。
- 两类独立分页，顶层 total_pages 取较大值。
- source 覆盖单行与多行定位。
- candidates 超过 20 时准确报告省略数。
- 可见 note 解析失败时不能返回部分审计。

### `get_note_neighborhood`

- 覆盖 path、stem、alias、heading reference 与 block reference。
- 覆盖三种 direction 和 depth 1–3。
- 环与重复链接不会产生重复 note。
- 只沿 resolved links 遍历。
- 节点/link 限额准确报告省略数。
- 输出不存在 tags、sizes、status、snippet、alias、raw target 或 source span。
- unresolved 错误在可用时含建议，ambiguous 错误列出候选。

### 删除与一致性

- MCP schema 和 CLI help 不再暴露 7 个删除名称。
- 仓库文档和示例不存在过期调用。
- 测试断言完整公共 JSON 形状，而不只检查个别字段。

## 不在范围内

- 兼容别名或弃用期。
- 附件枚举。
- 全 vault 图导出或可视化。
- 持久化索引、数据库或文件 watcher。
- vault 并发变化期间的跨页快照一致性。
- 将 `get_note_structure` 与关系查询合并。
