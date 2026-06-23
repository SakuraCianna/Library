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
  BackupExportResult,
  BackupRestorePreflight,
  BackupRestoreResult,
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

const snapshotWithTablePreview: WorkbenchSnapshot = {
  ...snapshotWithSpace,
  tablePreview: {
    id: "table-report",
    title: "经营报表.xlsx · 工作表 1",
    description: "结构：3 行，3 列。表头：月份、营收、成本。",
  },
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

const backupExportResult: BackupExportResult = {
  path: "E:\\Library\\backups\\library-backup.json",
  fileName: "library-backup.json",
  sizeBytes: 512,
  exportedAt: "2026-06-23T00:00:00Z",
  fileCount: 2,
  knowledgeBlockCount: 1,
  parseJobCount: 0,
};

const backupRestorePreflight: BackupRestorePreflight = {
  path: "E:\\Library\\backups\\library-backup.json",
  fileName: "library-backup.json",
  format: "library.backup.v1",
  schemaVersion: 1,
  exportedAt: "2026-06-23T00:00:00Z",
  spaceId: "backup-space",
  spaceName: "备份空间",
  rootPath: "D:\\知识库\\备份空间",
  defaultPermission: "approval",
  fileCount: 2,
  knowledgeBlockCount: 1,
  parseJobCount: 0,
  trashEntryCount: 0,
  willOverwrite: true,
};

const backupRestoreResult: BackupRestoreResult = {
  ...backupRestorePreflight,
  restoredAt: "2026-06-23T00:10:00Z",
  overwritten: true,
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
          sourceKind: "original_file",
        },
      ],
    },
  ],
};

const answeredTableSnapshot: WorkbenchSnapshot = {
  ...snapshotWithSpace,
  messages: [
    {
      id: "msg-user-table",
      role: "user",
      content: "2026-06 营收是多少？",
      sources: [],
    },
    {
      id: "msg-assistant-table",
      role: "assistant",
      content:
        "根据本地索引，1. [表格洞察] 经营报表.xlsx · 工作表 1：样例 1：2026-06 | 120 | 70",
      sources: [
        {
          id: "table-report",
          title: "经营报表.xlsx · 工作表 1",
          excerpt: "表头：月份、营收、成本 样例 1：2026-06 | 120 | 70",
          sourceFileName: "经营报表.xlsx",
          sourceLocator: "经营报表.xlsx#sheet-001",
          sourceKind: "table",
        },
      ],
    },
  ],
};

const answeredMixedSourcesSnapshot: WorkbenchSnapshot = {
  ...snapshotWithSpace,
  messages: [
    {
      id: "msg-user-mixed",
      role: "user",
      content: "营收和扫描版发票一起看",
      sources: [],
    },
    {
      id: "msg-assistant-mixed",
      role: "assistant",
      content: "本地索引命中了文本、表格和 OCR 来源。",
      sources: [
        {
          id: "block-plain",
          title: "项目说明",
          excerpt: "普通文档提到营收。",
          sourceFileName: "项目说明.md",
          sourceLocator: "项目说明.md",
          sourceKind: "original_file",
        },
        {
          id: "block-markdown",
          title: "会议纪要",
          excerpt: "Markdown 笔记补充了营收背景。",
          sourceFileName: "会议纪要.md",
          sourceLocator: "会议纪要.md#block-001",
          sourceKind: "markdown_note",
        },
        {
          id: "block-table",
          title: "经营报表.xlsx · 工作表 1",
          excerpt: "样例 1：2026-06 | 120 | 70",
          sourceFileName: "经营报表.xlsx",
          sourceLocator: "经营报表.xlsx#sheet-001",
          sourceKind: "table",
        },
        {
          id: "block-ocr",
          title: "scan.pdf · OCR 片段 1/1",
          excerpt: "本地 OCR 识别到扫描版发票金额。",
          sourceFileName: "scan.pdf",
          sourceLocator: "scan.pdf#ocr-block-001",
          sourceKind: "ocr",
        },
      ],
    },
  ],
};

const redisSourceContext = {
  currentIndex: 1,
  totalCount: 2,
  blocks: [
    {
      id: "block-redis",
      title: "Redis 缓存穿透",
      excerpt: "缓存穿透需要空值缓存和布隆过滤器。",
      sourceFileName: "Redis面试.md",
      sourceLocator: "Redis面试.md#block-001",
    },
    {
      id: "block-redis-2",
      title: "Redis 缓存穿透 · 片段 2/2",
      excerpt: "布隆过滤器需要配合参数校验，避免不存在的键落到数据库。",
      sourceFileName: "Redis面试.md",
      sourceLocator: "Redis面试.md#block-002",
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
    expect(
      screen.getByRole("heading", { name: "未选择文件夹" }),
    ).toBeInTheDocument();
    expect(screen.getByText("请先添加一个真实文件夹")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /面试/ }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("待批准操作")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "打开设置" }));
    expect(
      await screen.findByRole("dialog", { name: "常规" }),
    ).toBeInTheDocument();
    expect(screen.getByText("当前知识库")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "模型与 OCR" }));
    expect(await screen.findByText("DeepSeek")).toBeInTheDocument();
    expect(screen.getByText("deepseek-v4-flash")).toBeInTheDocument();
    expect(screen.getByText("本地 OCR")).toBeInTheDocument();
  });

  it("shows folder default permission controls and rounded selects", async () => {
    render(<App />);

    expect(await screen.findByLabelText("切换文件夹默认权限")).toBeDisabled();
    expect(screen.getByRole("button", { name: "打开设置" })).toBeEnabled();
    expect(
      screen.getByRole("combobox", { name: "切换会话权限" }),
    ).toBeDisabled();
  });

  it("renders page-level source evidence labels", async () => {
    const snapshotWithPageEvidence: WorkbenchSnapshot = {
      ...snapshotWithSpace,
      blockPreview: {
        id: "block-pdf-page",
        title: "report.pdf · 第 1 页",
        excerpt: "证据范围：PDF 第 1/3 页 · 8 行 · 120 字 正文：PDF 第一页证据。",
        sourceFileName: "report.pdf",
        sourceLocator: "report.pdf#page-001",
      },
      messages: [
        {
          id: "msg-user-ocr-page",
          role: "user",
          content: "扫描版发票金额在哪里？",
          sources: [],
        },
        {
          id: "msg-assistant-ocr-page",
          role: "assistant",
          content: "扫描版发票金额在 OCR 第 1 页。",
          sources: [
            {
              id: "block-ocr-page",
              title: "scan.pdf · OCR 第 1 页 · 片段 1/2",
              excerpt:
                "证据范围：OCR 第 1/2 页 · 5 行 · 80 字 · 置信度 91% 正文摘录：扫描版发票金额。",
              sourceFileName: "scan.pdf",
              sourceLocator: "scan.pdf#ocr-page-001#block-001",
              sourceKind: "ocr",
            },
          ],
        },
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
      return snapshotWithPageEvidence;
    });

    render(<App />);

    expect(await screen.findByText("证据：PDF 第 1 页")).toBeInTheDocument();
    expect(
      screen.getByText("细节：PDF 第 1/3 页 · 8 行 · 120 字"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("证据：OCR 第 1 页 · 片段 1"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "细节：OCR 第 1/2 页 · 5 行 · 80 字 · 置信度 91%",
      ),
    ).toBeInTheDocument();
  });

  it("renders embedded document image source evidence labels", async () => {
    const snapshotWithImageEvidence: WorkbenchSnapshot = {
      ...snapshotWithSpace,
      blockPreview: {
        id: "block-docx-image",
        title: "架构说明.docx · 文档图片 1",
        excerpt:
          "证据范围：文档图片 1 · 5 行 · 110 字 正文：当前仅登记文档内图片和可用替代文本。",
        sourceFileName: "架构说明.docx",
        sourceLocator: "架构说明.docx#image-001",
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
      return snapshotWithImageEvidence;
    });

    render(<App />);

    expect(await screen.findByText("证据：文档图片 1")).toBeInTheDocument();
    expect(
      screen.getByText("细节：文档图片 1 · 5 行 · 110 字"),
    ).toBeInTheDocument();
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
    const gearButton = screen.getByRole("button", { name: "打开设置" });
    expect(gearButton).toBeEnabled();

    fireEvent.click(gearButton);

    expect(
      await screen.findByRole("dialog", { name: "常规" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "关闭设置" })).toHaveFocus();
    expect(screen.getAllByText("文件夹默认权限").length).toBeGreaterThan(0);
    expect(screen.getByText(/长期保存在本地 SQLite/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "权限" }));
    expect(
      await screen.findByRole("dialog", { name: "权限" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/会话权限不能超过文件夹默认权限/),
    ).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
    expect(gearButton).toHaveFocus();
  });

  it("renders real xlsx table insight preview from the snapshot", async () => {
    Object.defineProperty(globalThis, "isTauri", {
      configurable: true,
      value: true,
    });
    mockIPC((cmd) => {
      if (cmd === "get_runtime_status") {
        return runtimeStatus;
      }
      return snapshotWithTablePreview;
    });
    render(<App />);

    await screen.findByRole("heading", { name: "真实知识库" });

    expect(screen.getByText("经营报表.xlsx · 工作表 1")).toBeInTheDocument();
    expect(screen.getByText(/表头：月份、营收、成本/)).toBeInTheDocument();
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
    fireEvent.click(screen.getByRole("button", { name: "打开设置" }));
    fireEvent.click(screen.getByRole("button", { name: "模型与 OCR" }));
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
    expect(screen.getByRole("button", { name: "加入复习" })).toBeDisabled();

    const composer = screen.getByRole("form", { name: "智能助手输入区" });
    expect(
      within(composer).getByRole("button", { name: "发送" }),
    ).toBeInTheDocument();
  });

  it("exports a local backup from the folder toolbar", async () => {
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
      if (cmd === "export_space_backup") {
        return backupExportResult;
      }
      return snapshotWithSpace;
    });
    render(<App />);

    await screen.findByRole("heading", { name: "真实知识库" });
    fireEvent.click(screen.getByRole("button", { name: "导出备份" }));

    expect(
      await screen.findByText(/备份已导出 library-backup\.json/),
    ).toBeInTheDocument();
    expect(screen.getByText(/2 个文件 · 1 个知识块/)).toBeInTheDocument();
    expect(invocations).toContainEqual({
      cmd: "export_space_backup",
      args: {
        request: {
          spaceId: "space-real",
          fileName: null,
        },
      },
    });
  });

  it("preflights and confirms restoring a local backup from the folder toolbar", async () => {
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
      if (cmd === "plugin:dialog|open") {
        return "E:\\Library\\backups\\library-backup.json";
      }
      if (cmd === "preflight_space_backup_restore") {
        return backupRestorePreflight;
      }
      if (cmd === "restore_space_backup") {
        return backupRestoreResult;
      }
      return snapshotWithSpace;
    });
    render(<App />);

    await screen.findByRole("heading", { name: "真实知识库" });
    fireEvent.click(screen.getByRole("button", { name: "恢复备份" }));

    expect(
      await screen.findByText(/备份可恢复 library-backup\.json/),
    ).toBeInTheDocument();
    expect(screen.getByText(/备份空间 · 2 个文件 · 1 个知识块/)).toBeInTheDocument();
    expect(invocations.some((call) => call.cmd === "restore_space_backup")).toBe(
      false,
    );

    fireEvent.click(screen.getByRole("button", { name: "确认恢复" }));

    expect(
      await screen.findByText(/备份已恢复 library-backup\.json/),
    ).toBeInTheDocument();
    expect(invocations).toContainEqual({
      cmd: "preflight_space_backup_restore",
      args: {
        request: {
          path: "E:\\Library\\backups\\library-backup.json",
        },
      },
    });
    expect(invocations).toContainEqual({
      cmd: "restore_space_backup",
      args: {
        request: {
          path: "E:\\Library\\backups\\library-backup.json",
          confirmOverwrite: true,
        },
      },
    });
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
      if (cmd === "get_knowledge_block_context") {
        return redisSourceContext;
      }
      if (cmd === "open_source_file") {
        return null;
      }
      return snapshotWithSpace;
    });
    render(<App />);

    await screen.findByRole("heading", { name: "真实知识库" });
    expect(screen.getByRole("button", { name: "当前文件" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "当前文件夹" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "全库" })).toBeDisabled();
    const composer = screen.getByRole("form", { name: "智能助手输入区" });
    fireEvent.change(within(composer).getByLabelText("向智能助手提问"), {
      target: { value: "缓存穿透怎么处理？" },
    });
    fireEvent.click(within(composer).getByRole("button", { name: "发送" }));

    expect(await screen.findByText(/空值缓存、布隆过滤器/)).toBeInTheDocument();
    expect(screen.getByLabelText("回答来源")).toBeInTheDocument();
    expect(screen.getByText("Redis面试.md")).toBeInTheDocument();
    expect(screen.getAllByText("原始文件").length).toBeGreaterThan(0);
    expect(
      screen.getByText("定位：Redis面试.md#block-001"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("缓存穿透需要空值缓存和布隆过滤器。"),
    ).toBeInTheDocument();

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
    expect(within(sourcePanel).getByText("证据：片段 1")).toBeInTheDocument();
    expect(
      within(sourcePanel).getByText("缓存穿透需要空值缓存和布隆过滤器。"),
    ).toBeInTheDocument();
    expect(
      await within(sourcePanel).findByText("片段 1/2"),
    ).toBeInTheDocument();

    fireEvent.click(
      within(sourcePanel).getByRole("button", { name: "下一片段" }),
    );
    expect(
      within(sourcePanel).getByText("Redis 缓存穿透 · 片段 2/2"),
    ).toBeInTheDocument();
    expect(
      within(sourcePanel).getByText("定位：Redis面试.md#block-002"),
    ).toBeInTheDocument();
    expect(within(sourcePanel).getByText("证据：片段 2")).toBeInTheDocument();
    expect(within(sourcePanel).getByText("片段 2/2")).toBeInTheDocument();

    fireEvent.click(
      within(sourcePanel).getByRole("button", { name: "打开文件" }),
    );
    await waitFor(() =>
      expect(
        invocations.some((invocation) => invocation.cmd === "open_source_file"),
      ).toBe(true),
    );
    expect(invocations).toContainEqual({
      cmd: "open_source_file",
      args: {
        request: {
          spaceId: "space-real",
          sourceLocator: "Redis面试.md#block-002",
        },
      },
    });

    fireEvent.click(
      within(sourcePanel).getByRole("button", { name: "查看最新" }),
    );
    expect(
      screen.getByRole("article", { name: "知识块预览" }),
    ).toBeInTheDocument();
  });

  it("labels table insight sources in assistant answers", async () => {
    Object.defineProperty(globalThis, "isTauri", {
      configurable: true,
      value: true,
    });
    mockIPC((cmd) => {
      if (cmd === "get_runtime_status") {
        return runtimeStatus;
      }
      if (cmd === "ask_agent") {
        return answeredTableSnapshot;
      }
      return snapshotWithSpace;
    });
    render(<App />);

    await screen.findByRole("heading", { name: "真实知识库" });
    const composer = screen.getByRole("form", { name: "智能助手输入区" });
    fireEvent.change(within(composer).getByLabelText("向智能助手提问"), {
      target: { value: "2026-06 营收是多少？" },
    });
    fireEvent.click(within(composer).getByRole("button", { name: "发送" }));

    expect((await screen.findAllByText("表格洞察")).length).toBeGreaterThan(0);
    expect(screen.getByText("经营报表.xlsx")).toBeInTheDocument();
    expect(
      screen.getByText("定位：经营报表.xlsx#sheet-001"),
    ).toBeInTheDocument();
    expect(screen.getAllByText(/2026-06 \| 120 \| 70/).length).toBeGreaterThan(
      0,
    );
  });

  it("can hide assistant sources and filter them by source type", async () => {
    Object.defineProperty(globalThis, "isTauri", {
      configurable: true,
      value: true,
    });
    mockIPC((cmd) => {
      if (cmd === "get_runtime_status") {
        return runtimeStatus;
      }
      if (cmd === "ask_agent") {
        return answeredMixedSourcesSnapshot;
      }
      return snapshotWithSpace;
    });
    render(<App />);

    await screen.findByRole("heading", { name: "真实知识库" });
    const composer = screen.getByRole("form", { name: "智能助手输入区" });
    fireEvent.change(within(composer).getByLabelText("向智能助手提问"), {
      target: { value: "营收和扫描版发票一起看" },
    });
    fireEvent.click(within(composer).getByRole("button", { name: "发送" }));

    const sources = await screen.findByLabelText("回答来源");
    expect(within(sources).getByText("项目说明.md")).toBeInTheDocument();
    expect(within(sources).getByText("会议纪要.md")).toBeInTheDocument();
    expect(within(sources).getByText("经营报表.xlsx")).toBeInTheDocument();
    expect(within(sources).getByText("scan.pdf")).toBeInTheDocument();
    expect(
      within(sources).getByRole("button", { name: "全部来源" }),
    ).toHaveAttribute("aria-pressed", "true");

    fireEvent.click(
      within(sources).getByRole("button", { name: "Markdown 笔记" }),
    );
    expect(within(sources).queryByText("项目说明.md")).not.toBeInTheDocument();
    expect(within(sources).getByText("会议纪要.md")).toBeInTheDocument();
    expect(within(sources).queryByText("经营报表.xlsx")).not.toBeInTheDocument();
    expect(
      within(sources).getByRole("button", { name: "Markdown 笔记" }),
    ).toHaveAttribute("aria-pressed", "true");

    fireEvent.click(within(sources).getByRole("button", { name: "表格洞察" }));
    expect(within(sources).queryByText("项目说明.md")).not.toBeInTheDocument();
    expect(within(sources).queryByText("会议纪要.md")).not.toBeInTheDocument();
    expect(within(sources).getByText("经营报表.xlsx")).toBeInTheDocument();
    expect(within(sources).queryByText("scan.pdf")).not.toBeInTheDocument();

    fireEvent.click(within(sources).getByRole("button", { name: "本地 OCR" }));
    expect(within(sources).queryByText("经营报表.xlsx")).not.toBeInTheDocument();
    expect(within(sources).getByText("scan.pdf")).toBeInTheDocument();

    fireEvent.click(within(sources).getByRole("button", { name: "全部来源" }));
    expect(within(sources).getByText("项目说明.md")).toBeInTheDocument();
    expect(within(sources).getByText("会议纪要.md")).toBeInTheDocument();
    expect(within(sources).getByText("经营报表.xlsx")).toBeInTheDocument();
    expect(within(sources).getByText("scan.pdf")).toBeInTheDocument();
    expect(
      within(sources).getByRole("button", { name: "全部来源" }),
    ).toHaveAttribute("aria-pressed", "true");

    fireEvent.click(within(sources).getByRole("button", { name: "隐藏来源" }));
    expect(within(sources).queryByText("scan.pdf")).not.toBeInTheDocument();
    expect(
      within(sources).getByRole("button", { name: "显示来源" }),
    ).toBeInTheDocument();
  });

  it("renders parse queue status when jobs exist", async () => {
    const snapshotWithJob = {
      ...snapshotWithSpace,
      parseJobs: [parseJob({ id: "job-1", fileId: "file-scan" })],
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

  it("redacts absolute paths from parse queue errors", async () => {
    const snapshotWithPrivatePathError = {
      ...snapshotWithSpace,
      parseJobs: [
        parseJob({
          status: "failed",
          errorMessage:
            "OCR_RUNTIME_ERROR：E:\\Users\\Sakura\\Secret\\scan.pdf 处理失败",
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
      return snapshotWithPrivatePathError;
    });
    render(<App />);

    expect(
      await screen.findByText("OCR_RUNTIME_ERROR：本地文件路径 处理失败"),
    ).toBeInTheDocument();
    expect(screen.queryByText(/Sakura/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Secret/)).not.toBeInTheDocument();
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

    expect(
      await screen.findByText("本地 OCR · 已识别第 1/2 页"),
    ).toBeInTheDocument();
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
      parseJobs: [parseJob()],
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
    expect(
      screen.getByRole("button", { name: "排队 OCR scan.pdf" }),
    ).toBeDisabled();
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
    expect(
      screen.getByRole("button", { name: "排队 OCR scan.png" }),
    ).toBeDisabled();
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
    expect(
      screen.getByText("文件夹扫描 · 正在扫描 README.md"),
    ).toBeInTheDocument();
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
      parseJobs: [parseJob()],
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
        sourceLocator: "scan.pdf#ocr-page-001",
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
    expect(screen.getByText("证据：OCR 第 1 页")).toBeInTheDocument();
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
        sourceLocator: "scan.pdf#ocr-page-001",
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

    expect(
      await screen.findByText("本地 OCR · 正在执行本地 OCR"),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(eventListenMock).toHaveBeenCalledWith(
        "workbench-updated",
        expect.any(Function),
      ),
    );

    currentSnapshot = snapshotAfterEvent;
    emitWorkbenchUpdate?.();

    expect(
      await screen.findByText("事件刷新后的 OCR 文本"),
    ).toBeInTheDocument();
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

    expect(
      await screen.findByText("缓存穿透需要空值缓存和布隆过滤器。"),
    ).toBeInTheDocument();
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
      parseJobs: [parseJob()],
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
