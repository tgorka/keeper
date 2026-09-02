/**
 * The tray and the deliverable paths (Epic 61, Story 61.12, FR-392, FR-393,
 * AD-58, AD-160).
 *
 * Four things are asserted here that nothing else in the tree asserts:
 *
 * 1. **No base64 and no remote URL ever reaches the DOM.** The thumbnail's
 *    `src` is the `blob:` URL the webview made from the clipboard file it
 *    already held; a `data:` URI in an `img` would be the AD-58 violation, and
 *    an `http(s)` one would be the tracking-pixel position `note_protocol.rs`
 *    already refuses for model-authored content.
 * 2. **An unmeasured dimension renders as unmeasured.** A picture whose decode
 *    has not finished says its size on disk and nothing about its pixels —
 *    this is the app that refuses to print a number it did not measure.
 * 3. **The control exists only inside a grant, and is ABSENT outside one.** Not
 *    disabled, not hidden without explanation: absent, with the sentence saying
 *    why (AD-27).
 * 4. **A path is never rewritten out of a reply.** The row shows the characters
 *    the reply used, which is the whole difference between keeper's receiving
 *    end and the Hermes gateway's stripping one.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  BOT_ATTACHMENT_STAGING,
  BOT_DELIVERABLE_OPEN,
  BOT_DIMENSIONS_UNKNOWN,
  BotAttachmentStrip,
  BotDeliverablePaths,
  type BotPendingImage,
  BotReplyPaths,
  botAttachmentCaption,
} from "@/components/bots/bot-attachment";
import type { BotDeliverableVm } from "@/lib/ipc/client";

const botsDeliverablePaths = vi.fn<(sessionId: string, body: string) => Promise<unknown>>();
const syncOpenEntry = vi.fn<(id: string, subpath: string) => Promise<void>>();

vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    botsDeliverablePaths: (sessionId: string, body: string) =>
      botsDeliverablePaths(sessionId, body),
    syncOpenEntry: (id: string, subpath: string) => syncOpenEntry(id, subpath),
  };
});

function image(overrides: Partial<BotPendingImage> = {}): BotPendingImage {
  return {
    id: "01J8IMAGE",
    filename: "pasted-image.png",
    mime: "image/png",
    size: 184_320,
    width: 1280,
    height: 720,
    previewUrl: "blob:keeper/01J8IMAGE",
    ...overrides,
  };
}

function deliverable(overrides: Partial<BotDeliverableVm> = {}): BotDeliverableVm {
  return {
    raw: "/Users/ada/Drive/journal/2026/notes.md",
    absolute: "/Users/ada/Drive/journal/2026/notes.md",
    start: 10,
    end: 48,
    profileId: "p1",
    subpath: "journal/2026/notes.md",
    reason: null,
    ...overrides,
  };
}

beforeEach(() => {
  botsDeliverablePaths.mockReset();
  syncOpenEntry.mockReset();
  syncOpenEntry.mockResolvedValue(undefined);
});

describe("BotAttachmentStrip", () => {
  it("renders nothing when there is neither an image nor anything to say", () => {
    const { container } = render(
      <BotAttachmentStrip images={[]} notice={null} onRemove={() => {}} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("shows the thumbnail from a blob URL and never a data URI", () => {
    render(<BotAttachmentStrip images={[image()]} notice={null} onRemove={() => {}} />);
    const thumbnail = screen.getByAltText("pasted-image.png");
    expect(thumbnail).toHaveAttribute("src", "blob:keeper/01J8IMAGE");
    const src = thumbnail.getAttribute("src") ?? "";
    expect(src.startsWith("data:")).toBe(false);
    expect(src.startsWith("http")).toBe(false);
  });

  it("prints the dimensions and the size it measured", () => {
    render(<BotAttachmentStrip images={[image()]} notice={null} onRemove={() => {}} />);
    expect(screen.getByText("1280×720, 180 kB")).toBeInTheDocument();
  });

  it("prints an unmeasured dimension as unmeasured, never as zero", () => {
    render(
      <BotAttachmentStrip
        images={[image({ width: null, height: null })]}
        notice={null}
        onRemove={() => {}}
      />,
    );
    expect(screen.getByText(`180 kB, ${BOT_DIMENSIONS_UNKNOWN}`)).toBeInTheDocument();
    expect(screen.queryByText(/0×0/)).not.toBeInTheDocument();
    expect(botAttachmentCaption(image({ width: null, height: 720 }))).toContain(
      BOT_DIMENSIONS_UNKNOWN,
    );
  });

  it("says an image is still being staged rather than showing a stale caption", () => {
    render(<BotAttachmentStrip images={[image({ id: null })]} notice={null} onRemove={() => {}} />);
    expect(screen.getByText(BOT_ATTACHMENT_STAGING)).toBeInTheDocument();
  });

  it("takes one image back off the message by index", () => {
    const onRemove = vi.fn();
    render(
      <BotAttachmentStrip
        images={[image(), image({ filename: "second.png", previewUrl: "blob:keeper/2" })]}
        notice={null}
        onRemove={onRemove}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /second\.png/ }));
    expect(onRemove).toHaveBeenCalledWith(1);
  });

  it("shows a refusal with no image beside it", () => {
    render(
      <BotAttachmentStrip
        images={[]}
        notice="llama4:8b does not accept images, so the paste was not attached."
        onRemove={() => {}}
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent("llama4:8b does not accept images");
    expect(screen.queryByLabelText("Attached images")).not.toBeInTheDocument();
  });
});

describe("BotDeliverablePaths", () => {
  it("renders nothing when the reply named no path", () => {
    const { container } = render(<BotDeliverablePaths paths={[]} onReveal={() => {}} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("offers the control for a path inside a grant, and opens it contained", () => {
    const onReveal = vi.fn();
    render(<BotDeliverablePaths paths={[deliverable()]} onReveal={onReveal} />);
    fireEvent.click(screen.getByRole("button", { name: BOT_DELIVERABLE_OPEN }));
    expect(onReveal).toHaveBeenCalledWith("p1", "journal/2026/notes.md");
  });

  it("shows the path outside a grant as text with its reason and no control", () => {
    render(
      <BotDeliverablePaths
        paths={[
          deliverable({
            raw: "/etc/hosts",
            absolute: "/etc/hosts",
            profileId: null,
            subpath: null,
            reason: "That path is outside every folder keeper syncs.",
          }),
        ]}
        onReveal={() => {}}
      />,
    );
    expect(screen.getByText("/etc/hosts")).toBeInTheDocument();
    expect(screen.getByText("That path is outside every folder keeper syncs.")).toBeInTheDocument();
    // Absent, not disabled: AD-27's rule is that an affordance which cannot
    // work does not exist, and the reason is on screen instead.
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("shows the path exactly as the reply spelled it", () => {
    render(
      <BotDeliverablePaths paths={[deliverable({ raw: "~/Drive/a.md" })]} onReveal={() => {}} />,
    );
    expect(screen.getByText("~/Drive/a.md")).toBeInTheDocument();
  });
});

describe("BotReplyPaths", () => {
  it("asks nothing while the answer is still arriving", () => {
    render(<BotReplyPaths sessionId="s1" body="Saved to /Users/ada/Drive/a.md" streaming />);
    expect(botsDeliverablePaths).not.toHaveBeenCalled();
  });

  it("resolves a finished answer against the live grants and renders the control", async () => {
    botsDeliverablePaths.mockResolvedValue([deliverable()]);
    render(
      <BotReplyPaths
        sessionId="s1"
        body="Saved to /Users/ada/Drive/journal/2026/notes.md"
        streaming={false}
      />,
    );
    await waitFor(() => {
      expect(screen.getByRole("button", { name: BOT_DELIVERABLE_OPEN })).toBeInTheDocument();
    });
    expect(botsDeliverablePaths).toHaveBeenCalledWith(
      "s1",
      "Saved to /Users/ada/Drive/journal/2026/notes.md",
    );
    fireEvent.click(screen.getByRole("button", { name: BOT_DELIVERABLE_OPEN }));
    expect(syncOpenEntry).toHaveBeenCalledWith("p1", "journal/2026/notes.md");
  });

  it("renders no control when the resolution failed", async () => {
    botsDeliverablePaths.mockRejectedValue({ code: "internal", message: "no" });
    const { container } = render(
      <BotReplyPaths sessionId="s1" body="Saved to /Users/ada/Drive/a.md" streaming={false} />,
    );
    await waitFor(() => {
      expect(container).toBeEmptyDOMElement();
    });
  });
});
