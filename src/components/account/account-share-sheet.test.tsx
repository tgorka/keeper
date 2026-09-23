import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  accountShare: vi.fn(),
}));

import {
  AccountShareSheet,
  SHARE_COPIED_LABEL,
  SHARE_COPIED_RESET_MS,
  SHARE_COPY_FAILED,
  SHARE_COPY_LABEL,
  SHARE_NO_QR,
  SHARE_QR_ALT,
} from "@/components/account/account-share-sheet";
import { accountShare } from "@/lib/ipc/client";

const LINK = "keeper://setup?descriptor=https%3A%2F%2Fid.acme.dev%2Fk.json";
const SVG = '<svg xmlns="http://www.w3.org/2000/svg"><rect width="1" height="1"/></svg>';

/** Give the webview a clipboard (or take it away) for one test; `afterEach` removes it. */
function stubClipboard(clipboard: { writeText: (text: string) => Promise<void> } | undefined) {
  Object.defineProperty(navigator, "clipboard", { value: clipboard, configurable: true });
}

afterEach(() => {
  Reflect.deleteProperty(navigator, "clipboard");
  vi.useRealTimers();
  vi.clearAllMocks();
});

describe("AccountShareSheet", () => {
  it("draws Rust's QR at 240 px or more and copies the link it encodes", async () => {
    vi.mocked(accountShare).mockResolvedValue({ link: LINK, qrSvg: SVG });
    const writeText = vi.fn(() => Promise.resolve());
    stubClipboard({ writeText });
    render(<AccountShareSheet open onOpenChange={() => {}} />);

    const qr = await screen.findByRole<HTMLImageElement>("img", { name: SHARE_QR_ALT });
    expect(qr).toHaveAttribute("src", `data:image/svg+xml,${encodeURIComponent(SVG)}`);
    // DESIGN.md sizes the code, not its card: the image itself is ≥ 240 px.
    expect(qr.width).toBeGreaterThanOrEqual(240);
    expect(qr.height).toBeGreaterThanOrEqual(240);
    expect(screen.getByText(LINK)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: SHARE_COPY_LABEL }));
    expect(writeText).toHaveBeenCalledWith(LINK);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: SHARE_COPIED_LABEL })).toBeInTheDocument(),
    );
  });

  it("goes back to Copy link after the confirmation has been seen", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.mocked(accountShare).mockResolvedValue({ link: LINK, qrSvg: SVG });
    stubClipboard({ writeText: () => Promise.resolve() });
    render(<AccountShareSheet open onOpenChange={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: SHARE_COPY_LABEL }));
    await screen.findByRole("button", { name: SHARE_COPIED_LABEL });
    act(() => vi.advanceTimersByTime(SHARE_COPIED_RESET_MS));
    expect(screen.getByRole("button", { name: SHARE_COPY_LABEL })).toBeInTheDocument();
  });

  it("says so when the clipboard refuses the link, and when there is none", async () => {
    vi.mocked(accountShare).mockResolvedValue({ link: LINK, qrSvg: SVG });
    stubClipboard({ writeText: () => Promise.reject(new Error("NotAllowedError")) });
    const { unmount } = render(<AccountShareSheet open onOpenChange={() => {}} />);

    fireEvent.click(await screen.findByRole("button", { name: SHARE_COPY_LABEL }));
    expect(await screen.findByRole("alert")).toHaveTextContent(SHARE_COPY_FAILED);
    expect(screen.queryByRole("button", { name: SHARE_COPIED_LABEL })).not.toBeInTheDocument();
    unmount();

    stubClipboard(undefined);
    render(<AccountShareSheet open onOpenChange={() => {}} />);
    fireEvent.click(await screen.findByRole("button", { name: SHARE_COPY_LABEL }));
    expect(await screen.findByRole("alert")).toHaveTextContent(SHARE_COPY_FAILED);
  });

  it("says the link is too long for a code instead of drawing an empty card", async () => {
    vi.mocked(accountShare).mockResolvedValue({ link: LINK, qrSvg: null });
    render(<AccountShareSheet open onOpenChange={() => {}} />);

    expect(await screen.findByText(SHARE_NO_QR)).toBeInTheDocument();
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: SHARE_COPY_LABEL })).toBeInTheDocument();
  });
});
