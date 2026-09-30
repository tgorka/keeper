import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { APP_CRASH_RELOAD, APP_CRASH_TITLE, AppCrashBoundary } from "./app-crash-boundary";

const { frontendErrorReport } = vi.hoisted(() => ({ frontendErrorReport: vi.fn(async () => {}) }));
vi.mock("@/lib/ipc/client", () => ({ frontendErrorReport }));

function Explodes({ message }: { message: string }): never {
  throw new Error(message);
}

describe("AppCrashBoundary", () => {
  beforeEach(() => {
    frontendErrorReport.mockClear();
    vi.spyOn(console, "error").mockImplementation(() => {});
  });

  it("says what broke and offers a reload instead of leaving an empty window", () => {
    render(
      <AppCrashBoundary>
        <Explodes message="properties exploded" />
      </AppCrashBoundary>,
    );

    expect(screen.getByRole("alert")).toHaveTextContent(APP_CRASH_TITLE);
    expect(screen.getByText("properties exploded")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: APP_CRASH_RELOAD })).toBeInTheDocument();
  });

  it("writes the error and the component that threw to the app log once", () => {
    render(
      <AppCrashBoundary>
        <Explodes message="written once" />
      </AppCrashBoundary>,
    );
    render(
      <AppCrashBoundary>
        <Explodes message="written once" />
      </AppCrashBoundary>,
    );

    expect(frontendErrorReport).toHaveBeenCalledTimes(1);
    const [source, message, , componentStack] = frontendErrorReport.mock.calls[0] as unknown as [
      string,
      string,
      string | null,
      string | null,
    ];
    expect(source).toBe("render");
    expect(message).toBe("Error: written once");
    expect(componentStack).toContain("Explodes");
  });
});
