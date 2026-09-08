# 议题索引

这里保存已经明确进入项目范围、需要继续讨论或留档的设计议题。
临时摘录、尚未成形的想法和一次性实验记录请放在 `.scratch/`。

## 专题

- [附件读取](attachment-reading/README.md)：只读访问 PDF、图片和 Office 附件，并向 LLM 返回有界、可定位的内容。
- [Markdown 导出](markdown-export/README.md)：将项目内 Markdown 稳定导出为 HTML、DOCX 和 PDF。
- [全文与语义检索](full-text-search/README.md)：以可选 Tantivy 全文检索和本地向量检索提供可定位的资料召回（前置依赖：附件读取）。

## 状态约定

- `待处理`：问题已经明确，尚未得出结论。
- `受阻`：必须先完成其“依赖”议题。
- `已解决`：结论已记录；如果实现条件改变，应新建议题，不直接抹去旧结论。

每个专题的 `README.md` 负责说明目标、范围和当前决策；详细论证保留在对应议题中。
