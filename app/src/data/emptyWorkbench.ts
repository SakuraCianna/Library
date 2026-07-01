import type { WorkbenchSnapshot } from "../types/workbench";

export const emptyWorkbench: WorkbenchSnapshot = {
  activeSpaceId: "",
  activeConversationId: null,
  activeScope: "current_folder",
  sessionPermission: "readonly",
  spaces: [],
  files: [],
  parseJobs: [],
  blockPreview: {
    id: "block-empty",
    title: "暂无知识块",
    excerpt: "请先添加一个真实文件夹作为知识库。",
    sourceFileName: "暂无来源文件",
    sourceLocator: "暂无来源定位",
  },
  tablePreview: {
    id: "table-empty",
    title: "表格理解等待接入",
    description: "本阶段先完成文件扫描入库，表格结构洞察会在解析 xlsx 后显示。",
  },
  messages: [
    {
      id: "1",
      conversationId: "default",
      role: "system",
      content: "这是一个基于 Tauri + React 的知识库应用。",
      sources: [],
      createdAt: new Date().toISOString(),
    },
  ],
  pendingAction: null,
};
