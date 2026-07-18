# Blueprint v2 设计

## 目标

将 Blueprint 从“单个 Markdown 文件内嵌全部 Todo”的协议，升级为基于项目本地多文档、可在 Obsidian 中直接阅读和编辑的多 Agent 计划协议。

Blueprint v2 保留 v0.1 中已经有效的部分：

- 显式记录 Intent、Constraints、Definition of Done 和高层 Plan；
- 使用一个中央 Todo Graph，并以 Obsidian Task marker 作为 Todo 执行状态的唯一事实来源；
- 根据依赖派生 readiness，支持多 Agent 恢复和交接；
- Markdown 是协议的唯一事实来源；
- 使用 ETag 防止过期覆盖，并通过锁串行化写入。

v2 补充三个缺失部分：

- 由路由 Agent 或 Skill 针对当前目标生成的 Rubric；
- 可由 Results 引用的显式 Evidence；
- 不依赖 Git commit、用于解释语义变化的追加式 Revision History。

Todo 的身份、状态和图关系继续集中在 `blueprint.md`；可能很大的 Todo 执行内容迁移到每个 Todo 独立的文档中。

## 非目标

- Blueprint 不是通用工作流引擎，不直接执行 Skill 或脚本。
- Rubric 不限定为数值评分或通过/失败判断。
- Evidence 不会被系统自动解释为证明、事实或支持关系。
- Revision History 不是文本 diff、事件溯源系统或文件历史替代品。
- v1 尚未投入使用，因此不提供 v1 兼容或自动迁移。
- Todo 文件不独立保存状态、Owner、依赖或父子关系。

## 工作区目录

工作区使用 `blueprint/v2` schema：

```text
<vault>/
└── .blueprint/
    ├── manifest.md
    ├── blueprints/
    │   └── bp-<id>/
    │       ├── blueprint.md
    │       └── todos/
    │           ├── todo-<id>.md
    │           └── todo-<id>.md
    ├── .locks/
    │   └── bp-<id>.lock
    └── .tmp/
```

`manifest.md` 继续作为工作区协议标记：

```markdown
---
schema: blueprint/v2
---

# Blueprint Workspace
```

Blueprint 生命周期变化时，Blueprint 和 Todo 的路径都不改变。稳定路径保证标准 Markdown 链接、依赖引用和跨文档 Evidence 引用不会失效。

## Blueprint Frontmatter 与生命周期

`blueprint.md` 使用最小 frontmatter：

```yaml
---
schema: blueprint/v2
id: bp-01K...
state: active
---
```

保留三个生命周期状态：

- `active`：仍可继续执行；
- `closed`：已正式结束，Results 中记录完整或不完整的 Closure；
- `cancelled`：整体 Intent 已被明确放弃。

Frontmatter 中的 `state` 只是生命周期元数据，不是 Execution State。Execution State 始终根据 Definition of Done 和中央 Todo Graph 实时派生。

`blueprint_close` 和 `blueprint_cancel` 更新 frontmatter，并在 Results 中追加原有的关闭或取消记录，但不移动目录。除读取外，非 active Blueprint 拒绝其他修改。`blueprint_list` 读取 frontmatter 后按状态过滤。

## 一等 Section 模型

Source 层不再只匹配一个硬编码 Blueprint 模板，而是将 Markdown 文档解析为有序 Section 模型：

```text
ParsedDocument
├── Frontmatter
├── H1
└── Section[]
    ├── title
    ├── heading source range
    ├── body source range
    └── original source
```

每种文档 schema 声明必需且唯一的 H2 Section。未知 Section 仍然合法；修改无关内容时，必须逐字节保留未知 Section。领域解析器消费 Section，但不再各自实现 Markdown 范围定位。

Blueprint 和 Todo 仍使用定点源文本编辑，禁止解析后重渲染整个文档。

## Blueprint 文档

新建 `blueprint.md` 时，按以下规范顺序生成必需 Section：

```text
Record
Intent
Constraints
Definition of Done
Plan
Rubric
Todos
Results
Evidence
Revision History
Notes
```

职责如下：

```text
Record               Blueprint 责任元数据
Intent               用户期望的最终结果和工作范围
Constraints          执行时不能违反的边界
Definition of Done   整体完成条件
Plan                 高层方法和路线
Rubric                Agent 针对当前目标选择的评估程序
Todos                 Todo Graph 的唯一事实来源
Results               整体成果、决策和开放问题
Evidence              Blueprint 级 Results 引用的证据
Revision History      Blueprint 语义变化及其原因
Notes                 尚未结构化的信息
```

Record 继续保存 `Created By`、`Closed By` 和 `Cancelled By`。Frontmatter 面向机器生命周期和过滤；Record 面向人类责任记录。

## 中央 Todo Graph

`Todos` Section 是 Todo 身份、状态和图关系的唯一事实来源。每个可执行 Todo 是一个 Obsidian Task，并使用标准 Markdown 链接指向详情文档：

```markdown
- [/] [完成初稿](todos/todo-01K1DRAFT.md) ^todo-01K1DRAFT
  - Created By: planner
  - Owner: writer
  - Depends On: todo-01K1RESEARCH
```

中央图保存：

- Todo ID 和标题；
- Todo 文档链接；
- task marker：`pending`、`in_progress`、`completed`、`blocked` 或 `cancelled`；
- Created By、Owner 和 Completed By；
- Depends On；
- Children 下的父子嵌套；
- Block Reason 和 Cancel Reason。

中央图不再内嵌可能很大的 Completion Criteria、Handoff、Results、Evidence、Revision History 或 Notes。

子 Todo 继续嵌套在 `Children:` 下，并各自链接独立 Todo 文档。依赖和父子图继续遵守 v0.1 校验规则。

## Todo 文档

每个 Todo 使用稳定的 ID 文件名。最小 frontmatter 只识别文档，不重复图状态：

```yaml
---
schema: blueprint/todo/v2
id: todo-01K...
blueprint: bp-01K...
---
```

Todo 文件包含以下必需 Section：

```text
Intent
Completion Criteria
Plan
Handoff
Results
Evidence
Revision History
Notes
```

Todo H1 必须与中央图中的 Markdown 链接文本一致。标题更新在 Blueprint 锁内同时修改两处。

Todo 文档职责如下：

```text
Intent               当前 Todo 的局部目标
Completion Criteria  完成前必须满足的局部 checkbox
Plan                 局部执行方法
Handoff              后继 Agent 所需的最小上下文
Results               局部成果和结论
Evidence              当前 Todo 收集或产生的证据
Revision History      当前 Todo 语义变化及其原因
Notes                 尚未结构化的局部信息
```

Todo frontmatter 不保存 status、Owner、dependency、parent、Block Reason 或 Cancel Reason，避免形成冲突的双重事实来源。

## Rubric

Rubric 是路由 Agent 或 Skill 在理解用户目标后，为当前任务编写的评估程序。路由器可以选择已有 Skill、专用 Agent、脚本和工具，再将本次任务实际采用的评估流程写入 Rubric。

Rubric 以 Markdown 为主，可以描述：

- 评估意图和审视维度；
- 选中的评估 Skill、Agent、脚本或工具标识符；
- 为什么当前目标需要这些评估能力；
- 必须收集的观察或 Evidence；
- 期望输出；
- 局限和禁止作出的解释；
- 如何保留分歧和不确定性。

Rubric 不要求分数或二元正确性。文学任务可以要求连续性审查和读者模拟；代码任务可以要求测试和静态分析。

Blueprint MCP 只保存并暴露 Rubric，不执行 Rubric。执行 Agent 读取 Rubric、载入所引用的能力、完成评估，并写入 Results 和 Evidence。

Rubric 是必需且非空的。无需专用评估器的简单任务，也必须说明如何核对 Definition of Done 和 Constraints。

## Evidence

Evidence 直接写在收集它的 Blueprint 或 Todo 文档中，并位于独立的 `Evidence` Section。Evidence 不保存为中央 Evidence 文件。

每条 Evidence 在整个 Blueprint 目录内拥有全局唯一的 `^evidence-*` Block ID。Evidence 正文保持自由 Markdown，从而能够表达命令输出、文本观察、用户反馈、成果文件或具有歧义的文学解释。

示例：

```markdown
## Evidence

### 主角反复表现出对家庭关系的依恋 ^evidence-01K1A

- Collected By: literary-continuity
- Observation: 第一章两处场景都强调主角害怕失去家庭关系。
- References:
  - [家庭争执](../../story.md#家庭争执)
  - [深夜谈话](../../story.md#深夜谈话)
- Limitations:
  - 同样的段落也可能意味着主角正在压抑离开的愿望。
```

Results 使用带 heading 或 Block ID fragment 的标准 Markdown 相对链接引用 Evidence：

```markdown
- Evidence:
  - [家庭依恋](#^evidence-01K1A)
  - [背景研究](todo-01K1RESEARCH.md#^evidence-01K1B)
```

Evidence 可以引用其他 Todo 中的 Evidence。Evidence 引用不参与 Todo readiness，也不会被自动解释为支持、反驳或事实；这些关系继续由正文显式说明，除非未来实践证明需要类型化关系。

协议生成的链接必须使用标准 Markdown link 语法。Blueprint v2 不生成或要求 Wiki Link。解析器校验 Evidence ID 在整个 Blueprint 内唯一，并校验 Blueprint 内部 Evidence 链接能够解析。项目正常目录中的外部 Reference 不要求通过 Blueprint Store 解析。

completed Todo 必须具有非空 Results，并至少定义或引用一条有效 Evidence。这只验证 Evidence 存在，不代表 Evidence 在语义上充分；充分性由 Rubric 和 Agent 审查判断。

## Revision History

Revision History 是 Blueprint 和 Todo 文档中的独立、追加式 Section。它替代 v0.1 中“Git commit 会可靠保存协议历史”的错误假设。

Revision History 记录语义变化的原因，而不是每次写入。适用变化包括：

- Blueprint Intent、Constraints、Definition of Done、Plan 或 Rubric；
- Todo Intent、dependencies、Completion Criteria 或 Plan；
- 对 Results 或 Evidence 的重大重新解释、修正或失效；
- 当原因不能从操作记录直接看出时，计划工作的创建、取消或重组。

执行状态迁移、Handoff 刷新、Evidence 收集、Results 更新和普通措辞调整，不自动产生 revision。

每条 revision 使用 `^revision-*` Block ID，并至少记录：

- Changed By；
- Reason；
- Change summary；
- 受影响的 Section 或 Todo ID；
- 当已有 Evidence 或 Results 可能失效时，记录 Evidence impact。

修改语义字段的类型化操作必须接收 `changed_by` 和 `change_reason`，并在同一次锁定写入中追加 Revision。公共工具不能替换或删除已有 Revision 条目。Revision History 中未知 Markdown 仍必须保留。

## Results 与完成条件

Todo Results 保存局部成果并引用相关 Evidence。Blueprint Results 保存整体成果、deliverables、decisions 和 open questions，可以引用任何 Todo 文档中的 Evidence。

`todo_complete` 要求：

- 中央图中的当前状态为 `in_progress`；
- Todo 文件中的 Completion Criteria 全部勾选；
- 所有未取消的子 Todo 均已完成；
- Todo Results 非空；
- 至少存在一条 Evidence 定义或有效 Evidence 引用；
- `completed_by` 非空。

`blueprint_close` 保留完整和不完整 Closure 语义。完整关闭要求全部 Definition of Done 已勾选、所有 Todo 已 completed 或 cancelled、Blueprint Results 非空，并且 Blueprint Results 至少包含一条有效 Evidence 引用。

不完整关闭必须提供原因，可以把缺失的 Results 或 Evidence 记录为开放工作。关闭只记录 Agent 的显式结论，不表示 MCP 在语义上认同 Rubric 评估结果。

## Execution State 与读取视图

Execution State 始终实时派生，不写入 Markdown。服务从中央图读取 Todo 状态和依赖，只在具体操作或视图需要时加载 Todo 详情文件。

`blueprint_status` 返回生命周期状态，以及派生的 DoD 数量、Todo 状态数量、ready/not-ready 依赖、blocked、unassigned 和 open Todo ID。

`blueprint_get(view: "full")` 返回 `blueprint.md`、对应 ETag 和 Todo 文档索引，不把全部 Todo 源文本拼成一个巨大响应。

`blueprint_get(view: "resume")` 返回 Blueprint Intent、Constraints、未完成 Definition of Done、Plan、Rubric、当前 Results，以及紧凑的 active/ready/blocked Todo 摘要。相关 Todo 的 Handoff 从独立文件加载。

`todo_get` 返回中央图元数据、Todo 文档内容、Todo 文档 ETag、Results、Evidence 和派生 readiness。

## 存储、锁与 ETag

同一 Blueprint 内的所有修改共用一把 Blueprint 锁，保证中央图更新、详情编辑、跨文档 Evidence 校验和生命周期变化被串行化，不引入多锁协议。

`blueprint.md` 和每个 Todo 文件分别拥有内容 ETag：

- 修改中央图或 Blueprint Section 时接收 `expected_blueprint_etag`；
- 修改 Todo 文档时接收 `expected_todo_etag`；
- 同时修改两者的操作可以接收两个 ETag，并在获得 Blueprint 锁后校验。

创建 Todo 时，先暂存 Todo 文档，最后提交中央图链接。进程内失败时删除暂存文件。读取和写入遇到中央图引用的 Todo 文件缺失，或遇到没有中央图引用的 `todo-*.md` 孤儿文件时，返回可修复的一致性错误，不能静默忽略。

完成 Todo 时，先写入并验证 Todo Results/Evidence，最后修改中央 task marker。进程中断最多留下“内容已完成但 Todo 仍为 in_progress”的可安全重试状态，不得留下“中央图已 completed 但 Todo 内容无效”的状态。

每个文件仍通过 `.tmp` 原子替换。协议不宣称跨多个文件的 crash-atomic transaction。

## MCP 契约变化

v2 在用途仍成立时保留现有 Blueprint、DoD 和 Todo 操作名，但输入输出需要适应聚合目录和文档级 ETag。

主要变化：

- `blueprint_create` 要求非空 Rubric，并创建 Blueprint 目录及 `blueprint.md`；
- `blueprint_get` 返回 Todo 文档索引，而不是把全部内容嵌入一个响应；
- `blueprint_list` 根据 frontmatter state 过滤，不依赖生命周期目录；
- `blueprint_update` 可以更新 Rubric，语义变化必须提供 Revision 元数据；
- `blueprint_close` 和 `blueprint_cancel` 更新 frontmatter，但不移动路径；
- `todo_create` 同时创建中央图节点和 Todo 文档；
- `todo_get` 合并中央图元数据和 Todo 文档内容；
- `todo_update` 区分中央图字段与 Todo 文档字段，并接收对应 ETag；
- `todo_complete` 校验 Todo 文档中的 Completion Criteria、Results 和 Evidence；
- `dod_update` 仍属于执行状态更新，不自动创建 Revision。

增加两个面向追加内容的操作：

```text
evidence_add
revision_append
```

二者都接收文档 scope：`blueprint_id` 和可选 `todo_id`。`evidence_add` 生成全局唯一 Evidence Block ID，并向目标 Evidence Section 追加条目。`revision_append` 生成 Revision ID 并追加记录；类型化语义修改通常应自动追加所需 Revision。

不增加通用公共 `section_replace` 工具。一等 Section 是内部 Source 模型能力，公共领域操作继续保护协议约束。

## 校验与错误

每次读取和候选写入都校验：

- workspace、Blueprint 和 Todo schema 版本；
- Blueprint/Todo ID 与路径、frontmatter 是否一致；
- 各文档类型的必需 Section 是否存在且唯一；
- 中央图 task 语法和 Todo 文档链接；
- 每个 Todo ID 是否恰好对应一个图节点和一个文件；
- 依赖图、父子图、readiness 和状态不变量；
- Completion Criteria 位置与 checkbox 语法；
- Evidence 和 Revision ID 在 Blueprint 内是否唯一；
- Blueprint 内部 Evidence 链接是否可解析；
- completed Todo 是否满足 Results 和 Evidence 要求；
- 修改操作是否满足 active Blueprint 前置条件；
- 获得 Blueprint 锁后 ETag 是否仍匹配。

错误在适用时指出文档路径、Section 和 ID。候选写入在替换目标文件前被拒绝。多文件操作必须显式报告可恢复的中央图/详情不一致，不能隐藏错误。

## 兼容性

工作区 manifest 升级为 `blueprint/v2`。遇到 v1 manifest 时返回明确的 unsupported-schema 错误，不修改、不删除、也不自动迁移 v1 内容。

## 测试策略

实现遵循测试驱动开发。测试覆盖：

1. 一等 Section 解析、文档必需 schema、未知内容保留和定点编辑；
2. v2 工作区创建，以及生命周期变化前后的稳定聚合路径；
3. 中央图和 Todo 文档的创建、组合读取、标题更新、孤儿/缺失文件错误；
4. Completion Criteria 位于 Todo 文件时的状态迁移和 readiness；
5. Rubric 创建、更新、resume 暴露及必需 Revision；
6. Evidence 创建、全局唯一性、跨 Todo 链接、未解析内部链接和完成门槛；
7. Blueprint/Todo 追加式 Revision History；
8. 独立 Blueprint/Todo ETag 和 Blueprint 级串行化；
9. Todo 创建与完成操作的安全写入顺序；
10. frontmatter 生命周期过滤，以及不移动路径的关闭和取消；
11. 变更及新增 MCP 工具的 schema 和转发行为；
12. 全仓库测试、lint、格式化和文档生成。

## 完成标准

满足以下条件时，v2 设计目标完成：

- Blueprint 拥有稳定目录，生命周期状态保存在 frontmatter；
- `blueprint.md` 是 Todo Graph 的唯一事实来源，每个 Todo 拥有独立详情文档；
- Todo task marker 继续是执行状态的唯一事实来源；
- Rubric 能指导 Agent 选择评估能力，但不强制通用评分；
- Todo 和整体 Results 能引用独立 Evidence Section 中的证据；
- 语义变化形成追加式 Revision History，不依赖 Git；
- 多文档聚合下，派生状态、resume、锁、ETag 和保留源文本编辑继续有效；
- 所有聚焦验证和全仓库验证通过。
