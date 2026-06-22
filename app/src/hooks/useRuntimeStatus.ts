import { useCallback, useEffect, useRef, useState } from "react";

import {
  checkOcrEnvironment as invokeCheckOcrEnvironment,
  getRuntimeStatus,
} from "../lib/tauriClient";
import type { OcrEnvironmentReport, RuntimeStatus } from "../types/workbench";

interface RuntimeStatusData {
  runtimeStatus: RuntimeStatus | null;
  runtimeStatusError: string | null;
  ocrEnvironmentReport: OcrEnvironmentReport | null;
  ocrEnvironmentError: string | null;
  checkingOcrEnvironment: boolean;
}

interface RuntimeStatusState extends RuntimeStatusData {
  refreshRuntimeStatus: () => Promise<void>;
  checkOcrEnvironment: () => Promise<void>;
}

export function useRuntimeStatus(): RuntimeStatusState {
  const mountedRef = useRef(true);
  const [state, setState] = useState<RuntimeStatusData>({
    runtimeStatus: null,
    runtimeStatusError: null,
    ocrEnvironmentReport: null,
    ocrEnvironmentError: null,
    checkingOcrEnvironment: false,
  });

  const refreshRuntimeStatus = useCallback(async () => {
    try {
      const runtimeStatus = await getRuntimeStatus();

      if (mountedRef.current) {
        setState((currentState) => ({
          ...currentState,
          runtimeStatus,
          runtimeStatusError: null,
        }));
      }
    } catch {
      if (mountedRef.current) {
        setState((currentState) => ({
          ...currentState,
          runtimeStatus: null,
          runtimeStatusError: "运行配置读取失败",
        }));
      }
    }
  }, []);

  const checkOcrEnvironment = useCallback(async () => {
    setState((currentState) => ({
      ...currentState,
      checkingOcrEnvironment: true,
      ocrEnvironmentError: null,
    }));

    try {
      const ocrEnvironmentReport = await invokeCheckOcrEnvironment();
      const runtimeStatus = await getRuntimeStatus();

      if (mountedRef.current) {
        setState((currentState) => ({
          ...currentState,
          runtimeStatus,
          runtimeStatusError: null,
          ocrEnvironmentReport,
          ocrEnvironmentError: null,
          checkingOcrEnvironment: false,
        }));
      }
    } catch {
      if (mountedRef.current) {
        setState((currentState) => ({
          ...currentState,
          checkingOcrEnvironment: false,
          ocrEnvironmentError: "OCR 环境自检失败",
        }));
      }
    }
  }, []);

  useEffect(() => {
    void refreshRuntimeStatus();
  }, [refreshRuntimeStatus]);

  useEffect(() => {
    return () => {
      mountedRef.current = false;
    };
  }, []);

  return {
    ...state,
    refreshRuntimeStatus,
    checkOcrEnvironment,
  };
}
