# 紧凑 MCP 返回设计

## 状态与范围

本文档记录公共 API 噪音审计中逐项确认的返回契约。它独立于 `2026-07-13-task-oriented-query-tools-design.md`：前者把 8 个重叠查询工具收敛为 3 个面向任务的工具，本文则让其余公共工具更紧凑、更适合 LLM 调用。

本文覆盖审计中的全部剩余公共工具。被“面向任务的查询工具设计”删除或替代的工具不在此重复。

## 通用返回规则

1. 除非解析、规范化后的值会被后续调用使用，否则不回显请求参数。
2. 使用 `note.md#L12`、`note.md#Heading#Child` 等可直接使用的 locator/reference，不返回嵌套 source 或 parser 对象。
3. 可选字段和数组没有值时直接省略，不输出 `null` 占位。
4. 如果详细证据可通过定向 `read_note` 获取，则删除 `verbose` 模式。
5. 有序集合采用固定页大小的数字分页。page 从 1 开始；越界页成功返回空集合和真实 totals。
6. path 和 reference 永不截断；只有展示文本可以设限。
7. 多级 heading reference 必须保留完整路径，不能只留下最后一级。

## 输入命名与查找契约

输入字段名直接表达查找语义：

- `note`、`target`、`reference` 走 Obsidian reference resolver。单个工具允许的范围内，可传 stem、alias、wikilink、heading reference 或 block reference；无需 `.md` 后缀或 vault-relative 目录，因此示例优先使用 `林动`、`林动#身体`、`林动#^profile`。
- 当 stem 重名时，reference 字段仍允许传完整 vault-relative note path 来消歧。
- `path`、`new_path` 使用精确文件路径语法，必须是安全、带 `.md` 的 vault-relative path，例如 `人物/林动.md`；绝对路径和越出 vault 的路径报错。
- reference 查找失败时，将规范化后的目标与可见 resolver 名称和路径比较；精确 path 查找失败时，与可见 vault-relative paths 比较。
- 建议按编辑距离、自然路径排序；只有 Levenshtein distance 小于等于 3 的候选才有资格出现，不做远距离猜测。
- 修正 note 目标时必须保留原 heading/block suffix。
- 建议只用于查找已有 path；`new_path` 是目标位置，不进行相似路径建议。

## 引用包含关系

反向链接范围使用语法上的引用包含关系：

```text
Note
├── Note#Heading
│   └── Note#Heading#Child
│       └── Note#Heading#Child#Grandchild
└── Note#^block-id
```

- note scope 包含对 note 本身、所有 headings 和所有 blocks 的引用。
- heading scope 包含自身和后代 heading paths，不包含父级、兄弟或物理上位于章节内的 block reference。
- block scope 只包含精确 block reference，没有子 scope。
- block ID 与 heading 是平行的寻址系统。

## `get_outlinks`

### 输入

```json
{
  "note": "林动",
  "page": 1
}
```

- `page` 默认 1，必须大于等于 1。
- 固定每页 50 个 link occurrences。
- 删除 `verbose`。

### 输出

```json
{
  "note": "人物/林动.md",
  "targets": [
    {
      "source": "人物/林动.md#L42",
      "target": "设定/境界体系.md#淬体境"
    }
  ],
  "ambiguous_targets": [
    {
      "source": "人物/林动.md#L63",
      "reference": "林氏宗族",
      "candidates": [
        "势力/林氏宗族.md",
        "旧稿/林氏宗族.md"
      ]
    }
  ],
  "unresolved_targets": [
    {
      "source": "人物/林动.md#L51",
      "reference": "缺失设定"
    }
  ],
  "pagination": {
    "page": 1,
    "total_pages": 1,
    "total_links": 3
  }
}
```

- occurrences 按 source 排序、分页，再按解析结果分组。
- `targets` 始终存在，包含唯一解析成功的规范化 targets。
- `ambiguous_targets`、`unresolved_targets` 只在当前页非空时出现。
- ambiguous candidates 保留原 heading/block selector。
- 不返回 alias、section、snippet、嵌套 resolver、candidate match kind 或 status 字符串。

## `get_backlinks`

### 输入

```json
{
  "target": "林动#身体",
  "include": ["剧情/**"],
  "exclude": ["**/草稿/**"],
  "page": 1
}
```

- target 必须唯一解析成功。unresolved 在有候选时给路径建议；ambiguous 列出候选并失败。
- include/exclude 过滤 source note paths，exclude 优先。
- 固定每页 50 个 backlink occurrences。
- 删除 `verbose`。

### 输出

```json
{
  "scope": "人物/林动.md#身体",
  "references": [
    {
      "target": "人物/林动.md#身体",
      "sources": [
        "剧情/第一章.md#L12",
        "剧情/第二章.md#L18"
      ]
    },
    {
      "target": "人物/林动.md#身体#伤势",
      "sources": [
        "剧情/第三章.md#L27"
      ]
    }
  ],
  "pagination": {
    "page": 1,
    "total_pages": 1,
    "total_backlinks": 3
  }
}
```

- `scope` 是唯一解析成功、规范化后的查询 reference。
- 匹配 occurrences 先分页，再按实际规范化 target 分组。
- 同一 source line 指向同一 target 的重复链接只返回一次。
- ambiguous/unresolved links 无法安全归入 resolved scope，由 `audit_links` 负责。
- 不返回逐 occurrence alias、status、section、source object 或 snippet。

## `resolve_ref`

删除现有 `result` wrapper、status、raw input 回显、内部 `ReferenceInfo`、candidate match kind 和 null 字段。

Resolved：

```json
{
  "target": "人物/林动.md#身体#伤势"
}
```

Ambiguous：

```json
{
  "ambiguous_targets": [
    "人物/发动机.md#原理",
    "设定/发动机.md#原理"
  ]
}
```

Unresolved 且存在唯一建议：

```json
{
  "unresolved_target": "错误目录/林动#身体",
  "suggested_target": "人物/林动.md#身体"
}
```

Unresolved 且无建议：

```json
{
  "unresolved_target": "不存在的人物"
}
```

`target`、`ambiguous_targets`、`unresolved_target` 三个主字段只出现一个。`suggested_target` 只在 unresolved 且建议唯一可靠时出现。

## `get_note_structure`

`get_note_structure` 是受限结构概览，不是完整 parser dump。

### 输入

```json
{
  "note": "林动"
}
```

### 输出

```json
{
  "note": "人物/林动.md",
  "frontmatter_fields": [
    "aliases",
    "status"
  ],
  "headings": [
    {
      "heading": "身体",
      "line": 12
    },
    {
      "heading": "身体/伤势",
      "line": 20
    }
  ],
  "link_count": 14,
  "embeds": [
    "附件/林动.png",
    "设定/境界体系.md#淬体境"
  ],
  "tags": [
    "人物",
    "状态/活跃"
  ],
  "blocks": [
    "profile",
    "injury"
  ]
}
```

- `note` 是解析后的 vault-relative path。
- `frontmatter_fields` 只含自然排序的顶层字段名，不返回任意 YAML value。
- `headings` 只含可选择的非 H1 slash paths 和行号。
- `link_count` 统计本地链接 occurrences，详情交给 `get_outlinks`。
- embeds、tags、block IDs 规范化并去重；tags 合并正文与 frontmatter。
- 空数组字段省略；`note`、`link_count` 始终存在。
- 每个数组最多 50 项。发生省略时增加只包含受影响类别的 `omitted`：

```json
{
  "omitted": {
    "headings": 12,
    "tags": 3
  }
}
```

## `get_note_outline`

### 输入

```json
{
  "note": "林动",
  "page": 1
}
```

- 删除可选 heading/ancestor-chain mode。
- 固定每页 100 个非 H1 headings。

### 输出

```json
{
  "note": "人物/林动.md",
  "headings": [
    {
      "heading": "身体",
      "line": 12
    },
    {
      "heading": "身体/伤势",
      "line": 20
    },
    {
      "heading": "关系/绫清竹",
      "line": 46
    }
  ],
  "pagination": {
    "page": 1,
    "total_pages": 1,
    "total_headings": 3
  }
}
```

- 保持文档顺序。
- slash-separated paths 表达层级，并能直接传给 `read_note`。
- 删除 level、重复 heading path、递归 children、source 和 section。

## `read_note`

输入选择语义不变：使用 bare reference，或恰好一个显式 heading、block ID、line selector。保留请求级 `max_chars`。

完整结果：

```json
{
  "source": "人物/林动.md#L12-L38",
  "content": "## 身体\n\n……"
}
```

截断结果：

```json
{
  "source": "人物/林动.md#L12-L38",
  "content": "## 身体\n\n……",
  "truncated": true
}
```

- `source` 描述完整选中 span；truncated 存在时，content 是该 span 的字符前缀。
- 删除重复 path、嵌套 section、selector 回显、returned-character count 和静态 `next_step`。
- `truncated` 只在 true 时出现。

## `get_note_stats`

### 输入

```json
{
  "note": "林动#身体",
  "word_count_mode": "visible"
}
```

### 输出

```json
{
  "scope": "人物/林动.md#身体",
  "word_count": 824,
  "character_count": 2150,
  "line_count": 47,
  "backlink_count": 6
}
```

- `scope` 是规范化后的 note、heading 或 block reference。
- 不回显 word-count mode。
- heading backlink count 使用后代 heading containment；block 只精确匹配；whole note 包含直接 note、heading、block references。
- line range 不作为 stats scope，因为它不是可链接的语义范围。

## `search_text`

### 输入

```json
{
  "query": "祖符",
  "case_sensitive": false,
  "include": ["正文/**"],
  "exclude": ["**/草稿/**"],
  "page": 1
}
```

- 删除 `context_lines`。
- 固定每页 50 个匹配行。

### 输出

```json
{
  "matches": [
    {
      "source": "正文/第一章.md#L42",
      "preview": "……林动在石池中感应到了祖符的气息……"
    }
  ],
  "pagination": {
    "page": 1,
    "total_pages": 1,
    "total_matches": 1
  }
}
```

- 同一行即使出现多次 literal 也只返回一次。
- preview 最多 240 个字符，以首次匹配为中心；只有实际省略前后文本时才增加省略号。
- 不回显 query，不返回嵌套 section 或 truncated。
- 按 note 自然路径和行号排序。

## `search_regex`

`search_regex` 与 `search_text` 使用完全相同的 DTO、排序、固定页大小、过滤和 preview 规则。preview 以该行首次 regex match 为中心；不回显 pattern；无效 regex 显式失败。

示例输入：

```json
{
  "pattern": "祖符.{0,20}气息",
  "case_sensitive": false,
  "include": ["正文/**"],
  "exclude": ["**/草稿/**"],
  "page": 1
}
```

## `list_tags`

### 输入

```json
{
  "scope": "note",
  "include": ["人物/**"],
  "exclude": ["**/草稿/**"],
  "page": 1
}
```

### 输出

```json
{
  "tags": [
    "人物",
    "状态/活跃",
    "阵营/道宗"
  ],
  "pagination": {
    "page": 1,
    "total_pages": 1,
    "total_tags": 3
  }
}
```

- 固定每页 100 个唯一 tags。
- tags 规范化为不带开头 `#`，并自然排序。
- 不回显 scope 和 path filters。

## `get_tag`

将复数 `get_tags` 改名为 `get_tag`，每次只接受一个标签；删除原 `tags` 数组和 `verbose`。

### 输入

```json
{
  "tag": "状态/活跃",
  "scope": "section",
  "include": ["人物/**"],
  "exclude": ["**/草稿/**"],
  "page": 1
}
```

### 输出

```json
{
  "matches": [
    "人物/林动.md#身体",
    "人物/绫清竹.md#状态"
  ],
  "pagination": {
    "page": 1,
    "total_pages": 1,
    "total_matches": 2
  }
}
```

请求 scope 决定 locator 粒度：

- `note`：正文或 frontmatter 含 tag 的 note paths。
- `frontmatter`：frontmatter 含 tag 的 note paths。
- `body`：正文含 tag 的 note paths。
- `section`：含 tag 的规范化 heading references，按 section 去重。
- `line`：含 tag 的 line references，按 line 去重。

固定每页 100 个 locators。不返回 source kind、compact/detailed 双 occurrence、tag/scope 回显或 section object。

## `list_categories`

Category 使用目录名标签语义，而不是完整目录路径 identity。同名目录位于不同层级、卷或全局设定集时属于同一 category。

### 输入

```json
{
  "include": ["第一卷/**", "第二卷/**", "设定集/**"],
  "exclude": ["**/草稿/**"],
  "page": 1
}
```

### 输出

```json
{
  "categories": [
    "人物",
    "关系",
    "设定"
  ],
  "pagination": {
    "page": 1,
    "total_pages": 1,
    "total_categories": 3
  }
}
```

- 可见 note 的每个父目录 segment 都贡献一个 category 名称。
- 相同 segment 跨卷、子树和全局设定集取并集。
- 根目录 note 不产生空 category。
- 固定每页 100 个自然排序的唯一名称。
- 不回显 filters。

## `get_category`

将复数 `get_categories` 及其 `categories` 数组输入改为单个分类查询。

### 输入

```json
{
  "category": "关系",
  "include": ["第一卷/**", "第二卷/**", "设定集/**"],
  "exclude": ["**/草稿/**"],
  "page": 1
}
```

### 输出

```json
{
  "notes": [
    "第一卷/人物/关系/林动.md",
    "第二卷/人物/关系/绫清竹.md",
    "设定集/人物/关系/关系总览.md"
  ],
  "pagination": {
    "page": 1,
    "total_pages": 1,
    "total_notes": 3
  }
}
```

- 任一父目录 segment 等于规范化 category 名称时匹配。
- 同名目录跨位置匹配是有意的 union，不是 ambiguity。
- 去除首尾空白和 `/`；清理后仍含 `/` 时失败，因为 category 是目录名标签，不是路径。
- include/exclude 进一步限制匹配 notes。
- 固定每页 100 个自然排序的 paths。
- 不回显 category、filters、title、size 或 bucket wrapper。

## `query_frontmatter`

### 输入

```json
{
  "field": "phase",
  "mode": "equals",
  "value": "active",
  "include": ["第一卷/**", "第二卷/**"],
  "exclude": ["**/草稿/**"],
  "page": 1
}
```

- mode 保留显式 `exists`、`equals`、`regex`。
- exists 拒绝 value；equals 和 regex 必须带 value。
- 新增统一 include/exclude。
- 固定每页 100 个 notes。

### 输出

```json
{
  "notes": [
    "第一卷/人物/林动.md",
    "第二卷/人物/绫清竹.md"
  ],
  "pagination": {
    "page": 1,
    "total_pages": 1,
    "total_notes": 2
  }
}
```

- 只返回自然排序的 note paths。
- 不回显 field、mode、value 或 filters。
- 不返回任意 YAML arrays/objects；需要实际值时定向调用 `read_note`。
- 无效 regex 显式失败。

## 章节写操作返回

`append_section`、`replace_section`、`delete_section` 保持独立工具，但共享返回形状。

Append/replace：

```json
{
  "changed": "人物/林动.md#L20-L24"
}
```

Delete：

```json
{
  "changed": "人物/林动.md#L20"
}
```

- append 指向新插入内容，replace 指向替换后内容。
- delete 指向删除后应复查的最近有效行；夹在编辑后文档的有效范围内，空文档使用第 1 行。
- 使用一个 locator 替代 note、line_start、line_end。
- 不回显 selector、content、operation 或 success boolean。

## 重命名操作返回

`rename_note`、`rename_heading`、`rename_block_id` 保留现有返回契约：

```json
{
  "dry_run": true,
  "updated_references": 6,
  "changed_notes": [
    "人物/林动.md",
    "剧情/第一章.md",
    "剧情/第二章.md"
  ]
}
```

- `dry_run` 保留原名。
- `updated_references` 区分目标自身修改和引用修复。
- `changed_notes` 必须完整，不截断、不分页；变更预览必须是一组连贯变更。
- 不回显 old/new name 或 selector。

## 公开 API 变化汇总

- 从 `get_outlinks`、`get_backlinks`、`get_tag` 删除 `verbose`。
- `get_tags` 改名 `get_tag`，`tags[]` 改为 `tag`。
- `get_categories` 改名 `get_category`，`categories[]` 改为 `category`。
- 从 `search_text`、`search_regex` 删除 `context_lines`。
- 从 `get_note_outline` 删除可选 heading/ancestor-chain 输入。
- 为上述集合工具增加固定数字 `page`。
- 为 `query_frontmatter` 增加 include/exclude。

不提供兼容别名。MCP 与 CLI 的名称、参数、错误和序列化结果同步修改。

## 内部架构

公共 DTO 面向任务，与 parser、resolver、edit 内部类型分离。内部保留完整证据，公共序列化只选择各工具需要的字段。

共享内部组件：

- `ResolvedReference`：保留 resolved path、完整 heading path 或 block ID，负责格式化规范化 reference 并判断语法包含关系。
- `Locator`：统一格式化 path、line span、heading reference、block reference，不暴露 byte offsets 或嵌套 section。
- `PageSlice<T>`：校验 1-based page，计算 totals，按各工具固定页大小切片，并提供统一分页元数据。
- `PathFilter`：统一 include 并集和 exclude 优先规则。
- 展示 helpers：规范化 tags/categories，在 Unicode 字符边界截断 preview，并限制结构概览分组。

集合查询统一数据流：

```text
可见 notes
  -> 请求过滤
  -> 收集完整轻量 matches
  -> 确定性排序与去重
  -> 计算 totals
  -> 选择固定数字页
  -> 生成紧凑公共 DTO
```

这是实时文件系统分页，不提供跨调用快照保证；vault 并发变化可能移动后续页边界。

## 错误与输出边界

- page 0 失败；越界页成功返回空集合和真实 totals。
- 无效 glob/regex 失败，并指出对应字段。
- 精确 path 缺少 `.md`、为绝对路径或越出 vault 时失败。
- reference ambiguity 返回自然排序候选。
- missing reference/path 只在编辑距离小于等于 3 时建议，并在适用时保留 suffix。
- `get_backlinks` 要求唯一 resolved scope；扫描到的 ambiguous/unresolved links 不归入该 scope。
- 公共集合不能在全局输出字节边界静默截断；必须返回定义好的分页/省略结果或显式错误。
- 执行重命名前先计算并校验完整变更结果；完整 `changed_notes` 无法适配输出边界时，预览和执行都在写入前失败。
- 章节写操作采用原子写入，成功后才返回编辑后的定位符。

## 验证

### 通用契约

- MCP schema 与 CLI help 暴露相同的新名称、删除参数、page/filter 字段。
- 可选字段和数组无值时省略，只有文档明确说明的始终存在数组例外。
- 所有 locators 和规范化 references 可由对应 `read_note`/resolver 输入再次解析。
- 多级 heading 保留每一级。
- note/ref 和精确 path 失败遵守编辑距离 3 的建议边界。
- 固定分页覆盖空、首页、部分末页、越界页。

### 链接与 reference

- `get_outlinks` 将同一有序页分到 resolved、ambiguous、unresolved 数组，不返回 status/verbose 证据。
- `get_backlinks` 覆盖 whole note、heading descendant、exact block、兄弟/父级排除和 heading/block 独立性。
- backlink source 过滤发生在 totals 与分页之前。
- 文档规定的同 source line、同 target 重复链接正确去重。
- `resolve_ref` 覆盖 resolved、ambiguous、带建议 unresolved、不带建议 unresolved，且无旧 wrapper。

### 笔记检查

- `get_note_structure` 返回受限概览，省略空组并准确报告各组 omissions。
- `get_note_outline` 跨页保持文档顺序，并输出可直接选择的 slash paths。
- `read_note` 只返回一个紧凑 source；只有内容缩短时出现 truncated。
- `get_note_stats` 返回规范化 scope，并将 reference containment 应用于 backlink count。

### 搜索与元数据

- literal/regex search 共用同一 match DTO，并围绕首次匹配生成 Unicode-safe 240 字符 preview。
- 同一行多次匹配只产生一条结果。
- tag locator 使用请求的 note/frontmatter/body/section/line 粒度并按该粒度去重。
- 相同 category 目录名跨卷和全局设定集有意取并集。
- frontmatter query 不序列化任意匹配值，并在分页前应用 path filters。

### 写操作

- append/replace 返回准确 post-edit range，delete 返回 post-delete 复查边界。
- 重命名预览和执行保留 `dry_run`、准确的 `updated_references` 计数和完整的 `changed_notes`。

## 不在范围内

- 兼容别名或弃用期。
- 游标或快照一致性分页。
- 返回附件内容。
- 持久化 search/reference index。
- 通过其他布尔模式重新引入详细证据。
