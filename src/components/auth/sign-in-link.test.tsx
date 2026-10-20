import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/crash-report", () => ({ reportFrontendError: vi.fn() }));

import {
  SIGN_IN_LINK_CLIPBOARD_REFUSED,
  SIGN_IN_LINK_COPY_FAILED,
  SIGN_IN_LINK_COPY_SOURCE,
  SIGN_IN_LINK_NO_CLIPBOARD,
  SignInLink,
} from "@/components/auth/sign-in-link";
import { reportFrontendError } from "@/lib/crash-report";

const url = "https://id.acme.dev/authorize?client_id=keeper&state=s1&code_challenge=c";

/** Give the webview a clipboard (or take it away) for one test. */
function stubClipboard(clipboard: { writeText: (text: string) => Promise<void> } | undefined) {
  Object.defineProperty(navigator, "clipboard", { value: clipboard, configurable: true });
}

afterEach(() => {
  Reflect.deleteProperty(navigator, "clipboard");
  vi.clearAllMocks();
});

describe("SignInLink's Copy (102.9, R381, R480/04)", () => {
  it("says a refused copy where the person looks and logs a fixed reason, never the link", async () => {
    // The clipboard's own error quotes what it was handed.
    stubClipboard({ writeText: () => Promise.reject(new Error(`denied writing ${url}`)) });
    render(<SignInLink link={{ url, notOpened: null }} />);

    fireEvent.click(screen.getByRole("button", { name: "Copy link" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(SIGN_IN_LINK_COPY_FAILED);
    expect(reportFrontendError).toHaveBeenCalledExactlyOnceWith(
      SIGN_IN_LINK_COPY_SOURCE,
      SIGN_IN_LINK_CLIPBOARD_REFUSED,
      null,
    );
    expect(JSON.stringify(vi.mocked(reportFrontendError).mock.calls)).not.toContain("id.acme.dev");
  });

  it("says a missing clipboard the same way, and logs that it is missing", () => {
    stubClipboard(undefined);
    render(<SignInLink link={{ url, notOpened: null }} />);

    fireEvent.click(screen.getByRole("button", { name: "Copy link" }));

    expect(screen.getByRole("alert")).toHaveTextContent(SIGN_IN_LINK_COPY_FAILED);
    expect(reportFrontendError).toHaveBeenCalledExactlyOnceWith(
      SIGN_IN_LINK_COPY_SOURCE,
      SIGN_IN_LINK_NO_CLIPBOARD,
      null,
    );
    expect(JSON.stringify(vi.mocked(reportFrontendError).mock.calls)).not.toContain("id.acme.dev");
  });

  it("logs nothing when the copy worked", async () => {
    stubClipboard({ writeText: () => Promise.resolve() });
    render(<SignInLink link={{ url, notOpened: null }} />);

    fireEvent.click(screen.getByRole("button", { name: "Copy link" }));

    expect(await screen.findByText("Link copied")).toBeInTheDocument();
    expect(reportFrontendError).not.toHaveBeenCalled();
  });
});
