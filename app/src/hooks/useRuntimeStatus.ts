import { useEffect, useState } from "react";

import { getRuntimeStatus } from "../lib/tauriClient";
import type { RuntimeStatus } from "../types/workbench";

interface RuntimeStatusState {
  runtimeStatus: RuntimeStatus | null;
  runtimeStatusError: string | null;
}

export function useRuntimeStatus(): RuntimeStatusState {
  const [state, setState] = useState<RuntimeStatusState>({
    runtimeStatus: null,
    runtimeStatusError: null,
  });

  useEffect(() => {
    let active = true;

    getRuntimeStatus()
      .then((runtimeStatus) => {
        if (active) {
          setState({ runtimeStatus, runtimeStatusError: null });
        }
      })
      .catch(() => {
        if (active) {
          setState({
            runtimeStatus: null,
            runtimeStatusError: "运行配置读取失败",
          });
        }
      });

    return () => {
      active = false;
    };
  }, []);

  return state;
}
