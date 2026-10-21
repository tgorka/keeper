import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import { type AgentViewPrefsVm, agentViewPrefsGet, agentViewPrefsSet } from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";

interface ViewerPreferences {
  value: AgentViewPrefsVm;
  loaded: boolean;
  busy: boolean;
  error: string | null;
}

/** One mirror across the window and dock; Rust stores only the changed preference. */
export const agentViewPrefsStore = createStore<ViewerPreferences>(() => ({
  value: { showReasoning: false, showDetails: false },
  loaded: false,
  busy: false,
  error: null,
}));

export function useAgentViewPrefs() {
  return useStore(agentViewPrefsStore);
}

export async function loadAgentViewPrefs() {
  const current = agentViewPrefsStore.getState();
  if (current.loaded || current.busy) return;
  agentViewPrefsStore.setState({ busy: true, error: null });
  try {
    const value = await agentViewPrefsGet();
    agentViewPrefsStore.setState({ value, loaded: true });
  } catch (raw) {
    agentViewPrefsStore.setState({
      error: syncErrorMessage(raw, "keeper could not read your answer preferences."),
    });
  } finally {
    agentViewPrefsStore.setState({ busy: false });
  }
}

export async function setAgentViewPrefs(patch: Partial<AgentViewPrefsVm>) {
  const current = agentViewPrefsStore.getState();
  // A click cannot race hydration or another preference write.
  if (!current.loaded || current.busy) return;
  agentViewPrefsStore.setState({ busy: true, error: null });
  try {
    const value = await agentViewPrefsSet(patch);
    agentViewPrefsStore.setState({ value });
  } catch (raw) {
    agentViewPrefsStore.setState({
      error: syncErrorMessage(raw, "keeper could not save your answer preference."),
    });
  } finally {
    agentViewPrefsStore.setState({ busy: false });
  }
}
