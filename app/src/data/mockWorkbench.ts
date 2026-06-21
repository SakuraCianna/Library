import type { WorkbenchSnapshot } from "../types/workbench";

export const mockWorkbench: WorkbenchSnapshot = {
  activeSpaceId: "space-interview",
  activeScope: "current_folder",
  sessionPermission: "approval",
  spaces: [
    {
      id: "space-interview",
      name: "面试",
      path: "D:\\知识库\\面试",
      defaultPermission: "approval",
      changedFileCount: 2,
      ocrQueueCount: 1,
    },
    {
      id: "space-springboot",
      name: "SpringBoot",
      path: "D:\\知识库\\SpringBoot",
      defaultPermission: "readonly",
      changedFileCount: 0,
      ocrQueueCount: 0,
    },
    {
      id: "space-work",
      name: "工作项目A",
      path: "D:\\知识库\\工作项目A",
      defaultPermission: "readonly",
      changedFileCount: 1,
      ocrQueueCount: 0,
    },
  ],
  files: [
    {
      id: "file-java-docx",
      name: "Java面试八股.docx",
      extension: ".docx",
      status: "indexed",
      statusLabel: "已索引",
    },
    {
      id: "file-redis-pdf",
      name: "Redis缓存.pdf",
      extension: ".pdf",
      status: "changed",
      statusLabel: "已变更",
    },
    {
      id: "file-interview-xlsx",
      name: "面试题.xlsx",
      extension: ".xlsx",
      status: "indexed",
      statusLabel: "表格模型就绪",
    },
  ],
  blockPreview: {
    id: "block-redis-cache-penetration",
    title: "知识块预览",
    excerpt:
      "Redis 缓存穿透：请求查询不存在的数据，缓存和数据库都无法命中，导致请求直接打到数据库。",
    sourceFileName: "Redis缓存.pdf",
  },
  tablePreview: {
    id: "table-interview-question-bank",
    title: "表格理解",
    description:
      "识别工作表、表头、字段含义、单位和可问答指标，不做复杂报表仪表盘。",
  },
  messages: [
    {
      id: "msg-user-1",
      role: "user",
      content: "问：Redis 缓存穿透怎么回答面试？",
    },
    {
      id: "msg-assistant-1",
      role: "assistant",
      content: "可以从定义、风险、解决方案和追问点四段回答。我会引用 3 个来源块。",
    },
  ],
  pendingAction: {
    id: "action-flash-card-draft",
    label: "待批准操作：生成复习卡草稿，批准后保存。",
    requiresApproval: true,
  },
};
