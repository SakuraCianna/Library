import { useCallback, useEffect, useState } from "react";

import { emptyWorkbench } from "../data/emptyWorkbench";
import {
  askAgent,
  createKnowledgeSpace,
  getWorkbenchSnapshot,
  indexKnowledgeSpace,
  requestSessionPermission,
  scanKnowledgeSpace,
  selectKnowledgeFolder,
  setDefaultPermission,
} from "../lib/tauriClient";
import type { PermissionMode, WorkbenchSnapshot } from "../types/workbench";

interface WorkbenchSnapshotState {
  snapshot: WorkbenchSnapshot;
  loading: boolean;
  error: string | null;
}

interface WorkbenchSnapshotResult extends WorkbenchSnapshotState {
  askAgentQuestion: (question: string) => Promise<void>;
  createSpaceFromFolder: (permission: PermissionMode) => Promise<void>;
  indexActiveSpace: () => Promise<void>;
  scanActiveSpace: () => Promise<void>;
  setFolderDefaultPermission: (permission: PermissionMode) => Promise<void>;
  setSessionPermission: (permission: PermissionMode) => Promise<void>;
}

const fallbackError = "状态读取失败，请检查本地数据库或稍后重试";

function getErrorMessage(error: unknown, fallback: string) {
  if (typeof error === "object" && error !== null && "message" in error) {
    const message = String((error as { message?: unknown }).message ?? "").trim();
    return message || fallback;
  }

  return fallback;
}

export function useWorkbenchSnapshot(): WorkbenchSnapshotResult {
  const [state, setState] = useState<WorkbenchSnapshotState>({
    snapshot: emptyWorkbench,
    loading: true,
    error: null,
  });

  const commitSnapshot = useCallback((snapshot: WorkbenchSnapshot) => {
    setState({
      snapshot,
      loading: false,
      error: null,
    });
  }, []);

  const setSessionPermission = useCallback(async (permission: PermissionMode) => {
    try {
      const snapshot = await requestSessionPermission(permission);
      setState((current) => ({
        snapshot: {
          ...current.snapshot,
          ...snapshot,
          spaces:
            snapshot.spaces.length > 0 ? snapshot.spaces : current.snapshot.spaces,
          activeSpaceId: snapshot.activeSpaceId || current.snapshot.activeSpaceId,
          files: snapshot.files.length > 0 ? snapshot.files : current.snapshot.files,
          sessionPermission: permission,
        },
        loading: false,
        error: null,
      }));
    } catch (error) {
      setState((current) => ({
        ...current,
        loading: false,
        error: getErrorMessage(error, "权限切换失败，已保留当前权限"),
      }));
    }
  }, [commitSnapshot]);

  const createSpaceFromFolder = useCallback(
    async (permission: PermissionMode) => {
      const rootPath = await selectKnowledgeFolder();
      if (!rootPath) {
        return;
      }

      setState((current) => ({ ...current, loading: true, error: null }));
      try {
        const snapshot = await createKnowledgeSpace(rootPath, permission);
        commitSnapshot(snapshot);
      } catch (error) {
        setState((current) => ({
          ...current,
          loading: false,
          error: getErrorMessage(error, "创建知识库失败"),
        }));
      }
    },
    [commitSnapshot],
  );

  const scanActiveSpace = useCallback(async () => {
    const spaceId = state.snapshot.activeSpaceId;
    if (!spaceId) {
      setState((current) => ({
        ...current,
        error: "请先添加一个知识库文件夹",
      }));
      return;
    }

    setState((current) => ({ ...current, loading: true, error: null }));
    try {
      const snapshot = await scanKnowledgeSpace(spaceId);
      commitSnapshot(snapshot);
    } catch (error) {
      setState((current) => ({
        ...current,
        loading: false,
        error: getErrorMessage(error, "扫描文件夹失败"),
      }));
    }
  }, [commitSnapshot, state.snapshot.activeSpaceId]);

  const indexActiveSpace = useCallback(async () => {
    const spaceId = state.snapshot.activeSpaceId;
    if (!spaceId) {
      setState((current) => ({
        ...current,
        error: "请先添加一个知识库文件夹",
      }));
      return;
    }

    setState((current) => ({ ...current, loading: true, error: null }));
    try {
      const snapshot = await indexKnowledgeSpace(spaceId);
      commitSnapshot(snapshot);
    } catch (error) {
      setState((current) => ({
        ...current,
        loading: false,
        error: getErrorMessage(error, "索引/摘要失败"),
      }));
    }
  }, [commitSnapshot, state.snapshot.activeSpaceId]);

  const askAgentQuestion = useCallback(
    async (question: string) => {
      const spaceId = state.snapshot.activeSpaceId;
      if (!spaceId) {
        setState((current) => ({
          ...current,
          error: "请先添加一个知识库文件夹",
        }));
        return;
      }

      const trimmedQuestion = question.trim();
      if (!trimmedQuestion) {
        return;
      }

      setState((current) => ({ ...current, loading: true, error: null }));
      try {
        const snapshot = await askAgent(spaceId, trimmedQuestion);
        commitSnapshot(snapshot);
      } catch (error) {
        setState((current) => ({
          ...current,
          loading: false,
          error: getErrorMessage(error, "助手回答失败"),
        }));
      }
    },
    [commitSnapshot, state.snapshot.activeSpaceId],
  );

  const setFolderDefaultPermission = useCallback(
    async (permission: PermissionMode) => {
      const spaceId = state.snapshot.activeSpaceId;
      if (!spaceId) {
        setState((current) => ({
          ...current,
          error: "请先添加一个知识库文件夹",
        }));
        return;
      }

      try {
        const snapshot = await setDefaultPermission(spaceId, permission);
        setState((current) => {
          const nextSpaces =
            snapshot.spaces.length > 0
              ? snapshot.spaces
              : current.snapshot.spaces.map((space) =>
                  space.id === spaceId
                    ? { ...space, defaultPermission: permission }
                    : space,
                );

          return {
            snapshot: {
              ...current.snapshot,
              ...snapshot,
              spaces: nextSpaces,
              activeSpaceId: snapshot.activeSpaceId || current.snapshot.activeSpaceId,
              files: snapshot.files.length > 0 ? snapshot.files : current.snapshot.files,
            },
            loading: false,
            error: null,
          };
        });
      } catch (error) {
        setState((current) => ({
          ...current,
          loading: false,
          error: getErrorMessage(error, "默认权限更新失败"),
        }));
      }
    },
    [commitSnapshot, state.snapshot.activeSpaceId],
  );

  useEffect(() => {
    let active = true;

    getWorkbenchSnapshot()
      .then((snapshot) => {
        if (active) {
          commitSnapshot(snapshot);
        }
      })
      .catch(() => {
        if (active) {
          setState({
            snapshot: emptyWorkbench,
            loading: false,
            error: fallbackError,
          });
        }
      });

    return () => {
      active = false;
    };
  }, [commitSnapshot]);

  return {
    ...state,
    askAgentQuestion,
    createSpaceFromFolder,
    indexActiveSpace,
    scanActiveSpace,
    setFolderDefaultPermission,
    setSessionPermission,
  };
}
