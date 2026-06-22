import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import App from "../App";
import { emptyWorkbench } from "../data/emptyWorkbench";
import type { WorkbenchSnapshot } from "../types/workbench";

const snapshotWithSpace: WorkbenchSnapshot = {
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

const runtimeStatus = {
  deepseek: {
    configured: false,
    model: "deepseek-v4-flash",
    baseUrl: "https://api.deepseek.com",
    keyHint: "未配置",
  },
  ocr: {
    configured: false,
    tier: "medium",
    modelDir: "E:\\CodeHome\\Library\\models\\ocr\\pp-ocrv6",
    missingModels: ["PP-OCRv6_medium_det", "PP-OCRv6_medium_rec"],
  },
};

const answeredSnapshot: WorkbenchSnapshot = {
  ...snapshotWithSpace,
  messages: [
    {
      id: "msg-user-redis",
      role: "user",
      content: "缓存穿透怎么处理？",
    },
    {
      id: "msg-assistant-redis",
      role: "assistant",
      content: "缓存穿透可以用空值缓存、布隆过滤器和参数校验处理。",
    },
  ],
};

describe("App", () => {
  afterEach(() => {
    cleanup();
    clearMocks();
    Reflect.deleteProperty(globalThis, "isTauri");
  });

  it("renders the Chinese workbench without sample knowledge spaces", async () => {
    render(<App />);

    expect(screen.getByRole("heading", { name: "知识库" })).toBeInTheDocument();
    expect(await screen.findByText("暂无知识库文件夹")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "未选择文件夹" })).toBeInTheDocument();
    expect(screen.getByText("请先添加一个真实文件夹")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /面试/ })).not.toBeInTheDocument();
    expect(screen.queryByText("待批准操作")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "打开默认权限设置" }));
    expect(await screen.findByText("DeepSeek")).toBeInTheDocument();
    expect(screen.getByText("deepseek-v4-flash")).toBeInTheDocument();
    expect(screen.getByText("本地 OCR")).toBeInTheDocument();
  });

  it("shows folder default permission controls and rounded selects", async () => {
    render(<App />);

    expect(await screen.findByLabelText("切换文件夹默认权限")).toBeDisabled();
    expect(screen.getByRole("button", { name: "打开默认权限设置" })).toBeEnabled();
    expect(screen.getByRole("combobox", { name: "切换会话权限" })).toBeDisabled();
  });

  it("opens the default permission explanation from the gear button", async () => {
    Object.defineProperty(globalThis, "isTauri", {
      configurable: true,
      value: true,
    });
    mockIPC((cmd) => {
      if (cmd === "get_runtime_status") {
        return runtimeStatus;
      }
      if (cmd === "get_workbench_snapshot") {
        return snapshotWithSpace;
      }
      return snapshotWithSpace;
    });
    render(<App />);

    await screen.findByRole("heading", { name: "真实知识库" });
    const gearButton = screen.getByRole("button", { name: "打开默认权限设置" });
    expect(gearButton).toBeEnabled();

    fireEvent.click(gearButton);

    expect(screen.getAllByText("默认权限").length).toBeGreaterThan(0);
    expect(screen.getByText(/文件夹长期保存的 Agent 操作边界/)).toBeInTheDocument();
  });

  it("keeps send inside composer and exposes icon actions", async () => {
    render(<App />);

    await screen.findByText("暂无知识库文件夹");
    expect(screen.getByRole("button", { name: "新建" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "扫描" })).toBeDisabled();

    const composer = screen.getByRole("form", { name: "智能助手输入区" });
    expect(
      within(composer).getByRole("button", { name: "发送" }),
    ).toBeInTheDocument();
  });

  it("sends a sidebar question through the agent command and renders the answer", async () => {
    Object.defineProperty(globalThis, "isTauri", {
      configurable: true,
      value: true,
    });
    mockIPC((cmd) => {
      if (cmd === "get_runtime_status") {
        return runtimeStatus;
      }
      if (cmd === "ask_agent") {
        return answeredSnapshot;
      }
      return snapshotWithSpace;
    });
    render(<App />);

    await screen.findByRole("heading", { name: "真实知识库" });
    const composer = screen.getByRole("form", { name: "智能助手输入区" });
    fireEvent.change(within(composer).getByLabelText("向智能助手提问"), {
      target: { value: "缓存穿透怎么处理？" },
    });
    fireEvent.click(within(composer).getByRole("button", { name: "发送" }));

    expect(await screen.findByText(/空值缓存、布隆过滤器/)).toBeInTheDocument();
  });
});
