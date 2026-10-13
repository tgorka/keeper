/**
 * Platform-facts mirror store (Epic 98, story 98.3 #3, #4, #5).
 *
 * Holds the Rust-authored {@link PlatformVm}: the spelling keeper's
 * custom-scheme URLs take on this build, whether the device is a tablet, and
 * the phone's or tablet's "On this …" disclosure. The front composes no
 * platform fact of its own — no user agent, no build flag (the
 * `no-user-agent-gating` convention test), no viewport — it reads this mirror.
 *
 * Rust writes the facts into the webview before the bundle runs (the shell's
 * `ipc::platform_plugin` initialization script sets `window.__KEEPER_PLATFORM__`),
 * so the store starts from them and no frame, URL or layout is ever composed
 * from a guess. Every fact is fixed for the process; nothing asks again.
 * Without the script (the test runner, a page the shell did not load) the
 * store starts at the Apple spelling, no tablet and no disclosure; the dev mock
 * shell applies its own facts before the app mounts.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { PlatformDisclosureVm, PlatformVm, UrlForm } from "@/lib/ipc/client";

declare global {
  interface Window {
    /** The shell's platform facts, written before the bundle runs. */
    __KEEPER_PLATFORM__?: PlatformVm;
  }
}

export interface PlatformState {
  /** The spelling of keeper's custom-scheme URLs on this build. */
  urlForm: UrlForm;
  /** This device is a tablet by Android's smallest-width rule — never the viewport's. */
  tablet: boolean;
  /** The reduced build's disclosure; `null` on a desktop. */
  disclosure: PlatformDisclosureVm | null;
  /** Replace the mirror from a {@link PlatformVm}. */
  applySnapshot: (vm: PlatformVm) => void;
}

const BEFORE_RUST: PlatformVm = { urlForm: "scheme", tablet: false, disclosure: null };

export const platformStore = createStore<PlatformState>()((set) => {
  // The facts Rust wrote into this webview, read once as the store is created.
  const { urlForm, tablet, disclosure } =
    (typeof window !== "undefined" && window.__KEEPER_PLATFORM__) || BEFORE_RUST;
  return {
    urlForm,
    tablet,
    disclosure,
    applySnapshot: (vm) =>
      set({ urlForm: vm.urlForm, tablet: vm.tablet, disclosure: vm.disclosure }),
  };
});

/** React selector hook over {@link platformStore}. */
export function usePlatformStore<T>(selector: (state: PlatformState) => T): T {
  return useStore(platformStore, selector);
}
