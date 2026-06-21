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
});
