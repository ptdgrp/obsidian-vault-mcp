# 面向任务查询与紧凑响应实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**目标：** 依两份定稿 spec 重塑 MCP/CLI 查询面，并将保留工具的 JSON 收缩为最小、可继续调用的响应。

**架构：** query 层新增本次调用临时 LinkIndex、ResolvedReference、Locator 与 PageSlice<T>。所有集合先在完整可见 note 集上解析、过滤、自然排序和去重，再固定页切片；server.rs、main.rs 只做相同名称、参数和错误的适配。

**技术栈：** Rust 2024、RMCP/schemars、Clap、serde、globset、现有 parser/resolver、Cargo。

## 全局约束

- 两份 docs/superpowers/specs/2026-07-13-*.md 是不可修改的公共 API 契约。
- 不保留兼容别名：移除 find_unresolved_links、find_ambiguous_links、get_vault_graph、get_graph_neighborhood、collect_note_context、collect_reference_context、list_vault_files、get_tags、get_categories。
- note、target、reference 走 Obsidian resolver；path、new_path 是安全、vault-relative、带 .md 的精确 path。附件内容、cursor、page_size、持久化 index、兼容期均不实现。
- 可选字段和数组为空时不输出；audit_links 两数组与 get_outlinks.targets 是 spec 明定始终存在的例外。禁止用全局字节上限截断 JSON。
- 固定页：list_notes=100；audit 每类/outlinks/backlinks/search=50；outline/tags/categories/tag/category/frontmatter=100；neighborhood 不分页，限 50 notes/100 links。
- 建议仅用于查找已有目标，Levenshtein distance <=3，再按 distance、自然 path 排序；reference 建议保留 heading/block suffix；new_path 不建议。
- 开始实施前运行 git status --short；当前未跟踪 .superpowers/ 和已有计划是用户改动，不覆盖、不暂存、不提交。

## Spec → 当前代码 → 任务 → 验证映射

| Spec 要求 | 当前代码位置与差异 | 任务 | 验证 |
| --- | --- | --- | --- |
| locator、完整 heading reference、containment、固定数字页 | src/query.rs 的 SearchSource/ResolveSummary 分散；VaultConfig.max_results 截断 | 1 | cargo test query::tests::contract_primitives |
| 精确 path、<=3 建议和 suffix | vault.rs::read_note 自动补 .md；query.rs::find_indexed_note 建议不足 | 1 | cargo test resolver::tests::public_reference_contract |
| list_notes 仅 path/title/pagination | query/notes.rs::list_notes 输出 size、无 filter/page | 2 | cargo test list_notes_ 与 CLI/MCP JSON 比较 |
| audit_links 合并两次扫描 | query/graph.rs 两个 find 方法分开扫描、输出 evidence | 3 | cargo test audit_links_ |
| get_note_neighborhood 替代 graph/context | graph.rs 暴露 status/tags；context.rs 是旧 collector | 3 | cargo test note_neighborhood_ |
| resolve/outlinks/backlinks 紧凑化 | query/links.rs 用 LinkEvidence、snippet、verbose | 4 | cargo test resolve_ref_ outlinks_ backlinks_ |
| structure/outline/read/stats 紧凑化 | query.rs 暴露 parser dump、树 outline、next_step、mode/source | 5 | cargo test note_structure_ note_outline_ read_note_ note_stats_ |
| search/tag/category/frontmatter 固定页 | search.rs 有 context_lines；links.rs 为复数 tag bucket；categories.rs 为复数输出 | 6 | cargo test search_ list_tags_ get_tag_ list_categories_ get_category_ query_frontmatter_ |
| section locator、rename dry_run、精确 rename path | mutation.rs 有 note/line；rename.rs 使用宽松 path | 7 | cargo test mutation::tests 与 CLI JSON |
| schema/help/docs 无旧名/字段 | server.rs、main.rs、README、docs/tools.md 均仍暴露旧面 | 8、9 | 名称集合、help、生成 docs、rg |

## 拆分判断与执行顺序

不拆成多个彼此独立的计划。两个 spec 共享同一公共面：LinkIndex 同时服务 audit/neighborhood，ResolvedReference 同时服务 resolver/links/stats/mutation，PageSlice 同时决定所有 schema 与 CLI。拆开会产生不符合定稿契约的中间 API。

本计划由 9 个可独立测试、可独立提交的任务组成。1 为所有任务前置；2、3 依赖 1；4、5、6 依赖 1，5 的 stats 复用 4 的 containment；7 依赖 1；8 汇总 2–7；9 依赖 8。执行：1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9。

---

### 任务 1：公共 primitives、严格 path 与 reference 错误边界

**文件：**

- 新建 src/query/public.rs：Pagination、PageSlice<T>、Locator、ResolvedReference、Unicode 展示截断。
- 修改 src/query.rs：注册 public 模块，迁移 DTO 依赖。
- 修改 src/resolver.rs：保留完整 heading path。
- 修改 src/vault.rs：新增 resolve_exact_note_path。
- 修改 src/query/notes.rs::find_indexed_note、src/query/edit_distance.rs。
- 测试：新建 src/query/tests/contract_primitives.rs 并在 mod.rs 注册；修改 resolver/tests.rs、vault/tests.rs。

**接口：**

~~~rust
pub(crate) struct Pagination { pub page: usize, pub total_pages: usize }
pub(crate) struct PageSlice<T> {
    items: Vec<T>, total_items: usize, pagination: Pagination,
}
impl<T> PageSlice<T> {
    fn new(items: Vec<T>, page: usize, size: usize) -> anyhow::Result<Self>;
}
pub(crate) struct ResolvedReference {
    pub path: String, pub heading_path: Vec<String>, pub block_id: Option<String>,
}
impl ResolvedReference { fn format(&self) -> String; fn contains(&self, other: &Self) -> bool; }
pub(crate) struct Locator;
impl Locator { fn lines(path: &str, start: u64, end: u64) -> String; }
pub fn resolve_exact_note_path(&self, path: &str) -> Result<Utf8PathBuf, VaultError>;
~~~

- [ ] **步骤 1：写失败测试**

在 contract_primitives.rs 断言 page=0 失败、空集合 total_pages=0、越界页为空但 totals 真实；Locator 单行输出 path#L12、范围输出 path#L12-L38；heading 包含后代 heading 而不包含 block，block 仅精确包含自身。vault/resolver 测试断言精确 path 缺 .md、绝对 path、父目录逃逸失败；错误目录 reference 推荐 人物/林动.md#身体，distance=4 不推荐，ambiguous 自然排序。

- [ ] **步骤 2：确认 RED**

运行：cargo test query::tests::contract_primitives && cargo test resolver::tests && cargo test vault::tests

预期：编译失败，缺少 public、PageSlice、Locator、ResolvedReference 或 resolve_exact_note_path。

- [ ] **步骤 3：最小实现**

PageSlice 切片前计算 complete totals，空集合 total_pages=0，不读取 max_results。ResolvedReference 格式为 path#parent#child 或 path#^block；contains 仅允许同 path heading prefix 包含后代 heading、block 精确相等。建议比较规范化 target 与可见 resolver 名称/完整 path，过滤 distance>3，按 distance/natord(path) 排序，只返回唯一可靠候选并拼回 suffix。resolve_exact_note_path 先检查相对 path 与 .md 后缀。

- [ ] **步骤 4：确认 GREEN**

运行：cargo test query::tests::contract_primitives && cargo test resolver::tests && cargo test vault::tests && cargo fmt --check

预期：通过；公共 JSON 不含 byte offset、cursor、page_size。

- [ ] **步骤 5：提交**

~~~bash
git add src/query/public.rs src/query.rs src/resolver.rs src/vault.rs src/query/notes.rs src/query/edit_distance.rs src/query/tests/mod.rs src/query/tests/contract_primitives.rs src/resolver/tests.rs src/vault/tests.rs
git commit -m "refactor: add compact query contract primitives"
~~~

### 任务 2：重做 list_notes

**文件：** 修改 src/query.rs 的 ListNotesResult/NoteSummary、src/query/notes.rs::list_notes、src/server.rs、src/main.rs；测试 src/query/tests/mod.rs、src/server/tests/dispatch_more.rs、tests/cli.rs。

**接口：**

~~~rust
fn list_notes(&self, include: &[String], exclude: &[String], page: usize)
  -> anyhow::Result<ListNotesResult>;
// {notes:[{path,title?}],pagination:{page,total_pages,total_notes}}
~~~

- [ ] **步骤 1：写失败测试**

构造 101 notes、草稿、201 字符 title 和一个不可解析但可枚举 .md。断言 include 并集、exclude 优先、过滤后自然排序再分页；第 2 页一项；第 3 页为 notes 空、page=3、total_pages=2、total_notes=101；空 vault totals 为 0；item 仅 path/title，title 以 … 截至 200 字符。增加同 fixture MCP dispatch/CLI 调用，比较 serde_json::Value。

- [ ] **步骤 2：确认 RED**

运行：cargo test list_notes_ && cargo test server::tests::dispatch_more::list_notes_ && cargo test --test cli list_notes

预期：失败，当前签名没有 filters/page，仍输出 size。

- [ ] **步骤 3：最小实现**

使用 PathFilter 和任务 1 的固定 100 PageSlice。保持 H1→frontmatter→filename title 优先；title 提取失败只省略 title。新增带 serde 默认的 ListNotesRequest 与 CLI --include/--exclude/--page，两个 adapter 均转发同一 query 方法。

- [ ] **步骤 4：确认 GREEN**

运行：cargo test list_notes_ && cargo test server::tests::dispatch_more::list_notes_ && cargo test --test cli list_notes && cargo fmt --check

预期：通过；page=0 错误含 page，MCP/CLI JSON 一致。

- [ ] **步骤 5：提交**

~~~bash
git add src/query.rs src/query/notes.rs src/server.rs src/main.rs src/query/tests src/server/tests/dispatch_more.rs tests/cli.rs
git commit -m "feat: paginate compact note listings"
~~~

### 任务 3：LinkIndex、audit_links、get_note_neighborhood 与旧图工具删除

**文件：**

- 新建 src/query/link_index.rs。
- 修改 src/query.rs、src/query/graph.rs、src/server.rs、src/main.rs。
- 删除 src/query/context.rs、src/query/files.rs。
- 测试 src/query/tests/mod.rs、src/query/tests/edge_cases.rs、src/server/tests/dispatch_more.rs、tests/cli.rs。

**接口：**

~~~rust
LinkIndex::build(&VaultQueries) -> anyhow::Result<LinkIndex>;
fn audit_links(&self, page: usize) -> anyhow::Result<AuditLinksResult>;
fn get_note_neighborhood(&self, target: &str, depth: usize, direction: NeighborhoodDirection)
  -> anyhow::Result<NeighborhoodResult>;
~~~

- [ ] **步骤 1：写失败测试**

为 audit 写 unresolved 单/多行 source、ambiguous 21 candidates、两类各 51 occurrence，断言同一 page 分别固定 50、total_pages 取较大页数、omitted_candidates=1。为 neighborhood 写环、重复 occurrence、交叉边、坏链接，断言只遍历 resolved unique note 对、center 不重复、links 只在保留 nodes 间；测 depth 1–3 与 out/in/both，51 notes/101 induced links 产生准确 omission。加入一个无法读/解析的可见 note，断言两个操作失败且报 path。

- [ ] **步骤 2：确认 RED**

运行：cargo test audit_links_ && cargo test note_neighborhood_ && cargo test server::tests::dispatch_more::task_oriented_tools_

预期：失败，当前没有新 DTO/method，旧 graph/context/file handler 仍注册。

- [ ] **步骤 3：最小实现**

LinkIndex 严格读取/解析全部可见 note，禁止 filter_map(ok)；一次 resolver 结果分 resolved/unresolved/ambiguous occurrence。audit 按 source path/line range 排序，独立 50 分页。neighborhood 先完整 BFS，再按 distance/path 保留 50 非中心 notes；保留 nodes 的诱导子图按 from/to 取 100 unique links，omitted_links 只计该图。删除旧 query、handler、CLI variant，无 wrapper。

- [ ] **步骤 4：确认 GREEN**

运行：cargo test audit_links_ && cargo test note_neighborhood_ && cargo test query::tests::edge_cases && cargo test server::tests::dispatch_more::task_oriented_tools_ && cargo test --test cli task_oriented

预期：通过；越界 audit 两数组为空且 totals 真实。

- [ ] **步骤 5：提交**

~~~bash
git add src/query/link_index.rs src/query.rs src/query/graph.rs src/server.rs src/main.rs src/query/tests src/server/tests/dispatch_more.rs tests/cli.rs
git rm src/query/context.rs src/query/files.rs
git commit -m "feat: add task-oriented link audit and neighborhood tools"
~~~

### 任务 4：紧凑 resolve_ref、outlinks、backlinks

**文件：** 修改 src/query.rs、src/query/links.rs、src/resolver.rs、src/server.rs、src/main.rs；测试 src/query/tests/mod.rs、src/resolver/tests.rs、src/server/tests.rs、tests/cli.rs。

**接口：**

~~~rust
fn resolve_ref(&self, reference: &str) -> anyhow::Result<ResolveRefResult>;
fn get_outlinks(&self, note: &str, page: usize) -> anyhow::Result<OutlinksResult>;
fn get_backlinks(&self, target: &str, include: &[String], exclude: &[String], page: usize)
  -> anyhow::Result<BacklinksResult>;
~~~

- [ ] **步骤 1：写失败测试**

添加 resolved multi-heading、ambiguous、带建议 unresolved、无建议 unresolved 四个完整 JSON snapshot。outlink 测试完整 occurrence 先排序、固定 50 页再分 targets/ambiguous_targets/unresolved_targets，ambiguous candidates 保留 selector。backlink fixture 覆盖 whole note、heading descendants、父级/兄弟排除、block exact、同 source line/target 去重和 source filters 在 totals/page 前生效。

- [ ] **步骤 2：确认 RED**

运行：cargo test resolve_ref_ && cargo test outlinks_ && cargo test backlinks_ && cargo test --test cli get_backlinks

预期：失败，旧 status/resolution/links/verbose 形状仍在。

- [ ] **步骤 3：最小实现**

删除 ResolveSummary、LinkEvidenceOutput、verbose。resolved 仅 target；ambiguous 仅自然排序 paths；unresolved 仅 normalized target 和唯一建议。backlinks 先唯一 resolve target，扫描时忽略 unresolved/ambiguous links，并使用 ResolvedReference.contains。outlinks 分页后分组；backlinks 分页后按实际 normalized target 聚合 target/sources。禁止 status、alias、snippet、section、source object、match kind。

- [ ] **步骤 4：确认 GREEN**

运行：cargo test resolve_ref_ && cargo test outlinks_ && cargo test backlinks_ && cargo test server::tests && cargo test --test cli && cargo fmt --check

预期：通过，空 optional groups 缺席，输出 reference/locator 可再次解析。

- [ ] **步骤 5：提交**

~~~bash
git add src/query.rs src/query/links.rs src/resolver.rs src/server.rs src/main.rs src/query/tests src/resolver/tests.rs src/server/tests.rs tests/cli.rs
git commit -m "feat: compact reference and link query responses"
~~~

### 任务 5：紧凑 note inspection、read、stats

**文件：** 修改 src/query.rs、src/query/notes.rs 的 get_note_structure/read_note/get_note_stats、src/query/outline.rs::get_note_outline、src/query/section.rs、src/server.rs、src/main.rs；测试 src/query/tests/search_and_section.rs、src/query/tests/mod.rs、src/server/tests.rs、tests/cli.rs。

**接口：**

~~~rust
// structure: note, link_count, frontmatter_fields?, headings?, embeds?, tags?, blocks?, omitted?
// outline: note, headings, pagination
// read: source, content, truncated?
// stats: scope, word_count, character_count, line_count, backlink_count
~~~

- [ ] **步骤 1：写失败测试**

创建 51 headings/tags、frontmatter object/array、H1、重复 embeds/tags/blocks。断言 structure 不输出 YAML value/parser link/H1，omitted 只含超限类别。outline 测第 1/2/越界页文档顺序和 slash paths。read 测完整 source，未截断无 truncated、截断才 true。stats 测 normalized scope、不回显 word mode，并复用任务 4 containment。

- [ ] **步骤 2：确认 RED**

运行：cargo test note_structure_ && cargo test note_outline_ && cargo test read_note_ && cargo test note_stats_

预期：失败，仍输出 path/frontmatter/links/children/source/next_step/word_count_mode 或 outline 无 page。

- [ ] **步骤 3：最小实现**

structure 生成自然排序去重 string groups，headings 为非 H1 path.join("/")/line，各组完整排序再取 50。read 用 Locator.lines，删除 path/next_step。stats 用 ResolvedReference.format，删除 mode/source。outline 展平非 H1 headings，保持文档顺序，固定 100 page；删除 outline heading/ancestor-chain 输入。

- [ ] **步骤 4：确认 GREEN**

运行：cargo test note_structure_ && cargo test note_outline_ && cargo test read_note_ && cargo test note_stats_ && cargo test server::tests && cargo test --test cli

预期：通过，输出 heading 可作 read_note 输入。

- [ ] **步骤 5：提交**

~~~bash
git add src/query.rs src/query/notes.rs src/query/outline.rs src/query/section.rs src/server.rs src/main.rs src/query/tests src/server/tests.rs tests/cli.rs
git commit -m "feat: compact note inspection responses"
~~~

### 任务 6：统一 search、tags、categories、frontmatter 分页 DTO

**文件：** 修改 src/query.rs、src/query/search.rs、src/query/links.rs 的 list_tags/get_tags/query_frontmatter、src/query/categories.rs、src/server.rs、src/main.rs；测试 src/query/tests/search_and_section.rs、src/query/tests/mod.rs、src/server/tests/dispatch_more.rs、tests/cli.rs。

**接口：**

~~~rust
fn search_text(&self, query: &str, case_sensitive: bool, include: &[String], exclude: &[String], page: usize) -> anyhow::Result<SearchResult>;
fn search_regex(&self, pattern: &str, case_sensitive: bool, include: &[String], exclude: &[String], page: usize) -> anyhow::Result<SearchResult>;
fn get_tag(&self, tag: &str, scope: TagScope, include: &[String], exclude: &[String], page: usize) -> anyhow::Result<TagMatches>;
fn get_category(&self, category: &str, include: &[String], exclude: &[String], page: usize) -> anyhow::Result<CategoryNotes>;
~~~

- [ ] **步骤 1：写失败测试**

同一行两个 literal/regex match 仅一条；preview 围绕首次 match，Unicode-safe <=240，只有裁切一端才有相应省略号。每个集合覆盖空/首页/部分末页/越界/page=0。get_tag 覆盖 note/frontmatter/body/section/line locator 粒度与去重；get_category 清理边界空白/斜线、内部斜线失败；frontmatter exists 拒绝 value，equals/regex 必须 value，JSON 无 YAML value。

- [ ] **步骤 2：确认 RED**

运行：cargo test search_ && cargo test list_tags_ && cargo test get_tag_ && cargo test list_categories_ && cargo test get_category_ && cargo test query_frontmatter_

预期：失败，当前要求 context_lines/复数 tags/categories，回显 query/pattern/value 或 buckets。

- [ ] **步骤 3：最小实现**

search 保存首次 match 位置、生成中心 preview，删除 context line 物化和 max_results 截断，以 50 page。tag/category/frontmatter 全部先 PathFilter、完整收集、自然排序去重、100 page。完全替换 get_tags/get_categories 的 request/command/handler 为单数 API，不留 alias；frontmatter 增 include/exclude/page，只输出 paths。

- [ ] **步骤 4：确认 GREEN**

运行：cargo test search_ && cargo test list_tags_ && cargo test get_tag_ && cargo test list_categories_ && cargo test get_category_ && cargo test query_frontmatter_ && cargo test --test cli

预期：通过，literal/regex JSON keys 一致，空 optional 字段缺席。

- [ ] **步骤 5：提交**

~~~bash
git add src/query.rs src/query/search.rs src/query/links.rs src/query/categories.rs src/server.rs src/main.rs src/query/tests src/server/tests/dispatch_more.rs tests/cli.rs
git commit -m "feat: paginate compact discovery query responses"
~~~

### 任务 7：section mutation locator 与 rename 精确 path

**文件：** 修改 src/mutation.rs、src/mutation/edit.rs、src/mutation/rename.rs、src/server.rs、src/main.rs；测试 src/mutation/tests/mod.rs、src/mutation/tests/rename_more.rs、src/server/tests.rs、tests/cli.rs。

**接口：**

~~~rust
pub struct EditSectionResult { pub changed: String }
// RenameResult 保持 { dry_run, updated_references, changed_notes }
fn rename_note(&self, path: &str, new_path: &str, dry_run: bool) -> anyhow::Result<RenameResult>;
~~~

- [ ] **步骤 1：写失败测试**

append/replace JSON 精确为 changed locator；delete 对首/中/末/空文档返回 post-edit 有效 line。断言 rename_note 的无 .md、绝对、逃逸 path 均失败且无写入；三种 rename 的 dry_run/apply 无 old/new selector、changed_notes 完整自然排序、目标自身不计 updated_references。

- [ ] **步骤 2：确认 RED**

运行：cargo test mutation::tests && cargo test mutation::tests::rename_more && cargo test --test cli rename

预期：失败，edit 仍返回 note/line，rename_note 接受无 .md。

- [ ] **步骤 3：最小实现**

原子写成功后用更新文本重算 changed：append/replace 指向 inserted/replaced span；delete 将原 start clamp 到 1..=max(lines,1)。rename_note 对 path/new_path 调用任务 1 exact resolver，写前计算全部 edits/changed_notes；dry_run 字段名不变，不新增分页/截断。

- [ ] **步骤 4：确认 GREEN**

运行：cargo test mutation::tests && cargo test mutation::tests::rename_more && cargo test server::tests && cargo test --test cli && cargo fmt --check

预期：通过，成功 changed locator 能 read_note 复查，失败无文件变化。

- [ ] **步骤 5：提交**

~~~bash
git add src/mutation.rs src/mutation/edit.rs src/mutation/rename.rs src/server.rs src/main.rs src/mutation/tests src/server/tests.rs tests/cli.rs
git commit -m "feat: compact mutation result contracts"
~~~

### 任务 8：MCP schema、CLI help、删除面门禁

**文件：** 修改 src/server.rs、src/main.rs、src/server/tests.rs、src/server/tests/dispatch_more.rs、tests/cli.rs。

**接口：** 唯一 tool 集为 list_notes、audit_links、get_note_neighborhood、get_note_structure、get_note_outline、read_note、get_note_stats、search_text、search_regex、resolve_ref、get_outlinks、get_backlinks、list_tags、get_tag、list_categories、get_category、query_frontmatter、append_section、replace_section、delete_section、rename_note、rename_heading、rename_block_id。

- [ ] **步骤 1：写失败测试**

tool_definitions 名称集合与上述集合作等值比较。逐个检查 collection 有 page 且无 page_size/cursor；filter tools 有 include/exclude；outlinks/backlinks/tag 无 verbose；search 无 context_lines；outline 无 heading。CLI 对每个删除 command/flag 断言 unknown；新 command help 与 MCP description 同一句任务定义。

- [ ] **步骤 2：确认 RED**

运行：cargo test server::tests::dispatch_more::public_tool_set_ && cargo test --test cli removed_commands

预期：失败，仍发现旧 handler、旧 flag 或 schema 字段。

- [ ] **步骤 3：最小实现**

删除无调用点旧 request/tool macro/Command variant/dispatch/helper/import，禁止 runtime fallback。逐项对齐 tool description、Clap doc comment、serde default 与 CLI default；错误只由同一 query/mutation 层传播。

- [ ] **步骤 4：确认 GREEN**

运行：cargo test server::tests && cargo test server::tests::dispatch_more && cargo test --test cli && cargo run -- --help

预期：通过，help/schema 仅暴露定稿名称与字段。

- [ ] **步骤 5：提交**

~~~bash
git add src/server.rs src/main.rs src/server/tests.rs src/server/tests/dispatch_more.rs tests/cli.rs
git commit -m "chore: remove deprecated query tool surfaces"
~~~

### 任务 9：生成文档、spec 自审、最终验证

**文件：** 修改 README.md、README.zh-CN.md，并由命令更新 docs/tools.md；不改其他 plans。

- [ ] **步骤 1：写失败文档检查**

运行：

~~~bash
cargo run -- generate-docs --check
rg -n "find_unresolved_links|find_ambiguous_links|get_vault_graph|get_graph_neighborhood|collect_note_context|collect_reference_context|list_vault_files|get_tags|get_categories|verbose|context_lines|page_size|cursor|include_unresolved" README.md README.zh-CN.md docs/tools.md src tests
~~~

预期：更新前生成检查或扫描显示旧 API。

- [ ] **步骤 2：最小文档实现**

运行 cargo run -- generate-docs 重建 docs/tools.md。更新双语 README：四层模型为 notes 枚举、内部结构、外部关系、问题关系审计；示例从 list_notes --page 1 开始；说明固定数字 page、越界空页、include 并集/exclude 优先、reference/path 区别、audit_links、非分页 neighborhood。删除旧 file list、graph/context、verbose/context_lines、附件内容、兼容说明。

- [ ] **步骤 3：确认 GREEN**

运行：

~~~bash
cargo run -- generate-docs --check
rg -n "find_unresolved_links|find_ambiguous_links|get_vault_graph|get_graph_neighborhood|collect_note_context|collect_reference_context|list_vault_files|get_tags|get_categories|verbose|context_lines|page_size|cursor|include_unresolved" README.md README.zh-CN.md docs/tools.md src tests
~~~

预期：第一条 exit 0；第二条无匹配且 exit 1，这是成功条件。

- [ ] **步骤 4：逐项 spec 自审与全量验证**

重读两份 spec，核对：工具新增/删除对应任务 2/3/8；fixed page 对应 1–6；reference/path/suggestion 对应 1/4/7；最小 JSON/省略规则对应 1–7；mutation 对应 7；文档对应 9。再运行：

~~~bash
cargo fmt --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
git diff --check
git status --short
~~~

预期：前四条 exit 0；状态仅包含本计划实现改动与执行前既有用户未跟踪文件，后者不暂存。

- [ ] **步骤 5：提交**

~~~bash
git add README.md README.zh-CN.md docs/tools.md
git commit -m "docs: describe task-oriented compact MCP tools"
~~~

## 计划自审结果

- 已逐项覆盖两份 spec 的工具、删除项、输入、分页、LinkIndex、locator/reference、建议阈值、错误原子性、mutation、文档。
- 已检查命名使用 audit_links、get_note_neighborhood、get_tag、get_category 与保持不变的 dry_run；旧名称只作为删除扫描目标。
- 每项都给出文件、接口、失败测试、RED 命令、最小实现、GREEN 命令和独立提交范围，无占位步骤。
