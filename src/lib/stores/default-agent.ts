/**
 * The default agent (story 100.3, R383, R397): Rust's answer to "what opens at
 * launch, and which agent is the default", as the boot effect, Settings ›
 * Agents and the agent room's menu share it. A pure mirror of
 * `AgentsLaunchVm` — which room opens, which agents may be chosen, why the
 * default does not open and which file decides it are all Rust's — plus the
 * last failed read's and save's sentences, shown where the person is looking.
 *
 * Ordered (R397, R409): every launch, read and save takes a sequence number
 * when it is issued, and an answer — success or failure — older than the
 * newest one applied is dropped, so a launch or a Settings read issued before
 * a save can never replace the saved choice, nor an old failure a newer
 * answer. A launch or read asked for while a save is out waits for it and is
 * issued after it: Rust may still read the old value until the save commits,
 * so a read that overlapped the save could not be trusted either way round.
 *
 * `navRestore` is the phone's half of the launch rule: the phone applies it
 * only when its restored navigation (Story 14.4) had nothing to restore, so
 * the boot effect waits for `use-nav-state-persistence` to say which.
 *
 * `chooserOpen` is the launch's "Choose the agent keeper opens when it
 * starts" (story 102.7, R463): opened by the launch when Rust says so
 * (`AgentsLaunchVm.chooser`), closed by a choice or by putting it away —
 * which Rust records so it is not offered again until keeper is updated.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import { reportFrontendError } from "@/lib/crash-report";
import {
  type AgentsLaunchVm,
  agentsDefaultChooserDismiss,
  agentsDefaultGet,
  agentsDefaultSet,
  agentsLaunch,
} from "@/lib/ipc/client";

/** Whether the phone's restored navigation put something on screen. */
export type NavRestore = "pending" | "restored" | "nothing";

export interface DefaultAgentState {
  /** Rust's latest answer; `null` before the first one. */
  vm: AgentsLaunchVm | null;
  /** The last failed read's sentence; `null` once a read succeeds. */
  readError: string | null;
  /** The last failed save's sentence; `null` once a save succeeds. */
  saveError: string | null;
  /** A save is in flight. */
  saving: boolean;
  navRestore: NavRestore;
  /** What keeper opens at launch: mirrored here, and returned to apply. */
  launch: () => Promise<AgentsLaunchVm>;
  /** Read the answer again. */
  load: () => Promise<void>;
  /** Make `agent` the default (`null`: none); the new answer, or `null` when not saved. */
  choose: (agent: string | null) => Promise<AgentsLaunchVm | null>;
  setNavRestore: (navRestore: NavRestore) => void;
  /** The launch's chooser is on screen in the Agents window. */
  chooserOpen: boolean;
  /** Show the launch's chooser (the launch, when Rust says so). */
  openChooser: () => void;
  /** Take the chooser off screen for this session (a choice was made). */
  closeChooser: () => void;
  /**
   * Put the chooser away until keeper is updated; `null` when Rust stored
   * it, else the failure's sentence (also reported to `keeper.log`) — the
   * chooser then leaves the screen for this session all the same.
   */
  dismissChooser: () => Promise<string | null>;
}

/** Rust's sentence from a rejected command, else a generic one. */
export function sentenceOf(error: unknown): string {
  if (typeof error === "object" && error !== null && "message" in error) {
    const { message } = error as { message: unknown };
    if (typeof message === "string" && message.trim() !== "") {
      return message;
    }
  }
  return "keeper could not read or save the default agent. Try again.";
}

/** The sentence when the default agent could not be read. */
export function readFailedSentence(error: unknown): string {
  return `keeper could not read the default agent: ${sentenceOf(error)}`;
}

export const defaultAgentStore = createStore<DefaultAgentState>()((set) => {
  // The newest request issued, and the newest whose answer was applied.
  let issued = 0;
  let applied = 0;
  // Saves out, and a promise settled once every save issued so far has.
  let saving = 0;
  let saved: Promise<void> = Promise.resolve();
  /** Apply `patch` unless a newer answer already was. */
  const apply = (seq: number, patch: Partial<DefaultAgentState>) => {
    if (seq >= applied) {
      applied = seq;
      set(patch);
    }
  };
  /**
   * The next sequence number — at once when no save is out (an async
   * function runs synchronously up to its first `await`), else once every
   * save has settled.
   */
  const issue = async (): Promise<number> => {
    while (saving > 0) {
      await saved;
    }
    return ++issued;
  };
  return {
    vm: null,
    readError: null,
    saveError: null,
    saving: false,
    navRestore: "pending",
    launch: async () => {
      const seq = await issue();
      const vm = await agentsLaunch();
      apply(seq, { vm, readError: null });
      return vm;
    },
    load: async () => {
      const seq = await issue();
      try {
        apply(seq, { vm: await agentsDefaultGet(), readError: null });
      } catch (error) {
        // Rust logs nothing for a failed read; the log line is this one —
        // written even when a newer answer keeps the failure off screen.
        reportFrontendError("agents.default.read", error, null);
        apply(seq, { readError: readFailedSentence(error) });
      }
    },
    choose: async (agent) => {
      const seq = ++issued;
      const request = agentsDefaultSet(agent);
      saving += 1;
      // A barrier, not a handler: the rejection is reported below.
      saved = Promise.allSettled([saved, request]).then(() => {});
      set({ saving: true });
      try {
        const vm = await request;
        apply(seq, { vm, saveError: null });
        return vm;
      } catch (error) {
        // Rust wrote a WARN line for every failure it saw; a rejection it
        // never saw (the bridge itself) reaches `keeper.log` only from here.
        reportFrontendError("agents.default.save", error, null);
        apply(seq, { saveError: sentenceOf(error) });
        return null;
      } finally {
        saving -= 1;
        set({ saving: saving > 0 });
      }
    },
    setNavRestore: (navRestore) => set({ navRestore }),
    chooserOpen: false,
    openChooser: () => set({ chooserOpen: true }),
    closeChooser: () => set({ chooserOpen: false }),
    dismissChooser: async () => {
      set({ chooserOpen: false });
      try {
        await agentsDefaultChooserDismiss();
        return null;
      } catch (error) {
        // Rust wrote a WARN line for a failed write; a rejection it never
        // saw (the bridge itself) reaches `keeper.log` only from here.
        reportFrontendError("agents.default.chooser", error, null);
        return `keeper could not remember that you put this away, so it may ask again next time: ${sentenceOf(error)}`;
      }
    },
  };
});

/** React selector hook over {@link defaultAgentStore}. */
export function useDefaultAgentStore<T>(selector: (state: DefaultAgentState) => T): T {
  return useStore(defaultAgentStore, selector);
}
