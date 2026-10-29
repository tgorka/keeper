import { invoke } from "@tauri-apps/api/core";
import { clearMocks } from "@tauri-apps/api/mocks";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { useEffect, useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SessionDrives } from "@/components/agents/session-drives";
import type { AgentSessionRef, AgentSessionViewVm } from "@/lib/ipc/client";
import { installMockShell, stageSessionDriveTick } from "../../dev/mock-shell";

const target = { database: "alpha", sessionId: "device.1" };

function Harness() {
  const [vm, setVm] = useState<AgentSessionViewVm>();
  const [open, setOpen] = useState(false);
  useEffect(() => {
    const subscription = invoke<string>("agent_session_open", {
      ...target,
      channel: { onmessage: setVm },
    });
    return () => {
      void subscription.then((subscriptionId) => invoke("agent_session_close", { subscriptionId }));
    };
  }, []);
  return vm?.drives ? (
    <SessionDrives
      target={target}
      agentName="Assistant"
      drives={vm.drives}
      open={open}
      onOpenChange={setOpen}
    />
  ) : null;
}

async function readDrives(ref: AgentSessionRef) {
  let vm: AgentSessionViewVm | undefined;
  const subscriptionId = await invoke<string>("agent_session_open", {
    ...ref,
    channel: {
      onmessage: (next: AgentSessionViewVm) => {
        vm = next;
      },
    },
  });
  await invoke("agent_session_close", { subscriptionId });
  return vm?.drives;
}

beforeEach(() => {
  vi.useFakeTimers();
  clearMocks();
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  installMockShell();
  stageSessionDriveTick(target, "alpha", false);
});

afterEach(async () => {
  cleanup();
  await act(async () => {});
  vi.clearAllTimers();
  vi.useRealTimers();
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  clearMocks();
});

describe("drive tick acknowledgements through the dev shell", () => {
  it("keeps a legacy warning and remedy until explicit untick and manual reselect", async () => {
    stageSessionDriveTick(target, "alpha", null);
    render(<Harness />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "OK" }));
    });
    expect(screen.getByRole("alert")).toHaveTextContent(/Untick it.*select it again/i);
    expect(screen.getByRole("note", { name: "What ticking alpha allows" })).toBeInTheDocument();
    expect((await readDrives(target))?.unseen).toEqual([{ drive: "alpha", tick: undefined }]);

    // A delayed no-id acknowledgement cannot adopt a later legacy selection either.
    await act(async () => {
      stageSessionDriveTick(target, "alpha", false);
      stageSessionDriveTick(target, "alpha", null);
      await expect(
        invoke("agent_note_tick_seen", { ...target, drive: "alpha", tick: null }),
      ).rejects.toMatchObject({ code: "internal", retriable: false });
    });
    expect((await readDrives(target))?.unseen).toEqual([{ drive: "alpha", tick: undefined }]);

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Untick alpha" }));
      await invoke("agent_note_tick_seen", { ...target, drive: "alpha", tick: null });
    });
    expect(screen.queryByRole("note")).toBeNull();
    expect(screen.getByText("No drive ticked")).toBeInTheDocument();
    await act(async () => {
      fireEvent.keyDown(screen.getByRole("button", { name: "Drives" }), { key: "ArrowDown" });
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("menuitemcheckbox", { name: "alpha" }));
    });
    expect(screen.getByRole("menuitemcheckbox", { name: "alpha" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect((await readDrives(target))?.unseen).toEqual([]);
    // The person's new tick is seen; a later replica tick still needs its own acknowledgement.
    await act(async () => {
      stageSessionDriveTick(target, "alpha", "remote-after-manual");
      fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    });
    expect(screen.getByRole("note", { name: "What ticking alpha allows" })).toBeInTheDocument();
  });

  it("clears only the matching modern warning, never a replacement", async () => {
    stageSessionDriveTick(target, "alpha", "shown-modern");
    render(<Harness />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "OK" }));
    });
    expect(screen.queryByRole("note")).toBeNull();
    expect(
      within(screen.getByRole("list", { name: "Drives ticked" })).getByText("alpha"),
    ).toBeInTheDocument();
    await act(async () => {
      stageSessionDriveTick(target, "alpha", "replacement-modern");
      await invoke("agent_note_tick_seen", {
        ...target,
        drive: "alpha",
        tick: "shown-modern",
      });
    });
    expect(screen.getByRole("note", { name: "What ticking alpha allows" })).toBeInTheDocument();
    expect((await readDrives(target))?.unseen).toEqual([
      { drive: "alpha", tick: "replacement-modern" },
    ]);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "OK" }));
    });
    expect(screen.queryByRole("note")).toBeNull();
  });

  it("gives every clean carry a fresh unacknowledged identity", async () => {
    stageSessionDriveTick(target, "alpha", "source-modern");
    await invoke("agent_note_tick_seen", { ...target, drive: "alpha", tick: "source-modern" });
    const carry = async (ref: AgentSessionRef) => {
      const result = await invoke<{ session: AgentSessionRef }>("agent_command", {
        ...ref,
        draft: "/clean",
      });
      return { target: result.session, drives: await readDrives(result.session) };
    };
    const first = await carry(target);
    const second = await carry(first.target);
    const firstTick = first.drives?.unseen?.find((row) => row.drive === "alpha")?.tick;
    const secondTick = second.drives?.unseen?.find((row) => row.drive === "alpha")?.tick;
    expect(firstTick).toEqual(expect.any(String));
    expect(secondTick).toEqual(expect.any(String));
    expect(new Set(["source-modern", firstTick, secondTick]).size).toBe(3);
    await invoke("agent_note_tick_seen", { ...second.target, drive: "alpha", tick: firstTick });
    expect((await readDrives(second.target))?.unseen).toEqual([
      { drive: "alpha", tick: secondTick },
    ]);
    await invoke("agent_note_tick_seen", { ...second.target, drive: "alpha", tick: secondTick });
    expect((await readDrives(second.target))?.unseen).toEqual([]);
  });
});
