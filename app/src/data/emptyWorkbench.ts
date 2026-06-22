import type { WorkbenchSnapshot } from "../types/workbench";

export const emptyWorkbench: WorkbenchSnapshot = {
  activeSpaceId: "",
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
  },
  tablePreview: {
    id: "table-empty",
    title: "表格理解等待接入",
    description: "本阶段先完成文件扫描入库，表格结构理解将在后续解析阶段接入。",
  },
  messages: [
    {
      id: "msg-empty",
      role: "system",
      content: "请点击新建选择一个真实文件夹。",
      sources: [],
    },
  ],
  pendingAction: null,
};
