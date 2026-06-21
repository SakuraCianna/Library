import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import App from "../App";

describe("App", () => {
  it("renders the Chinese three-column workbench", () => {
    render(<App />);

    expect(screen.getByRole("heading", { name: "知识库" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /面试/ })).toBeInTheDocument();
    expect(screen.getAllByText("D:\\知识库\\面试")[0]).toBeInTheDocument();
    expect(screen.getByText("文件夹总览 README.md")).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "智能助手" }),
    ).toBeInTheDocument();
    expect(screen.getByPlaceholderText("询问当前文件夹")).toBeInTheDocument();
  });

  it("exposes active states and key workbench status text", () => {
    render(<App />);

    expect(screen.getByRole("button", { name: "总览" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(screen.getByRole("button", { name: "文件" })).not.toHaveAttribute(
      "aria-current",
    );
    expect(screen.getByRole("button", { name: "当前文件夹" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("button", { name: "当前文件" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
    expect(screen.getByText("待批准操作")).toBeInTheDocument();
    expect(screen.getByText("已索引")).toBeInTheDocument();
    expect(screen.getByText("已变更")).toBeInTheDocument();
    expect(screen.getByText("表格模型就绪")).toBeInTheDocument();
  });
});
