import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MediaAttachment } from "@/components/chat/media-attachment";
import type { MediaVm } from "@/lib/ipc/client";
import { lifecycleStore } from "@/lib/stores/lifecycle";

// The shed reads the shared lifecycleStore singleton; reset it after every test
// so no phase leaks into an order-dependent sibling suite.
afterEach(() => {
  lifecycleStore.setState({ phase: "foreground" });
});

function background(): void {
  act(() => {
    lifecycleStore.getState().setPhase("background");
  });
}

function foreground(): void {
  act(() => {
    lifecycleStore.getState().setPhase("foreground");
  });
}

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

describe("MediaAttachment", () => {
  it("renders an image thumbnail from the thumbnail protocol url", () => {
    render(<MediaAttachment media={media()} messageKey="k1" />);
    const img = screen.getByAltText("photo.png") as HTMLImageElement;
    expect(img).toBeInTheDocument();
    expect(img.getAttribute("src")).toBe("keeper-media://media/acct/room/k1/thumb");
  });

  it("renders a file chip with name and human size", () => {
    render(
      <MediaAttachment
        media={media({
          kind: "file",
          thumbnailUrl: null,
          filename: "report.pdf",
          mimetype: "application/pdf",
          size: 1048576,
          width: null,
          height: null,
        })}
        messageKey="k1"
      />,
    );
    expect(screen.getByText("report.pdf")).toBeInTheDocument();
    // 1 MiB rendered as a human size.
    expect(screen.getByText(/1\.0 MB/)).toBeInTheDocument();
  });

  it("renders an inline audio element from the full protocol url", () => {
    const { container } = render(
      <MediaAttachment
        media={media({
          kind: "audio",
          thumbnailUrl: null,
          filename: "clip.ogg",
          mimetype: "audio/ogg",
          width: null,
          height: null,
        })}
        messageKey="k1"
      />,
    );
    const audio = container.querySelector("audio");
    expect(audio).not.toBeNull();
    expect(audio?.getAttribute("src")).toBe("keeper-media://media/acct/room/k1/full");
  });

  it("shows a loading skeleton until the image loads", () => {
    const { container } = render(<MediaAttachment media={media()} messageKey="k1" />);
    // The skeleton renders before the image fires onLoad.
    expect(container.querySelector('[data-slot="skeleton"]')).not.toBeNull();
    // After load, the skeleton is gone.
    const img = screen.getByAltText("photo.png");
    fireEvent.load(img);
    expect(container.querySelector('[data-slot="skeleton"]')).toBeNull();
  });

  it("shows a retry affordance on error and reloads the src with a cache-buster", () => {
    render(<MediaAttachment media={media()} messageKey="k1" />);
    const img = screen.getByAltText("photo.png");
    fireEvent.error(img);
    // The image is replaced by a retry affordance.
    const retry = screen.getByRole("button", { name: /retry/i });
    expect(retry).toBeInTheDocument();

    fireEvent.click(retry);
    // The image re-appears with a cache-busting suffix on its src.
    const reloaded = screen.getByAltText("photo.png") as HTMLImageElement;
    expect(reloaded.getAttribute("src")).toContain("retry=1");
  });

  it("renders a static placeholder (no fetching <video>) for a posterless video", () => {
    const onOpenPreview = vi.fn();
    const { container } = render(
      <MediaAttachment
        media={media({
          kind: "video",
          thumbnailUrl: null,
          filename: "clip.mp4",
          mimetype: "video/mp4",
        })}
        messageKey="k1"
        onOpenPreview={onOpenPreview}
      />,
    );
    // No <video> element is mounted in the bubble — that would force a full
    // download+decrypt just to render a poster. The full video loads in the overlay.
    expect(container.querySelector("video")).toBeNull();
    // The play affordance is shown immediately and the placeholder opens the preview.
    fireEvent.click(screen.getByRole("button", { name: "Open video clip.mp4" }));
    expect(onOpenPreview).toHaveBeenCalledWith("k1");
  });

  it("calls onOpenPreview when the image is clicked", () => {
    const onOpenPreview = vi.fn();
    render(<MediaAttachment media={media()} messageKey="k1" onOpenPreview={onOpenPreview} />);
    fireEvent.click(screen.getByRole("button", { name: "Open image photo.png" }));
    expect(onOpenPreview).toHaveBeenCalledWith("k1");
  });

  it("does not make the image clickable when no preview handler is wired", () => {
    render(<MediaAttachment media={media()} messageKey="k1" />);
    const button = screen.getByRole("button", { name: "Open image photo.png" });
    expect(button).toBeDisabled();
  });

  it("drops the image src on background and restores it on foreground (Story 14.5)", () => {
    const { container } = render(<MediaAttachment media={media()} messageKey="k1" />);
    const img = screen.getByAltText("photo.png") as HTMLImageElement;
    expect(img.getAttribute("src")).toBe("keeper-media://media/acct/room/k1/thumb");
    // Load the image so the skeleton is normally gone in the foreground.
    fireEvent.load(img);
    expect(container.querySelector('[data-slot="skeleton"]')).toBeNull();

    // Background: the shed fires — the image src is dropped (bitmap released)
    // and the skeleton is shown again so the re-load is covered on restore.
    background();
    const shed = screen.getByAltText("photo.png") as HTMLImageElement;
    expect(shed.getAttribute("src")).toBeNull();
    expect(container.querySelector('[data-slot="skeleton"]')).not.toBeNull();

    // Foreground: the src is restored so the image re-hydrates.
    foreground();
    const restored = screen.getByAltText("photo.png") as HTMLImageElement;
    expect(restored.getAttribute("src")).toBe("keeper-media://media/acct/room/k1/thumb");
  });

  it("does NOT drop the inline audio src across a shed cycle (regression guard)", () => {
    // Dropping an <audio> src would reset playback to 0 and force a re-download;
    // audio playback is explicitly exempt from the shed (Story 14.5, finding #1).
    const { container } = render(
      <MediaAttachment
        media={media({
          kind: "audio",
          thumbnailUrl: null,
          filename: "clip.ogg",
          mimetype: "audio/ogg",
          width: null,
          height: null,
        })}
        messageKey="k1"
      />,
    );
    const audio = container.querySelector("audio");
    expect(audio?.getAttribute("src")).toBe("keeper-media://media/acct/room/k1/full");

    background();
    // The audio src stays put — playback is never reset by the shed.
    expect(container.querySelector("audio")?.getAttribute("src")).toBe(
      "keeper-media://media/acct/room/k1/full",
    );
    foreground();
    expect(container.querySelector("audio")?.getAttribute("src")).toBe(
      "keeper-media://media/acct/room/k1/full",
    );
  });

  it("does NOT drop the video poster src across a shed cycle (regression guard)", () => {
    // Dropping the poster <img> would flip hasPoster and morph the postered
    // video to its placeholder across a background round-trip (Story 14.5).
    const { container } = render(
      <MediaAttachment
        media={media({ kind: "video", filename: "clip.mp4", mimetype: "video/mp4" })}
        messageKey="k1"
      />,
    );
    const poster = screen.getByAltText("clip.mp4") as HTMLImageElement;
    expect(poster.getAttribute("src")).toBe("keeper-media://media/acct/room/k1/thumb");

    background();
    // The poster src stays put — the video does not morph to its placeholder.
    expect(screen.getByAltText("clip.mp4").getAttribute("src")).toBe(
      "keeper-media://media/acct/room/k1/thumb",
    );
    // No FileVideo placeholder appears (hasPoster stayed true).
    expect(container.querySelector("video")).toBeNull();
  });
});
