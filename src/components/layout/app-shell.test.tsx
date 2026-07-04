import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { AppShell } from "@/components/layout/app-shell";

describe("AppShell", () => {
  it("renders the semantic landmarks", () => {
    render(<AppShell />);
    expect(screen.getByRole("navigation", { name: "Views" })).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "Conversations" })).toBeInTheDocument();
    expect(screen.getByRole("main")).toBeInTheDocument();
  });

  it("renders the placeholder copy without any Matrix data", () => {
    render(<AppShell />);
    expect(screen.getByText("Synced. No conversations yet.")).toBeInTheDocument();
    expect(screen.getByText("Select a conversation to start reading.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Chats" })).toBeInTheDocument();
  });

  it("opens and closes the detail panel via the toggle control", () => {
    render(<AppShell />);

    expect(screen.queryByRole("complementary")).not.toBeInTheDocument();

    const toggle = screen.getByRole("button", { name: "Toggle detail panel" });
    fireEvent.click(toggle);
    expect(screen.getByRole("complementary", { name: "Details" })).toBeInTheDocument();

    fireEvent.click(toggle);
    expect(screen.queryByRole("complementary")).not.toBeInTheDocument();
  });
});
