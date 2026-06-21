import { invoke, isTauri } from "@tauri-apps/api/core";

import { mockWorkbench } from "../data/mockWorkbench";
import type { WorkbenchSnapshot } from "../types/workbench";

type TauriWorkbenchSnapshot = Pick<
  WorkbenchSnapshot,
  "spaces" | "activeSpaceId" | "activeScope" | "sessionPermission"
>;

function isTauriRuntime() {
  return isTauri();
}

function mergeWorkbenchSnapshot(
  tauriSnapshot: TauriWorkbenchSnapshot,
): WorkbenchSnapshot {
  return {
    ...mockWorkbench,
    spaces:
      tauriSnapshot.spaces.length > 0
        ? tauriSnapshot.spaces
        : mockWorkbench.spaces,
    activeSpaceId: tauriSnapshot.activeSpaceId,
    activeScope: tauriSnapshot.activeScope,
    sessionPermission: tauriSnapshot.sessionPermission,
  };
}

export async function getWorkbenchSnapshot(): Promise<WorkbenchSnapshot> {
  if (!isTauriRuntime()) {
    return mockWorkbench;
  }

  const tauriSnapshot = await invoke<TauriWorkbenchSnapshot>(
    "get_workbench_snapshot",
  );

  return mergeWorkbenchSnapshot(tauriSnapshot);
}
