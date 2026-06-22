import React from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { OcrEnvironmentReport, RuntimeStatus } from "../types/workbench";
import { useRuntimeStatus } from "./useRuntimeStatus";

const getRuntimeStatusMock = vi.hoisted(() => vi.fn());
const checkOcrEnvironmentMock = vi.hoisted(() => vi.fn());

vi.mock("../lib/tauriClient", () => ({
  getRuntimeStatus: getRuntimeStatusMock,
  checkOcrEnvironment: checkOcrEnvironmentMock,
}));

const runtimeStatus: RuntimeStatus = {
  deepseek: {
    configured: true,
    model: "deepseek-v4-flash",
    baseUrl: "https://api.deepseek.com",
    keyHint: "sk-****abcd",
  },
  ocr: {
    configured: true,
    tier: "medium",
    modelDir: "E:\\CodeHome\\Library\\models\\ocr\\pp-ocrv6",
    missingModels: [],
  },
};

const ocrEnvironmentReport: OcrEnvironmentReport = {
  ok: true,
  checks: [
    {
      name: "models",
      ok: true,
      message: "OCR model assets complete",
    },
  ],
};

function RuntimeProbe() {
  const {
    runtimeStatus: currentRuntimeStatus,
    ocrEnvironmentReport: currentOcrEnvironmentReport,
    checkingOcrEnvironment,
    checkOcrEnvironment,
  } = useRuntimeStatus();

  return (
    <section>
      <span data-testid="model">
        {currentRuntimeStatus?.deepseek.model ?? "loading"}
      </span>
      <span data-testid="checking">{String(checkingOcrEnvironment)}</span>
      <span data-testid="ocr-result">
        {currentOcrEnvironmentReport?.checks[0]?.name ?? ""}
      </span>
      <button onClick={() => void checkOcrEnvironment()} type="button">
        {checkingOcrEnvironment ? "自检中" : "自检"}
      </button>
    </section>
  );
}

describe("useRuntimeStatus", () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("recovers runtime and OCR check state under React StrictMode", async () => {
    getRuntimeStatusMock.mockResolvedValue(runtimeStatus);
    checkOcrEnvironmentMock.mockResolvedValue(ocrEnvironmentReport);

    render(
      <React.StrictMode>
        <RuntimeProbe />
      </React.StrictMode>,
    );

    expect(await screen.findByTestId("model")).toHaveTextContent(
      "deepseek-v4-flash",
    );

    fireEvent.click(screen.getByRole("button", { name: "自检" }));
    expect(screen.getByTestId("checking")).toHaveTextContent("true");

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "自检" })).toBeEnabled();
    });
    expect(screen.getByTestId("checking")).toHaveTextContent("false");
    expect(screen.getByTestId("ocr-result")).toHaveTextContent("models");
  });
});
