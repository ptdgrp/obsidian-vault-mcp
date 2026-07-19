Blueprint
├── Intent：用户到底要达成什么
├── Constraints：不能偏离什么，违反后结果不可接受
├── Definition of Done：何时算整体完成，判断整体是否已经交付
├── Plan：整体采用什么路线
├── Todo Graph：工作如何分解、依赖和分配
│  └── Execution State：现在执行到哪里，从 Todo Graph 派生
├── Results：已经产生了什么
├── Rubric：如何评价最终结果（可选）
├── Notes：无法归入以上结构的补充信息
└── Revision History：计划为什么发生变化

Blueprint Todo:（Todo Graph 中的每一个节点）
├── Plan： 如何完成这个任务
├── Completion Criteria：什么条件下可以声明完成
├── Handoff：后续如何继续
├── Result：已经产生了什么
├── Evidence：存在那些证据（可选）
├── Notes：局部补充信息（可选）
└── Revision History：任务为什么发生变化
