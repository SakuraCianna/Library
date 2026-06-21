import React from "react";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it, vi } from "vitest";

import { mockWorkbench } from "../data/mockWorkbench";

type UseWorkbenchSnapshot = typeof import("./useWorkbenchSnapshot").useWorkbenchSnapshot;

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
      { "data-testid": "file-count" },
      String(snapshot.files.length),
    ),
  );
}

describe("useWorkbenchSnapshot", () => {
  afterEach(() => {
    cleanup();
    clearMocks();
    Reflect.deleteProperty(globalThis, "isTauri");
    vi.doUnmock("../lib/tauriClient");
    vi.resetModules();
  });

  it("在浏览器环境返回本地示例并结束 loading", async () => {
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");

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
    expect(screen.getByTestId("active-space")).toHaveTextContent(
      mockWorkbench.activeSpaceId,
    );
    expect(screen.getByTestId("file-count")).toHaveTextContent(
      String(mockWorkbench.files.length),
    );
  });

  it("读取失败时回退到本地示例并返回中文错误", async () => {
    vi.doMock("../lib/tauriClient", () => ({
      getWorkbenchSnapshot: vi.fn(async () => {
        throw new Error("C:\\Users\\Sakura_Cianna\\secret.db");
      }),
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
      "状态读取失败，正在显示本地示例",
    );
    expect(screen.getByTestId("error")).not.toHaveTextContent("secret.db");
    expect(screen.getByTestId("active-space")).toHaveTextContent(
      mockWorkbench.activeSpaceId,
    );
    expect(screen.getByTestId("file-count")).toHaveTextContent(
      String(mockWorkbench.files.length),
    );
  });

  it("在 Tauri 环境合并命令返回的工作台状态", async () => {
    Object.defineProperty(globalThis, "isTauri", {
      configurable: true,
      value: true,
    });
    mockIPC((cmd) => {
      expect(cmd).toBe("get_workbench_snapshot");

      return {
        spaces: mockWorkbench.spaces,
        activeSpaceId: "space-springboot",
        activeScope: "all",
        sessionPermission: "full",
      };
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
    expect(screen.getByTestId("active-space")).toHaveTextContent(
      "space-springboot",
    );
    expect(screen.getByTestId("file-count")).toHaveTextContent(
      String(mockWorkbench.files.length),
    );
  });

  it("组件卸载后不会继续更新状态", async () => {
    let resolveSnapshot: (snapshot: typeof mockWorkbench) => void = () => {};
    vi.doMock("../lib/tauriClient", () => ({
      getWorkbenchSnapshot: vi.fn(
        () =>
          new Promise<typeof mockWorkbench>((resolve) => {
            resolveSnapshot = resolve;
          }),
      ),
    }));
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});

    const { useWorkbenchSnapshot } = await import("./useWorkbenchSnapshot");
    const { unmount } = render(
      React.createElement(SnapshotProbe, {
        useSnapshot: useWorkbenchSnapshot,
      }),
    );

    unmount();
    resolveSnapshot(mockWorkbench);
    await Promise.resolve();

    expect(consoleError).not.toHaveBeenCalled();
    consoleError.mockRestore();
  });
});
