import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import App from "../App";
import { emptyWorkbench } from "../data/emptyWorkbench";
import type {
  OcrEnvironmentReport,
  ParseJobSummary,
  WorkbenchSnapshot,
} from "../types/workbench";

const eventListenMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/event", () => ({
  listen: eventListenMock,
}));

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
      scanQueueCount: 0,
      documentQueueCount: 0,
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

const ocrEnvironmentReport: OcrEnvironmentReport = {
  ok: true,
  checks: [
    {
      name: "models",
      ok: true,
      message: "OCR model assets complete",
      details: {
        modelDir: "E:\\CodeHome\\Library\\models\\ocr\\pp-ocrv6",
      },
    },
    {
      name: "paddleocr",
      ok: true,
      message: "paddleocr installed",
    },
  ],
};

const answeredSnapshot: WorkbenchSnapshot = {
  ...snapshotWithSpace,
  messages: [
    {
      id: "msg-user-redis",
      role: "user",
      content: "缓存穿透怎么处理？",
      sources: [],
    },
    {
      id: "msg-assistant-redis",
      role: "assistant",
      content: "缓存穿透可以用空值缓存、布隆过滤器和参数校验处理。",
      sources: [
        {
          id: "block-redis",
          title: "Redis 缓存穿透",
          excerpt: "缓存穿透需要空值缓存和布隆过滤器。",
          sourceFileName: "Redis面试.md",
          sourceLocator: "Redis面试.md#block-001",
        },
      ],
    },
  ],
};

function parseJob(overrides: Partial<ParseJobSummary> = {}): ParseJobSummary {
  return {
    id: "job-ocr",
    fileId: "file-pdf",
    fileName: "scan.pdf",
    jobType: "ocr",
    status: "queued",
    errorMessage: null,
    startedAt: null,
    finishedAt: null,
    progressCurrent: 0,
    progressTotal: 1,
    phase: "等待执行",
    ...overrides,
  };
}

describe("App", () => {
  beforeEach(() => {
    eventListenMock.mockResolvedValue(() => undefined);
  });

  afterEach(() => {
    cleanup();
    clearMocks();
    eventListenMock.mockReset();
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

  it("runs the OCR environment check from the runtime panel", async () => {
    const commands: string[] = [];
    Object.defineProperty(globalThis, "isTauri", {
      configurable: true,
      value: true,
    });
    mockIPC((cmd) => {
      commands.push(cmd);
      if (cmd === "get_runtime_status") {
        return runtimeStatus;
      }
      if (cmd === "check_ocr_environment") {
        return ocrEnvironmentReport;
      }
      return snapshotWithSpace;
    });
    render(<App />);

    await screen.findByRole("heading", { name: "真实知识库" });
    fireEvent.click(screen.getByRole("button", { name: "打开默认权限设置" }));
    fireEvent.click(await screen.findByRole("button", { name: "自检" }));

    expect(await screen.findByText("通过")).toBeInTheDocument();
    expect(screen.getByText("models")).toBeInTheDocument();
    expect(screen.getByText("paddleocr")).toBeInTheDocument();
    expect(screen.getByText(/OCR model assets complete/)).toBeInTheDocument();
    expect(commands).toContain("check_ocr_environment");
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
    const invocations: Array<{ cmd: string; args?: unknown }> = [];
    Object.defineProperty(globalThis, "isTauri", {
      configurable: true,
      value: true,
    });
    mockIPC((cmd, args) => {
      invocations.push({ cmd, args });
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
    expect(screen.getByLabelText("回答来源")).toBeInTheDocument();
    expect(screen.getByText("Redis面试.md")).toBeInTheDocument();
    expect(screen.getByText("定位：Redis面试.md#block-001")).toBeInTheDocument();
    expect(screen.getByText("缓存穿透需要空值缓存和布隆过滤器。")).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", {
        name: "查看来源 Redis面试.md Redis 缓存穿透",
      }),
    );
    const sourcePanel = screen.getByRole("article", { name: "聊天来源详情" });
    expect(within(sourcePanel).getByText("聊天来源预览")).toBeInTheDocument();
    expect(within(sourcePanel).getByText("Redis 缓存穿透")).toBeInTheDocument();
    expect(
      within(sourcePanel).getByText("定位：Redis面试.md#block-001"),
    ).toBeInTheDocument();
    expect(
      within(sourcePanel).getByText("缓存穿透需要空值缓存和布隆过滤器。"),
    ).toBeInTheDocument();

    fireEvent.click(within(sourcePanel).getByRole("button", { name: "打开文件" }));
    await waitFor(() =>
      expect(invocations.some((invocation) => invocation.cmd === "open_source_file")).toBe(
        true,
      ),
    );
    expect(invocations).toContainEqual({
      cmd: "open_source_file",
      args: {
        request: {
          spaceId: "space-real",
          sourceLocator: "Redis面试.md#block-001",
        },
      },
    });

    fireEvent.click(within(sourcePanel).getByRole("button", { name: "查看最新" }));
    expect(screen.getByRole("article", { name: "知识块预览" })).toBeInTheDocument();
  });

  it("renders parse queue status when jobs exist", async () => {
    const snapshotWithJob = {
      ...snapshotWithSpace,
      parseJobs: [
        parseJob({ id: "job-1", fileId: "file-scan" }),
      ],
    };
    Object.defineProperty(globalThis, "isTauri", {
      configurable: true,
      value: true,
    });
    mockIPC((cmd) => {
      if (cmd === "get_runtime_status") {
        return runtimeStatus;
      }
      return snapshotWithJob;
    });
    render(<App />);

    expect(await screen.findByText("解析队列")).toBeInTheDocument();
    expect(screen.getByText("scan.pdf")).toBeInTheDocument();
    expect(screen.getByText("等待中")).toBeInTheDocument();
    expect(screen.getByText("0/1")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "取消解析任务 scan.pdf" }),
    ).toBeInTheDocument();
  });

  it("renders OCR page progress in the parse queue", async () => {
    const snapshotWithProgress = {
      ...snapshotWithSpace,
      parseJobs: [
        parseJob({
          status: "running",
          phase: "已识别第 1/2 页",
          progressCurrent: 1,
          progressTotal: 2,
        }),
      ],
    };
    Object.defineProperty(globalThis, "isTauri", {
      configurable: true,
      value: true,
    });
    mockIPC((cmd) => {
      if (cmd === "get_runtime_status") {
        return runtimeStatus;
      }
      return snapshotWithProgress;
    });
    render(<App />);

    expect(await screen.findByText("本地 OCR · 已识别第 1/2 页")).toBeInTheDocument();
    expect(screen.getByText("1/2")).toBeInTheDocument();
  });

  it("queues a PDF file for OCR through the desktop command", async () => {
    const snapshotWithPdf = {
      ...snapshotWithSpace,
      files: [
        {
          id: "file-pdf",
          name: "scan.pdf",
          extension: ".pdf",
          status: "queued" as const,
          statusLabel: "待解析",
        },
      ],
    };
    const queuedSnapshot = {
      ...snapshotWithPdf,
      parseJobs: [
        parseJob(),
      ],
    };
    Object.defineProperty(globalThis, "isTauri", {
      configurable: true,
      value: true,
    });
    mockIPC((cmd) => {
      if (cmd === "get_runtime_status") {
        return runtimeStatus;
      }
      if (cmd === "enqueue_ocr_parse_job") {
        return queuedSnapshot;
      }
      return snapshotWithPdf;
    });
    render(<App />);

    expect(await screen.findByText("scan.pdf")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "排队 OCR scan.pdf" }));

    expect(await screen.findByText("解析队列")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "排队 OCR scan.pdf" })).toBeDisabled();
  });

  it("queues an image file for OCR through the desktop command", async () => {
    const snapshotWithImage = {
      ...snapshotWithSpace,
      files: [
        {
          id: "file-image",
          name: "scan.png",
          extension: ".png",
          status: "queued" as const,
          statusLabel: "待解析",
        },
      ],
    };
    const queuedSnapshot = {
      ...snapshotWithImage,
      parseJobs: [
        parseJob({
          fileId: "file-image",
          fileName: "scan.png",
        }),
      ],
    };
    Object.defineProperty(globalThis, "isTauri", {
      configurable: true,
      value: true,
    });
    mockIPC((cmd) => {
      if (cmd === "get_runtime_status") {
        return runtimeStatus;
      }
      if (cmd === "enqueue_ocr_parse_job") {
        return queuedSnapshot;
      }
      return snapshotWithImage;
    });
    render(<App />);

    expect(await screen.findByText("scan.png")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "排队 OCR scan.png" }));

    expect(await screen.findByText("解析队列")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "排队 OCR scan.png" })).toBeDisabled();
  });

  it("starts folder scanning through the scan command", async () => {
    const commands: string[] = [];
    const scanningSnapshot = {
      ...snapshotWithSpace,
      spaces: [
        {
          ...snapshotWithSpace.spaces[0],
          scanQueueCount: 1,
        },
      ],
      parseJobs: [
        parseJob({
          id: "job-scan",
          fileId: null,
          fileName: "文件夹扫描",
          jobType: "scan",
          progressCurrent: 3,
          progressTotal: 0,
          phase: "正在扫描 README.md",
        }),
      ],
    };
    Object.defineProperty(globalThis, "isTauri", {
      configurable: true,
      value: true,
    });
    mockIPC((cmd) => {
      commands.push(cmd);
      if (cmd === "get_runtime_status") {
        return runtimeStatus;
      }
      if (cmd === "scan_knowledge_space") {
        return scanningSnapshot;
      }
      return snapshotWithSpace;
    });
    render(<App />);

    await screen.findByRole("heading", { name: "真实知识库" });
    fireEvent.click(screen.getByRole("button", { name: "扫描" }));

    expect(await screen.findByText("文件夹扫描")).toBeInTheDocument();
    expect(screen.getByText("文件夹扫描 · 正在扫描 README.md")).toBeInTheDocument();
    expect(screen.getByText("已处理 3")).toBeInTheDocument();
    expect(commands).toContain("scan_knowledge_space");

    await new Promise((resolve) => window.setTimeout(resolve, 1700));

    expect(
      commands.filter((command) => command === "get_workbench_snapshot").length,
    ).toBeGreaterThan(1);
  });

  it("starts the OCR worker through the desktop command", async () => {
    const snapshotWithJob = {
      ...snapshotWithSpace,
      parseJobs: [
        parseJob(),
      ],
    };
    const finishedSnapshot = {
      ...snapshotWithJob,
      parseJobs: [
        parseJob({
          status: "succeeded",
          phase: "已完成",
          progressCurrent: 1,
          progressTotal: 1,
          finishedAt: "2026-06-22T00:00:00Z",
        }),
      ],
      blockPreview: {
        id: "block-ocr",
        title: "scan.pdf",
        excerpt: "扫描版 PDF 的本地 OCR 文本",
        sourceFileName: "scan.pdf",
        sourceLocator: "scan.pdf#ocr",
      },
    };
    Object.defineProperty(globalThis, "isTauri", {
      configurable: true,
      value: true,
    });
    mockIPC((cmd) => {
      if (cmd === "get_runtime_status") {
        return runtimeStatus;
      }
      if (cmd === "start_ocr_worker") {
        return finishedSnapshot;
      }
      return snapshotWithJob;
    });
    render(<App />);

    expect(await screen.findByText("解析队列")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "启动 OCR" }));

    expect((await screen.findAllByText("已完成")).length).toBeGreaterThan(0);
    expect(screen.getByText("扫描版 PDF 的本地 OCR 文本")).toBeInTheDocument();
  });

  it("refreshes the workbench when a backend worker event is emitted", async () => {
    let emitWorkbenchUpdate: (() => void) | undefined;
    const commands: string[] = [];
    const snapshotWithRunningJob = {
      ...snapshotWithSpace,
      parseJobs: [
        parseJob({
          status: "running",
          phase: "正在执行本地 OCR",
        }),
      ],
    };
    const snapshotAfterEvent = {
      ...snapshotWithRunningJob,
      parseJobs: [
        parseJob({
          status: "succeeded",
          phase: "已完成",
          progressCurrent: 1,
          progressTotal: 1,
        }),
      ],
      blockPreview: {
        id: "block-event",
        title: "scan.pdf",
        excerpt: "事件刷新后的 OCR 文本",
        sourceFileName: "scan.pdf",
        sourceLocator: "scan.pdf#ocr",
      },
    };
    let currentSnapshot = snapshotWithRunningJob;

    eventListenMock.mockImplementation(
      async (
        _eventName: string,
        handler: (event: {
          payload: { spaceId: string | null; reason: string };
        }) => void,
      ) => {
        emitWorkbenchUpdate = () =>
          handler({
            payload: {
              spaceId: "space-real",
              reason: "ocr-progress",
            },
          });
        return () => undefined;
      },
    );
    Object.defineProperty(globalThis, "isTauri", {
      configurable: true,
      value: true,
    });
    mockIPC((cmd) => {
      commands.push(cmd);
      if (cmd === "get_runtime_status") {
        return runtimeStatus;
      }
      return currentSnapshot;
    });
    render(<App />);

    expect(await screen.findByText("本地 OCR · 正在执行本地 OCR")).toBeInTheDocument();
    await waitFor(() =>
      expect(eventListenMock).toHaveBeenCalledWith(
        "workbench-updated",
        expect.any(Function),
      ),
    );

    currentSnapshot = snapshotAfterEvent;
    emitWorkbenchUpdate?.();

    expect(await screen.findByText("事件刷新后的 OCR 文本")).toBeInTheDocument();
    expect(
      commands.filter((command) => command === "get_workbench_snapshot").length,
    ).toBeGreaterThan(1);
  });

  it("starts document parsing through the index command", async () => {
    const snapshotWithJob = {
      ...snapshotWithSpace,
      spaces: [
        {
          ...snapshotWithSpace.spaces[0],
          documentQueueCount: 1,
        },
      ],
      parseJobs: [
        parseJob({
          id: "job-doc",
          fileId: "file-md",
          fileName: "Redis面试.md",
          jobType: "document",
        }),
      ],
    };
    const finishedSnapshot = {
      ...snapshotWithJob,
      spaces: [
        {
          ...snapshotWithSpace.spaces[0],
          documentQueueCount: 0,
        },
      ],
      parseJobs: [
        parseJob({
          id: "job-doc",
          fileId: "file-md",
          fileName: "Redis面试.md",
          jobType: "document",
          status: "succeeded",
          phase: "已完成",
          progressCurrent: 2,
          progressTotal: 2,
        }),
      ],
      blockPreview: {
        id: "block-doc",
        title: "Redis面试.md",
        excerpt: "缓存穿透需要空值缓存和布隆过滤器。",
        sourceFileName: "Redis面试.md",
        sourceLocator: "Redis面试.md",
      },
    };
    Object.defineProperty(globalThis, "isTauri", {
      configurable: true,
      value: true,
    });
    mockIPC((cmd) => {
      if (cmd === "get_runtime_status") {
        return runtimeStatus;
      }
      if (cmd === "index_knowledge_space") {
        return finishedSnapshot;
      }
      return snapshotWithJob;
    });
    render(<App />);

    expect(await screen.findByText("解析队列")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "启动文档" }));

    expect(await screen.findByText("缓存穿透需要空值缓存和布隆过滤器。")).toBeInTheDocument();
    expect(screen.getByText("文档解析 · 已完成")).toBeInTheDocument();
  });

  it("keeps polling briefly after toolbar index starts before running appears", async () => {
    const commands: string[] = [];
    const snapshotWithJob = {
      ...snapshotWithSpace,
      spaces: [
        {
          ...snapshotWithSpace.spaces[0],
          documentQueueCount: 1,
        },
      ],
      parseJobs: [
        parseJob({
          id: "job-doc",
          fileId: "file-md",
          fileName: "Redis面试.md",
          jobType: "document",
        }),
      ],
    };
    Object.defineProperty(globalThis, "isTauri", {
      configurable: true,
      value: true,
    });
    mockIPC((cmd) => {
      commands.push(cmd);
      if (cmd === "get_runtime_status") {
        return runtimeStatus;
      }
      return snapshotWithJob;
    });
    render(<App />);

    expect(await screen.findByText("解析队列")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "建索引/摘要" }));
    expect(commands).toContain("index_knowledge_space");

    await new Promise((resolve) => window.setTimeout(resolve, 1700));

    expect(
      commands.filter((command) => command === "get_workbench_snapshot").length,
    ).toBeGreaterThan(1);
  });

  it("shows an error when running a queued OCR job fails", async () => {
    const snapshotWithJob = {
      ...snapshotWithSpace,
      parseJobs: [
        parseJob(),
      ],
    };
    Object.defineProperty(globalThis, "isTauri", {
      configurable: true,
      value: true,
    });
    mockIPC((cmd) => {
      if (cmd === "get_runtime_status") {
        return runtimeStatus;
      }
      if (cmd === "start_ocr_worker") {
        throw new Error("模型目录缺失");
      }
      return snapshotWithJob;
    });
    render(<App />);

    expect(await screen.findByText("解析队列")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "启动 OCR" }));

    expect(await screen.findByText("模型目录缺失")).toBeInTheDocument();
  });
});
