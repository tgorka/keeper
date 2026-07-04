import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ChatRow } from "@/components/chat/chat-row";
import type { RoomVm } from "@/lib/ipc/client";

function room(overrides: Partial<RoomVm> = {}): RoomVm {
  return {
    roomId: "!abc:example.org",
    displayName: "Alice Smith",
    lastMessage: "hey there",
    timestamp: Date.now(),
    avatarUrl: null,
    ...overrides,
  };
}

describe("ChatRow", () => {
  it("renders display name and preview", () => {
    render(<ChatRow room={room()} />);
    expect(screen.getByText("Alice Smith")).toBeInTheDocument();
    expect(screen.getByText("hey there")).toBeInTheDocument();
  });

  it("is a full-width accessible button with a room-labelled name", () => {
    render(<ChatRow room={room()} />);
    const button = screen.getByRole("button", { name: "Conversation with Alice Smith" });
    expect(button).toBeInTheDocument();
    expect(button).toHaveClass("w-full");
  });

  it("shows avatar fallback initials when no avatar url", () => {
    render(<ChatRow room={room({ displayName: "Alice Smith" })} />);
    expect(screen.getByText("AS")).toBeInTheDocument();
  });

  it("renders an empty preview when lastMessage is null", () => {
    render(<ChatRow room={room({ lastMessage: null })} />);
    expect(screen.queryByText("hey there")).not.toBeInTheDocument();
  });

  it("omits the timestamp when it is null", () => {
    const { container } = render(<ChatRow room={room({ timestamp: null })} />);
    // Only the display name and empty preview remain; no time text node.
    expect(container.querySelector(".text-xs")).toBeNull();
  });

  it("renders initials fallback and no img for an mxc:// avatar url", () => {
    const { container } = render(
      <ChatRow room={room({ displayName: "Alice Smith", avatarUrl: "mxc://x/y" })} />,
    );
    expect(screen.getByText("AS")).toBeInTheDocument();
    expect(container.querySelector('img[src="mxc://x/y"]')).toBeNull();
    expect(container.querySelector("img")).toBeNull();
  });

  it("renders an img for an https:// avatar url", async () => {
    // Radix Avatar only mounts the <img> once the image reports "loaded"; jsdom
    // never fires load events, so stub window.Image to dispatch "load" once src
    // is set.
    const RealImage = window.Image;
    class LoadingImage {
      #listeners: Record<string, Array<(e: unknown) => void>> = {};
      referrerPolicy = "";
      crossOrigin: string | null = null;
      complete = false;
      naturalWidth = 0;
      #src = "";
      addEventListener(type: string, cb: (e: unknown) => void): void {
        const list = this.#listeners[type] ?? [];
        list.push(cb);
        this.#listeners[type] = list;
      }
      removeEventListener(): void {}
      get src(): string {
        return this.#src;
      }
      set src(value: string) {
        this.#src = value;
        queueMicrotask(() => {
          this.complete = true;
          this.naturalWidth = 1;
          for (const cb of this.#listeners.load ?? []) {
            cb({ currentTarget: this });
          }
        });
      }
    }
    window.Image = LoadingImage as unknown as typeof Image;
    try {
      const { container } = render(
        <ChatRow room={room({ avatarUrl: "https://cdn.example.org/a.png" })} />,
      );
      await waitFor(() => {
        expect(container.querySelector('img[src="https://cdn.example.org/a.png"]')).not.toBeNull();
      });
    } finally {
      window.Image = RealImage;
    }
  });

  it("calls onSelect with the room id when clicked", () => {
    const onSelect = vi.fn();
    render(<ChatRow room={room({ roomId: "!xyz:example.org" })} onSelect={onSelect} />);
    fireEvent.click(screen.getByRole("button"));
    expect(onSelect).toHaveBeenCalledWith("!xyz:example.org");
  });
});
