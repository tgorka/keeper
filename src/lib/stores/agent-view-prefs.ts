import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import { type AgentViewPrefsVm, agentViewPrefsSet } from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";

/**
 * A preference write in flight and how the last one ended. What the
 * preferences ARE is never kept here: each open session's view carries them
 * as Rust resolves them (`AgentSessionViewVm.prefs`), whichever device or
 * settings file changed them last.
 */
interface PreferenceWrite {
  busy: boolean;
  error: string | null;
  /** The last write was saved, but a settings file keeps the preference as it is. */
  shadowed: boolean;
}

/** One across the window and dock, so a click cannot race another write. */
export const agentViewPrefsWriteStore = createStore<PreferenceWrite>(() => ({
  busy: false,
  error: null,
  shadowed: false,
}));

export function useAgentViewPrefsWrite() {
  return useStore(agentViewPrefsWriteStore);
}

export async function setAgentViewPrefs(patch: Partial<AgentViewPrefsVm>) {
  if (agentViewPrefsWriteStore.getState().busy) return;
  agentViewPrefsWriteStore.setState({ busy: true, error: null, shadowed: false });
  try {
    const held = await agentViewPrefsSet(patch);
    // What holds is the effective answer; a settings file's pin wins.
    const keys = Object.keys(patch) as (keyof AgentViewPrefsVm)[];
    agentViewPrefsWriteStore.setState({
      shadowed: keys.some((key) => patch[key] !== undefined && held[key] !== patch[key]),
    });
  } catch (raw) {
    agentViewPrefsWriteStore.setState({
      error: syncErrorMessage(raw, "keeper could not save your answer preference."),
    });
  } finally {
    agentViewPrefsWriteStore.setState({ busy: false });
  }
}
