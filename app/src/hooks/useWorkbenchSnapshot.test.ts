import React from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it, vi } from "vitest";

import { emptyWorkbench } from "../data/emptyWorkbench";
import type { WorkbenchSnapshot } from "../types/workbench";

type UseWorkbenchSnapshot = typeof import("./useWorkbenchSnapshot").useWorkbenchSnapshot;

const realSnapshot: WorkbenchSnapshot = {
  ...emptyWorkbench,
  activeSpaceId: "space-real",
  sessionPermission: "approval",
  spaces: [
    {
      id: "space-real",
      name: "真实知识库",
      path: "D:\\知识库\\真实",
      defaultPermission: "approval",
      changedFileCount: 0,
      ocrQueueCount: 0,
    },
  ],
};

function SnapshotProbe({
  useSnapshot,
}: {
  useSnapshot: UseWorkbenchSnapshot;
}) {
  const { snapshot, loading, error } = useSnapshot();

  return React.createElement(
    "section",
    null,
    React.createElement("span", { "data-testid": "loading" }, String(loading)),
    React.createElement("span", { "data-testid": "error" }, error ?? ""),
    React.createElement(
      "span",
      { "data-testid": "active-space" },
      snapshot.activeSpaceId,
    ),
    React.createElement(
      "span",
      { "data-testid": "space-count" },
      String(snapshot.spaces.length),
    ),
  );
}

function ActionProbe({
  useSnapshot,
}: {
  useSnapshot: UseWorkbenchSnapshot;
}) {
  const { snapshot, createSpaceFromFolder, setSessionPermission } = useSnapshot();

  return React.createElement(
    "section",
    null,
    React.createElement(
      "span",
      { "data-testid": "space-count" },
      String(snapshot.spaces.length),
    ),
    React.createElement(
      "span",
      { "data-testid": "permission" },
      snapshot.sessionPermission,
    ),
    React.createElement(
      "button",
      {
        onClick: () => {
          void createSpaceFromFolder("approval");
        },
        type: "button",
      },
      "创建",
    ),
    React.createElement(
      "button",
      {
        onClick: () => {
          void setSessionPermission("readonly");
        },
        type: "button",
      },
      "切只读",
    ),
  );
}

describe("useWorkbenchSnapshot", () => {
  afterEach(() => {
    cleanup();
    clearMocks();
    Reflect.deleteProperty(globalThis, "isTauri");
    vi.doUnmock("../lib/tauriClient");
    vi.doUnmock("react");
    vi.restoreAllMocks();
    vi.resetModules();
  });

  it("在浏览器环境返回空工作台并结束 loading", async () => {
    const { useWorkbenchSnapshot } = await import("./useWorkbenchSnapshot");

    render(
      React.createElement(SnapshotProbe, {
        useSnapshot: useWorkbenchSnapshot,
      }),
    );

    await waitFor(() => {
      expect(screen.getByTestId("loading")).toHaveTextContent("false");
    });

    expect(screen.getByTestId("error").textContent).toBe("");
    expect(screen.getByTestId("active-space").textContent).toBe("");
    expect(screen.getByTestId("space-count")).toHaveTextContent("0");
  });

  it("读取失败时返回中文错误且不泄漏原始路径", async () => {
    vi.doMock("../lib/tauriClient", () => ({
      createKnowledgeSpace: vi.fn(),
      getWorkbenchSnapshot: vi.fn(async () => {
        throw new Error("C:\\Users\\Sakura_Cianna\\secret.db");
      }),
      requestSessionPermission: vi.fn(),
      scanKnowledgeSpace: vi.fn(),
      selectKnowledgeFolder: vi.fn(),
      setDefaultPermission: vi.fn(),
    }));

    const { useWorkbenchSnapshot } = await import("./useWorkbenchSnapshot");

    render(
      React.createElement(SnapshotProbe, {
        useSnapshot: useWorkbenchSnapshot,
      }),
    );

    await waitFor(() => {
      expect(screen.getByTestId("loading")).toHaveTextContent("false");
    });

    expect(screen.getByTestId("error")).toHaveTextContent(
      "状态读取失败，请检查本地数据库或稍后重试",
    );
    expect(screen.getByTestId("error")).not.toHaveTextContent("secret.db");
  });

  it("在 Tauri 环境读取真实命令返回的工作台状态", async () => {
    Object.defineProperty(globalThis, "isTauri", {
      configurable: true,
      value: true,
    });
    mockIPC((cmd) => {
      expect(cmd).toBe("get_workbench_snapshot");
      return realSnapshot;
    });

    const { useWorkbenchSnapshot } = await import("./useWorkbenchSnapshot");

    render(
      React.createElement(SnapshotProbe, {
        useSnapshot: useWorkbenchSnapshot,
      }),
    );

    await waitFor(() => {
      expect(screen.getByTestId("loading")).toHaveTextContent("false");
    });

    expect(screen.getByTestId("error").textContent).toBe("");
    expect(screen.getByTestId("active-space")).toHaveTextContent("space-real");
    expect(screen.getByTestId("space-count")).toHaveTextContent("1");
  });

  it("浏览器预览中切换会话权限不会清空临时知识库", async () => {
    const promptSpy = vi
      .spyOn(window, "prompt")
      .mockReturnValue("D:\\知识库\\浏览器预览");
    const { useWorkbenchSnapshot } = await import("./useWorkbenchSnapshot");

    render(
      React.createElement(ActionProbe, {
        useSnapshot: useWorkbenchSnapshot,
      }),
    );

    fireEvent.click(screen.getByRole("button", { name: "创建" }));
    await waitFor(() => {
      expect(screen.getByTestId("space-count")).toHaveTextContent("1");
    });

    fireEvent.click(screen.getByRole("button", { name: "切只读" }));
    await waitFor(() => {
      expect(screen.getByTestId("permission")).toHaveTextContent("readonly");
    });
    expect(screen.getByTestId("space-count")).toHaveTextContent("1");

    promptSpy.mockRestore();
  });

  it("组件卸载后不会继续更新状态", async () => {
    let resolveSnapshot: (snapshot: WorkbenchSnapshot) => void = () => {};
    const setState = vi.fn();
    let cleanupEffect: (() => void) | undefined;

    vi.doMock("../lib/tauriClient", () => ({
      createKnowledgeSpace: vi.fn(),
      getWorkbenchSnapshot: vi.fn(
        () =>
          new Promise<WorkbenchSnapshot>((resolve) => {
            resolveSnapshot = resolve;
          }),
      ),
      requestSessionPermission: vi.fn(),
      scanKnowledgeSpace: vi.fn(),
      selectKnowledgeFolder: vi.fn(),
      setDefaultPermission: vi.fn(),
    }));
    vi.doMock("react", async () => {
      const actual = await vi.importActual<typeof import("react")>("react");

      return {
        ...actual,
        useCallback: vi.fn((callback) => callback),
        useEffect: vi.fn((effect: () => void | (() => void)) => {
          const cleanupEffectCandidate = effect();

          if (typeof cleanupEffectCandidate === "function") {
            cleanupEffect = cleanupEffectCandidate;
          }
        }),
        useState: vi.fn(() => [
          { snapshot: emptyWorkbench, loading: true, error: null },
          setState,
        ]),
      };
    });

    const { useWorkbenchSnapshot } = await import("./useWorkbenchSnapshot");
    useWorkbenchSnapshot();

    expect(cleanupEffect).toBeDefined();
    cleanupEffect?.();
    resolveSnapshot(realSnapshot);
    await Promise.resolve();

    expect(setState).not.toHaveBeenCalled();
  });
});
