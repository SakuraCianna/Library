import { useCallback, useEffect, useRef, useState } from "react";

import { emptyWorkbench } from "../data/emptyWorkbench";
import {
  askAgent,
  cancelParseJob,
  createKnowledgeSpace,
  enqueueOcrParseJob,
  exportSpaceBackup,
  getWorkbenchSnapshot,
  indexKnowledgeSpace,
  listenWorkbenchUpdates,
  preflightSpaceBackupRestore,
  requestSessionPermission,
  restoreSpaceBackup,
  scanKnowledgeSpace,
  selectBackupFile,
  selectKnowledgeFolder,
  setDefaultPermission,
  startOcrWorker,
} from "../lib/tauriClient";
import type {
  BackupExportResult,
  BackupRestorePreflight,
  BackupRestoreResult,
  PermissionMode,
  WorkbenchSnapshot,
} from "../types/workbench";

interface WorkbenchSnapshotState {
  snapshot: WorkbenchSnapshot;
  backupExport: BackupExportResult | null;
  backupRestorePreflight: BackupRestorePreflight | null;
  backupRestoreResult: BackupRestoreResult | null;
  loading: boolean;
  error: string | null;
}

interface WorkbenchSnapshotResult extends WorkbenchSnapshotState {
  askAgentQuestion: (question: string) => Promise<void>;
  cancelJob: (jobId: string) => Promise<void>;
  createSpaceFromFolder: (permission: PermissionMode) => Promise<void>;
  enqueueOcrJob: (fileId: string) => Promise<void>;
  exportActiveSpaceBackup: () => Promise<void>;
  confirmBackupRestore: () => Promise<void>;
  indexActiveSpace: () => Promise<void>;
  refreshSnapshot: (options?: { silent?: boolean }) => Promise<void>;
  scanActiveSpace: () => Promise<void>;
  selectBackupForRestore: () => Promise<void>;
  setFolderDefaultPermission: (permission: PermissionMode) => Promise<void>;
  setSessionPermission: (permission: PermissionMode) => Promise<void>;
  startOcrWorker: () => Promise<void>;
}

const fallbackError = "状态读取失败，请检查本地数据库或稍后重试";

function getErrorMessage(error: unknown, fallback: string) {
  if (typeof error === "object" && error !== null && "message" in error) {
    const message = String(
      (error as { message?: unknown }).message ?? "",
    ).trim();
    return message || fallback;
  }

  return fallback;
}

export function useWorkbenchSnapshot(): WorkbenchSnapshotResult {
  const mountedRef = useRef(true);
  const [state, setState] = useState<WorkbenchSnapshotState>({
    snapshot: emptyWorkbench,
    backupExport: null,
    backupRestorePreflight: null,
    backupRestoreResult: null,
    loading: true,
    error: null,
  });

  const commitSnapshot = useCallback((snapshot: WorkbenchSnapshot) => {
    if (!mountedRef.current) {
      return;
    }

    setState((current) => ({
      ...current,
      snapshot,
      loading: false,
      error: null,
    }));
  }, []);

  const refreshSnapshot = useCallback(
    async (options?: { silent?: boolean }) => {
      if (!options?.silent) {
        setState((current) => ({ ...current, loading: true, error: null }));
      }
      try {
        const snapshot = await getWorkbenchSnapshot();
        commitSnapshot(snapshot);
      } catch (error) {
        if (!mountedRef.current) {
          return;
        }

        setState((current) => ({
          ...current,
          loading: false,
          error: getErrorMessage(error, fallbackError),
        }));
      }
    },
    [commitSnapshot],
  );

  useEffect(() => {
    mountedRef.current = true;

    return () => {
      mountedRef.current = false;
    };
  }, []);

  const setSessionPermission = useCallback(
    async (permission: PermissionMode) => {
      try {
        const snapshot = await requestSessionPermission(permission);
        setState((current) => ({
          snapshot: {
            ...current.snapshot,
            ...snapshot,
            spaces:
              snapshot.spaces.length > 0
                ? snapshot.spaces
                : current.snapshot.spaces,
            activeSpaceId:
              snapshot.activeSpaceId || current.snapshot.activeSpaceId,
            files:
              snapshot.files.length > 0
                ? snapshot.files
                : current.snapshot.files,
            sessionPermission: permission,
          },
          backupExport: current.backupExport,
          backupRestorePreflight: current.backupRestorePreflight,
          backupRestoreResult: current.backupRestoreResult,
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
    },
    [commitSnapshot],
  );

  const createSpaceFromFolder = useCallback(
    async (permission: PermissionMode) => {
      setState((current) => ({ ...current, error: null }));

      let rootPath: string | null;
      try {
        rootPath = await selectKnowledgeFolder();
      } catch (error) {
        const message = getErrorMessage(error, "选择知识库文件夹失败");
        setState((current) => ({
          ...current,
          loading: false,
          error:
            message === "选择知识库文件夹失败"
              ? message
              : `选择知识库文件夹失败：${message}`,
        }));
        return;
      }

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

  const enqueueOcrJob = useCallback(
    async (fileId: string) => {
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
        const snapshot = await enqueueOcrParseJob(spaceId, fileId);
        commitSnapshot(snapshot);
      } catch (error) {
        setState((current) => ({
          ...current,
          loading: false,
          error: getErrorMessage(error, "OCR 排队失败"),
        }));
      }
    },
    [commitSnapshot, state.snapshot.activeSpaceId],
  );

  const cancelJob = useCallback(
    async (jobId: string) => {
      setState((current) => ({ ...current, loading: true, error: null }));
      try {
        const snapshot = await cancelParseJob(jobId);
        commitSnapshot(snapshot);
      } catch (error) {
        setState((current) => ({
          ...current,
          loading: false,
          error: getErrorMessage(error, "取消解析任务失败"),
        }));
      }
    },
    [commitSnapshot],
  );

  const startOcrWorkerForActiveSpace = useCallback(async () => {
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
      const snapshot = await startOcrWorker(spaceId);
      commitSnapshot(snapshot);
    } catch (error) {
      setState((current) => ({
        ...current,
        loading: false,
        error: getErrorMessage(error, "OCR 后台任务启动失败"),
      }));
    }
  }, [commitSnapshot, state.snapshot.activeSpaceId]);

  const exportActiveSpaceBackup = useCallback(async () => {
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
      const backupExport = await exportSpaceBackup(spaceId);
      setState((current) => ({
        ...current,
        backupExport,
        backupRestorePreflight: null,
        backupRestoreResult: null,
        loading: false,
        error: null,
      }));
    } catch (error) {
      setState((current) => ({
        ...current,
        loading: false,
        error: getErrorMessage(error, "导出备份失败"),
      }));
    }
  }, [state.snapshot.activeSpaceId]);

  const selectBackupForRestore = useCallback(async () => {
    setState((current) => ({
      ...current,
      backupRestorePreflight: null,
      backupRestoreResult: null,
      error: null,
    }));

    let backupPath: string | null;
    try {
      backupPath = await selectBackupFile();
    } catch (error) {
      const message = getErrorMessage(error, "选择备份文件失败");
      setState((current) => ({
        ...current,
        loading: false,
        error:
          message === "选择备份文件失败"
            ? message
            : `选择备份文件失败：${message}`,
      }));
      return;
    }

    if (!backupPath) {
      return;
    }

    setState((current) => ({ ...current, loading: true, error: null }));
    try {
      const backupRestorePreflight =
        await preflightSpaceBackupRestore(backupPath);
      setState((current) => ({
        ...current,
        backupExport: null,
        backupRestorePreflight,
        backupRestoreResult: null,
        loading: false,
        error: null,
      }));
    } catch (error) {
      setState((current) => ({
        ...current,
        loading: false,
        error: getErrorMessage(error, "恢复备份预检失败"),
      }));
    }
  }, []);

  const confirmBackupRestore = useCallback(async () => {
    const preflight = state.backupRestorePreflight;
    if (!preflight) {
      setState((current) => ({
        ...current,
        error: "请先选择并预检备份文件",
      }));
      return;
    }

    setState((current) => ({ ...current, loading: true, error: null }));
    try {
      const backupRestoreResult = await restoreSpaceBackup(preflight.path, true);
      const snapshot = await getWorkbenchSnapshot();
      setState((current) => ({
        ...current,
        snapshot,
        backupExport: null,
        backupRestorePreflight: null,
        backupRestoreResult,
        loading: false,
        error: null,
      }));
    } catch (error) {
      setState((current) => ({
        ...current,
        loading: false,
        error: getErrorMessage(error, "恢复备份失败"),
      }));
    }
  }, [state.backupRestorePreflight]);

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
              activeSpaceId:
                snapshot.activeSpaceId || current.snapshot.activeSpaceId,
              files:
                snapshot.files.length > 0
                  ? snapshot.files
                  : current.snapshot.files,
            },
            backupExport: current.backupExport,
            backupRestorePreflight: current.backupRestorePreflight,
            backupRestoreResult: current.backupRestoreResult,
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
            backupExport: null,
            backupRestorePreflight: null,
            backupRestoreResult: null,
            loading: false,
            error: fallbackError,
          });
        }
      });

    return () => {
      active = false;
    };
  }, [commitSnapshot]);

  useEffect(() => {
    let active = true;
    let stopListening: (() => void) | null = null;
    let refreshTimer: number | null = null;
    let refreshing = false;
    let pendingRefresh = false;

    const clearRefreshTimer = () => {
      if (refreshTimer !== null) {
        window.clearTimeout(refreshTimer);
        refreshTimer = null;
      }
    };

    const scheduleRefresh = () => {
      if (!active) {
        return;
      }

      if (refreshTimer !== null) {
        pendingRefresh = true;
        return;
      }

      refreshTimer = window.setTimeout(() => {
        refreshTimer = null;
        if (!active) {
          return;
        }

        if (refreshing) {
          pendingRefresh = true;
          return;
        }

        refreshing = true;
        void refreshSnapshot({ silent: true }).finally(() => {
          refreshing = false;
          if (pendingRefresh && active) {
            pendingRefresh = false;
            scheduleRefresh();
          }
        });
      }, 250);
    };

    listenWorkbenchUpdates(() => {
      scheduleRefresh();
    })
      .then((unlisten) => {
        if (!active) {
          unlisten?.();
          return;
        }

        stopListening = unlisten;
      })
      .catch(() => {
        stopListening = null;
      });

    return () => {
      active = false;
      clearRefreshTimer();
      stopListening?.();
    };
  }, [refreshSnapshot]);

  return {
    ...state,
    askAgentQuestion,
    cancelJob,
    confirmBackupRestore,
    createSpaceFromFolder,
    enqueueOcrJob,
    exportActiveSpaceBackup,
    indexActiveSpace,
    refreshSnapshot,
    scanActiveSpace,
    selectBackupForRestore,
    setFolderDefaultPermission,
    setSessionPermission,
    startOcrWorker: startOcrWorkerForActiveSpace,
  };
}
