/**
 * What the person does to the default agent outside Settings (story 100.3,
 * story 102.7, R383, R463): make an agent the default (or stop one being it)
 * from the room header's menu or an Agents row's menu, and open an agent's
 * conversation with its composer focused — after the launch, or after a
 * choice in the launch's chooser.
 *
 * Every sentence but the success line is Rust's: a failed save's, the file
 * that decides the key, and why the agent will not open at launch.
 */
import { toast } from "sonner";
import type { AgentsLaunchVm, LaunchRoomVm } from "@/lib/ipc/client";
import { openMainDm } from "@/lib/main-dm";
import { composerStore } from "@/lib/stores/composer";
import { defaultAgentStore } from "@/lib/stores/default-agent";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";

/** The line after a choice that will open at launch. */
export function opensAtLaunchSentence(name: string): string {
  return `${name} opens when keeper starts.`;
}

/**
 * Store `next` as the default agent (`null`: none) and say what came of it:
 * a failure, the file that decides the key instead, why the agent will not
 * open at launch, or that it will. The answer, or `null` when not saved.
 */
export async function saveDefaultAgent(next: string | null): Promise<AgentsLaunchVm | null> {
  const saved = await defaultAgentStore.getState().choose(next);
  if (saved === null) {
    toast.error(defaultAgentStore.getState().saveError);
  } else if (saved.fileSet !== null) {
    // Stored, but a file decides what opens: say that, not the choice.
    toast(saved.fileSet);
  } else if (next !== null && saved.defaultAgent === next) {
    if (saved.room === null && saved.ask === null && saved.reason !== null) {
      // Stored, and Rust says why it will not open at launch here.
      toast(saved.reason);
    } else {
      const name = saved.choices.find((c) => c.agent === next)?.name ?? next;
      toast(opensAtLaunchSentence(name));
    }
  }
  return saved;
}

/**
 * Whether the person has moved since it was taken — opened another room or
 * another window — or the surface that took it went away (`cancel`). The
 * person always wins (R397): taken before anything is awaited, it keeps a
 * slow save or a slow conversation request from opening an agent over
 * whatever the person chose meanwhile.
 */
export interface NavigationGuard {
  /** The person moved, or the guard was cancelled. */
  moved: () => boolean;
  /** Count as moved: the surface that took the guard went away. */
  cancel: () => void;
  /** Stop watching (the guarded step is over). */
  release: () => void;
}

/** Start watching the person's room and window; `onMove` runs once, on the first move. */
export function guardNavigation(onMove?: () => void): NavigationGuard {
  let moved = false;
  const watching: Array<() => void> = [];
  const release = () => {
    for (const stop of watching.splice(0)) {
      stop();
    }
  };
  const move = () => {
    if (!moved) {
      moved = true;
      release();
      onMove?.();
    }
  };
  watching.push(
    roomsStore.subscribe(({ selected: now }, { selected: was }) => {
      // By value: a fresh selection of the same room is not a move.
      if (now?.accountId !== was?.accountId || now?.roomId !== was?.roomId) {
        move();
      }
    }),
    primaryViewStore.subscribe((state, prev) => {
      if (state.view !== prev.view) {
        move();
      }
    }),
  );
  return { moved: () => moved, cancel: move, release };
}

/**
 * Select `room` and focus its composer once it can take focus — on the
 * desktop and the phone alike (R463). The room's composer is disabled until
 * its first timeline batch arrives, so the request is the room's and waits
 * for that composer to be enabled (no guessed delay); it is dropped when the
 * person opens another room or window first.
 */
export function openAndFocus(room: LaunchRoomVm): void {
  roomsStore.getState().selectRoom(room);
  composerStore.getState().focusRoomWhenReady(room);
  const request = composerStore.getState().focusWhenReady;
  if (request === null) {
    return;
  }
  let stopSettled = () => {};
  const guard = guardNavigation(() => {
    composerStore.getState().dropFocus(request);
    stopSettled();
  });
  // Landed, or replaced by a newer request: nothing left to watch.
  stopSettled = composerStore.subscribe((state) => {
    if (state.focusWhenReady !== request) {
      guard.release();
      stopSettled();
    }
  });
}

/**
 * Open the conversation Rust's answer names for its default agent: its
 * `main` room, or — when it is the person's own agent with none here —
 * the one keeper asks its host for (R416), once it lands. Nothing opens
 * once `guard` says the person moved; the guard is released either way.
 */
export function openDefaultAgent(vm: AgentsLaunchVm, guard: NavigationGuard): void {
  if (guard.moved()) {
    guard.release();
    return;
  }
  if (vm.room !== null) {
    guard.release();
    openAndFocus(vm.room);
    return;
  }
  if (vm.ask === null) {
    guard.release();
    return;
  }
  void openMainDm(vm.ask)
    .then((room) => {
      if (room !== null && !guard.moved()) {
        openAndFocus(room);
      }
    })
    .finally(guard.release);
}
