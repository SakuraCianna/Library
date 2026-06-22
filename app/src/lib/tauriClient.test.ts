import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it } from "vitest";

import { emptyWorkbench } from "../data/emptyWorkbench";
import {
  createKnowledgeSpace,
  selectKnowledgeFolder,
} from "./tauriClient";

describe("tauriClient", () => {
  afterEach(() => {
    clearMocks();
    Reflect.deleteProperty(globalThis, "isTauri");
  });

  it("uses Tauri internals for the folder picker even without global isTauri", async () => {
    const calls: Array<{ cmd: string; args?: unknown }> = [];

    Reflect.deleteProperty(globalThis, "isTauri");
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });

      if (cmd === "plugin:dialog|open") {
        return "D:\\知识库\\真实";
      }

      if (cmd === "create_knowledge_space") {
        return {
          ...emptyWorkbench,
          activeSpaceId: "space-real",
          spaces: [
            {
              id: "space-real",
              name: "真实",
              path: "D:\\知识库\\真实",
              defaultPermission: "approval",
              changedFileCount: 0,
              scanQueueCount: 0,
              documentQueueCount: 0,
              ocrQueueCount: 0,
            },
          ],
        };
      }

      return emptyWorkbench;
    });

    await expect(selectKnowledgeFolder()).resolves.toBe("D:\\知识库\\真实");

    const snapshot = await createKnowledgeSpace("D:\\知识库\\真实", "approval");

    expect(snapshot.activeSpaceId).toBe("space-real");
    expect(calls).toContainEqual({
      cmd: "plugin:dialog|open",
      args: {
        options: {
          directory: true,
          multiple: false,
          title: "选择知识库文件夹",
        },
      },
    });
    expect(calls).toContainEqual({
      cmd: "create_knowledge_space",
      args: {
        request: {
          name: "真实",
          rootPath: "D:\\知识库\\真实",
          defaultPermission: "approval",
        },
      },
    });
  });
});
