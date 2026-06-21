import { useEffect, useState } from "react";

import { mockWorkbench } from "../data/mockWorkbench";
import { getWorkbenchSnapshot } from "../lib/tauriClient";
import type { WorkbenchSnapshot } from "../types/workbench";

interface WorkbenchSnapshotState {
  snapshot: WorkbenchSnapshot;
  loading: boolean;
  error: string | null;
}

const fallbackError = "状态读取失败，正在显示本地示例";

function formatSnapshotError(error: unknown) {
  if (error instanceof Error && error.message) {
    return `${fallbackError}：${error.message}`;
  }

  if (typeof error === "string" && error) {
    return `${fallbackError}：${error}`;
  }

  return fallbackError;
}

export function useWorkbenchSnapshot(): WorkbenchSnapshotState {
  const [state, setState] = useState<WorkbenchSnapshotState>({
    snapshot: mockWorkbench,
    loading: true,
    error: null,
  });

  useEffect(() => {
    let active = true;

    getWorkbenchSnapshot()
      .then((snapshot) => {
        if (active) {
          setState({ snapshot, loading: false, error: null });
        }
      })
      .catch((error: unknown) => {
        if (active) {
          setState({
            snapshot: mockWorkbench,
            loading: false,
            error: formatSnapshotError(error),
          });
        }
      });

    return () => {
      active = false;
    };
  }, []);

  return state;
}
