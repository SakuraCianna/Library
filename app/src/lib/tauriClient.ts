import { invoke, isTauri } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

import { emptyWorkbench } from "../data/emptyWorkbench";
import type { PermissionMode, WorkbenchSnapshot } from "../types/workbench";

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
