/**
 * The voice pill window's entry point (Epic 64, Story 64.4, AD-185).
 *
 * The smallest document in the repo, and it should stay that way: one
 * listener, one component, no store, no router, no IPC command. The window
 * is created hidden at boot by `voice_window.rs` when — and only when —
 * voice is a real answer on this desktop, and it is never destroyed, so
 * this mounts once with nobody waiting and every later snapshot is one
 * event into a document that is already live.
 *
 * It asks Rust for nothing. `voice_ipc::push` emits every snapshot to this
 * window under `VOICE_STATE_EVENT` before it streams it to the pane, so the
 * pill and the pane always say the same thing, and the pill holds no watch
 * id that an unmount would have to give back. The window's capability
 * grants `core:default` and nothing else, and this file is why that is
 * enough.
 */
import { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { VoicePill } from "@/components/voice/voice-pill";
import { listenVoiceState, type VoiceStateVm } from "@/lib/ipc/client";
import "./index.css";

export function VoiceWindow() {
  const [state, setState] = useState<VoiceStateVm | null>(null);
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let gone = false;
    void listenVoiceState(setState)
      .then((stop) => {
        if (gone) {
          stop();
        } else {
          unlisten = stop;
        }
      })
      // Outside a Tauri webview (jsdom, `bun run dev`) there is nothing to
      // listen to and nothing to show; the pill sits empty rather than
      // throwing.
      .catch(() => {});
    return () => {
      gone = true;
      unlisten?.();
    };
  }, []);
  return <VoicePill state={state} />;
}

// Guarded so the module can be imported by a test without mounting a root.
const container = document.getElementById("root");
if (container) {
  ReactDOM.createRoot(container).render(<VoiceWindow />);
}
