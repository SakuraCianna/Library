import { invoke, isTauri } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

import { emptyWorkbench } from "../data/emptyWorkbench";
import type {
  PermissionMode,
  RuntimeStatus,
  WorkbenchSnapshot,
} from "../types/workbench";

const browserRuntimeStatus: RuntimeStatus = {
  deepseek: {
    configured: false,
    model: "deepseek-v4-flash",
    baseUrl: "https://api.deepseek.com",
    keyHint: "桌面端读取本机配置",
  },
  ocr: {
    configured: false,
    tier: "medium",
    modelDir: "桌面端读取本机模型目录",
    missingModels: ["PP-OCRv6_medium_det", "PP-OCRv6_medium_rec"],
  },
};

function isTauriRuntime() {
  return isTauri();
}

function deriveSpaceName(rootPath: string) {
  const normalizedPath = rootPath.split("/").join("\\");
  const segments = normalizedPath.split("\\").filter(Boolean);
  const lastSegment = segments[segments.length - 1];

  return lastSegment?.trim() || "新知识库";
}

function browserSnapshotForFolder(
  rootPath: string,
  defaultPermission: PermissionMode,
): WorkbenchSnapshot {
  const name = deriveSpaceName(rootPath);

  return {
    ...emptyWorkbench,
    activeSpaceId: "browser-preview-space",
    sessionPermission: defaultPermission,
    spaces: [
      {
        id: "browser-preview-space",
        name,
        path: rootPath,
        defaultPermission,
        changedFileCount: 0,
        scanQueueCount: 0,
        documentQueueCount: 0,
        ocrQueueCount: 0,
      },
    ],
    messages: [
      {
        id: "browser-preview-message",
        role: "system",
        content: "浏览器预览无法扫描本地文件，请在桌面应用中运行。",
      },
    ],
  };
}

export async function selectKnowledgeFolder(): Promise<string | null> {
  if (!isTauriRuntime()) {
    return window.prompt("输入要作为知识库的文件夹路径");
  }

  const selectedPath = await open({
    directory: true,
    multiple: false,
    title: "选择知识库文件夹",
  });

  return typeof selectedPath === "string" ? selectedPath : null;
}

export async function getWorkbenchSnapshot(): Promise<WorkbenchSnapshot> {
  if (!isTauriRuntime()) {
    return emptyWorkbench;
  }

  return invoke<WorkbenchSnapshot>("get_workbench_snapshot");
}

export async function getRuntimeStatus(): Promise<RuntimeStatus> {
  if (!isTauriRuntime()) {
    return browserRuntimeStatus;
  }

  return invoke<RuntimeStatus>("get_runtime_status");
}

export async function requestSessionPermission(
  requested: PermissionMode,
): Promise<WorkbenchSnapshot> {
  if (!isTauriRuntime()) {
    return {
      ...emptyWorkbench,
      sessionPermission: requested,
    };
  }

  return invoke<WorkbenchSnapshot>("set_session_permission", {
    request: { requested },
  });
}

export async function createKnowledgeSpace(
  rootPath: string,
  defaultPermission: PermissionMode,
): Promise<WorkbenchSnapshot> {
  const name = deriveSpaceName(rootPath);

  if (!isTauriRuntime()) {
    return browserSnapshotForFolder(rootPath, defaultPermission);
  }

  return invoke<WorkbenchSnapshot>("create_knowledge_space", {
    request: {
      name,
      rootPath,
      defaultPermission,
    },
  });
}

export async function scanKnowledgeSpace(
  spaceId: string,
): Promise<WorkbenchSnapshot> {
  if (!isTauriRuntime()) {
    return emptyWorkbench;
  }

  return invoke<WorkbenchSnapshot>("scan_knowledge_space", {
    request: { spaceId },
  });
}

export async function indexKnowledgeSpace(
  spaceId: string,
): Promise<WorkbenchSnapshot> {
  if (!isTauriRuntime()) {
    return emptyWorkbench;
  }

  return invoke<WorkbenchSnapshot>("index_knowledge_space", {
    request: { spaceId },
  });
}

export async function enqueueOcrParseJob(
  spaceId: string,
  fileId: string,
): Promise<WorkbenchSnapshot> {
  if (!isTauriRuntime()) {
    return emptyWorkbench;
  }

  return invoke<WorkbenchSnapshot>("enqueue_ocr_parse_job", {
    request: { spaceId, fileId },
  });
}

export async function cancelParseJob(
  jobId: string,
): Promise<WorkbenchSnapshot> {
  if (!isTauriRuntime()) {
    return emptyWorkbench;
  }

  return invoke<WorkbenchSnapshot>("cancel_parse_job", {
    request: { jobId },
  });
}

export async function startOcrWorker(
  spaceId: string,
): Promise<WorkbenchSnapshot> {
  if (!isTauriRuntime()) {
    return emptyWorkbench;
  }

  return invoke<WorkbenchSnapshot>("start_ocr_worker", {
    request: { spaceId },
  });
}

export async function askAgent(
  spaceId: string,
  question: string,
): Promise<WorkbenchSnapshot> {
  if (!isTauriRuntime()) {
    return {
      ...emptyWorkbench,
      activeSpaceId: spaceId,
      messages: [
        {
          id: "browser-question",
          role: "user",
          content: question,
        },
        {
          id: "browser-answer",
          role: "assistant",
          content: "浏览器预览无法读取本地索引，请在桌面应用中运行。",
        },
      ],
    };
  }

  return invoke<WorkbenchSnapshot>("ask_agent", {
    request: { spaceId, question },
  });
}

export async function setDefaultPermission(
  spaceId: string,
  permission: PermissionMode,
): Promise<WorkbenchSnapshot> {
  if (!isTauriRuntime()) {
    return {
      ...emptyWorkbench,
      sessionPermission: permission,
    };
  }

  return invoke<WorkbenchSnapshot>("set_default_permission", {
    request: { spaceId, permission },
  });
}
