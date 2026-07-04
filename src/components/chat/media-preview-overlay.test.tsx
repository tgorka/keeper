import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { MediaPreviewOverlay } from "@/components/chat/media-preview-overlay";
import type { MediaVm } from "@/lib/ipc/client";

function media(overrides: Partial<MediaVm> = {}): MediaVm {
  return {
    kind: "image",
    url: "keeper-media://media/acct/room/k1/full",
    thumbnailUrl: "keeper-media://media/acct/room/k1/thumb",
    filename: "photo.png",
    mimetype: "image/png",
    size: 12345,
    width: 800,
    height: 600,
    caption: null,
    ...overrides,
  };
}

describe("MediaPreviewOverlay", () => {
  it("renders nothing when there is no media", () => {
    render(<MediaPreviewOverlay media={null} onClose={() => {}} />);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("opens with the full-resolution image from the full protocol url", () => {
    render(<MediaPreviewOverlay media={media()} onClose={() => {}} />);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    const img = screen.getByAltText("photo.png") as HTMLImageElement;
    expect(img.getAttribute("src")).toBe("keeper-media://media/acct/room/k1/full");
  });

  it("renders a playable <video> for a video preview", () => {
    // The dialog content renders in a portal (document.body), so query the whole
    // document rather than the render container.
    render(
      <MediaPreviewOverlay
        media={media({ kind: "video", filename: "clip.mp4", mimetype: "video/mp4" })}
        onClose={() => {}}
      />,
    );
    const video = document.querySelector("video");
    expect(video).not.toBeNull();
    expect(video?.getAttribute("src")).toBe("keeper-media://media/acct/room/k1/full");
    expect(video?.hasAttribute("controls")).toBe(true);
  });

  it("renders a playable <audio> for an audio preview", () => {
    render(
      <MediaPreviewOverlay
        media={media({ kind: "audio", thumbnailUrl: null, filename: "clip.ogg" })}
        onClose={() => {}}
      />,
    );
    const audio = document.querySelector("audio");
    expect(audio).not.toBeNull();
    expect(audio?.hasAttribute("controls")).toBe(true);
  });

  it("shows a retry affordance when the full-resolution media fails to load", () => {
    render(<MediaPreviewOverlay media={media()} onClose={() => {}} />);
    const img = screen.getByAltText("photo.png");
    fireEvent.error(img);
    // The broken element is replaced by an honest retry affordance, not a blank.
    const retry = screen.getByRole("button", { name: /retry/i });
    expect(retry).toBeInTheDocument();

    fireEvent.click(retry);
    // The image re-appears with a cache-busting suffix so the handler re-fetches.
    const reloaded = screen.getByAltText("photo.png") as HTMLImageElement;
    expect(reloaded.getAttribute("src")).toContain("retry=1");
  });

  it("closes on Escape", () => {
    const onClose = vi.fn();
    render(<MediaPreviewOverlay media={media()} onClose={onClose} />);
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });
});
