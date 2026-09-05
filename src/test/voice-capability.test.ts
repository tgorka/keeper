/**
 * The voice pill window's wiring, checked on every machine (Story 64.4,
 * AD-185).
 *
 * `voice_window.rs` names four things that other files must spell the same
 * way, and the `keeper` shell crate does not compile on Linux (AD-55/AD-56),
 * so the Rust side of each pair is prose here unless a test reads it. The
 * pattern is `capture-capability.test.ts` and `print-capability.test.ts`.
 *
 * - The **label**, which `capabilities/voice.json` scopes its grant to. A
 *   window whose label matches no capability renders and can invoke nothing —
 *   here, it can `listen` to nothing, and a pill that never updates is a
 *   turn that reads as stalled.
 * - The **event**, emitted in Rust and listened for in `client.ts`; a
 *   `listen` on a name nothing emits fires never and logs nothing.
 * - The **document**, which Vite must build as an entry and Rust must load.
 * - The **absence** of a static declaration: the window is created by
 *   `voice_window::install` only when voice is a real answer (AD-27,
 *   AD-179), and a `voice` entry in `tauri.conf.json` would create it on
 *   every desktop, including the ones whose port cannot listen.
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const ROOT = resolve(import.meta.dirname, "../..");

function file(relative: string): string {
  return readFileSync(resolve(ROOT, relative), "utf8");
}

const VOICE_WINDOW_RS = file("src-tauri/crates/keeper/src/voice_window.rs");
const VOICE_IPC_RS = file("src-tauri/crates/keeper/src/voice_ipc.rs");
const LIB_RS = file("src-tauri/crates/keeper/src/lib.rs");
const CLIENT_TS = file("src/lib/ipc/client.ts");
const VITE_CONFIG = file("vite.config.ts");
const VOICE_MAIN = file("src/voice-main.tsx");
const BOTS_IPC_RS = file("src-tauri/crates/keeper/src/bots_ipc.rs");
const BOT_VOICE_MIC_TSX = file("src/components/bots/bot-voice-mic.tsx");
const BOT_VOICE_WAKE_TSX = file("src/components/bots/bot-voice-wake.tsx");
const BOT_VOICE_TARGET_TSX = file("src/components/bots/bot-voice-target.tsx");

const capability = JSON.parse(file("src-tauri/crates/keeper/capabilities/voice.json")) as {
  windows: string[];
  platforms: string[];
  permissions: string[];
};

const conf = JSON.parse(file("src-tauri/crates/keeper/tauri.conf.json")) as {
  app: { windows: { label: string; url?: string }[] };
};

const iosConf = JSON.parse(file("src-tauri/crates/keeper/tauri.ios.conf.json")) as {
  app: { windows: { label: string }[] };
};

const label = /pub const VOICE_WINDOW_LABEL: &str = "([^"]+)";/.exec(VOICE_WINDOW_RS)?.[1];
const document = /const VOICE_DOCUMENT: &str = "([^"]+)";/.exec(VOICE_WINDOW_RS)?.[1];

describe("the voice pill window", () => {
  it("is created by Rust when voice is a real answer, never declared statically", () => {
    expect(label, "voice_window.rs no longer names its label").toBeDefined();
    expect(conf.app.windows.map((window) => window.label)).not.toContain(label);
    expect(iosConf.app.windows.map((window) => window.label)).toEqual(["main"]);
    // The gate is the one every reach surface reads (AD-179), read before
    // the builder runs.
    expect(VOICE_WINDOW_RS).toMatch(
      /if !crate::voice_reach::present\(\) \{[\s\S]*?return;[\s\S]*?WebviewWindowBuilder::new/,
    );
    expect(LIB_RS).toContain("voice_window::install(app.handle());");
  });

  it("is never key, never focused, always on top, on every Space, and click-through", () => {
    // AD-185's builder flags, asserted as text because this crate does not
    // compile here. `transparent` is asserted ABSENT: it needs
    // `macOSPrivateApi`, which keeper does not enable.
    for (const flag of [
      ".decorations(false)",
      ".always_on_top(true)",
      ".visible_on_all_workspaces(true)",
      ".focusable(false)",
      ".focused(false)",
      ".visible(false)",
      "set_ignore_cursor_events(true)",
    ]) {
      expect(VOICE_WINDOW_RS, `voice_window.rs no longer sets ${flag}`).toContain(flag);
    }
    expect(VOICE_WINDOW_RS).not.toContain(".transparent(");
  });

  it("is covered by its capability under its exact label, with core:default only", () => {
    expect(capability.windows).toEqual([label]);
    expect(capability.permissions).toEqual(["core:default"]);
    expect(capability.platforms).not.toContain("iOS");
  });

  it("emits and listens for the snapshot under one event name", () => {
    const emitted = /pub const VOICE_STATE_EVENT: &str = "([^"]+)";/.exec(VOICE_WINDOW_RS)?.[1];
    const listened = /export const VOICE_STATE_EVENT = "([^"]+)";/.exec(CLIENT_TS)?.[1];
    expect(emitted).toBeDefined();
    expect(listened).toBe(emitted);
    // To that one window, not app-wide: the main window keeps its channel
    // and must not be told twice.
    expect(VOICE_WINDOW_RS).toContain("emit_to(VOICE_WINDOW_LABEL, VOICE_STATE_EVENT");
    expect(VOICE_MAIN).toContain("listenVoiceState(");
  });

  it("is fed from push, before the pane's channel, without a second watcher", () => {
    expect(VOICE_IPC_RS).toMatch(
      /fn push\(voice: &mut Voice\) \{[\s\S]*?crate::voice_window::observe\(&snapshot\);[\s\S]*?voice\.watcher/,
    );
    // The pill invokes nothing: no `voice_watch` from its document.
    expect(VOICE_MAIN).not.toContain("voiceWatch");
    expect(VOICE_MAIN).not.toContain("invoke(");
  });

  it("loads a document Vite builds", () => {
    expect(document).toBeDefined();
    expect(VITE_CONFIG).toContain(`path.resolve(__dirname, "${document}")`);
    expect(file(document ?? "")).toContain('src="/src/voice-main.tsx"');
  });
});

/**
 * Epic 67 (AD-205): the hands-free turn is Rust's from the phrase to the
 * last word. The shell performs `SendText` and drives `Speak`; the webview
 * observes the stream through one forwarded event and drives nothing.
 */
describe("the spoken turn (Epic 67, AD-205)", () => {
  it("forwards the spoken stream under one event name, emitted in Rust and listened for in client.ts", () => {
    const emitted = /pub const SPOKEN_STREAM_EVENT: &str = "([^"]+)";/.exec(BOTS_IPC_RS)?.[1];
    const listened = /export const BOTS_SPOKEN_STREAM_EVENT = "([^"]+)";/.exec(CLIENT_TS)?.[1];
    expect(emitted).toBeDefined();
    expect(listened).toBe(emitted);
    expect(BOTS_IPC_RS).toContain("emit(SPOKEN_STREAM_EVENT, &event)");
  });

  it("performs SendText in the shell and drives Speak from the stream's close", () => {
    expect(VOICE_IPC_RS).toMatch(/Effect::SendText\(text\) => Some\(text\)/);
    expect(VOICE_IPC_RS).toContain("crate::bots_ipc::send_spoken(&app, text).await");
    expect(BOTS_IPC_RS).toContain("crate::voice_ipc::answer_complete(content.to_owned())");
    // The command the webview used to speak with is gone on both sides.
    expect(LIB_RS).not.toContain("voice_ipc::voice_speak");
    expect(CLIENT_TS).not.toContain('"voice_speak"');
  });

  it("has no voice component that sends a message or reads an answer aloud", () => {
    for (const source of [BOT_VOICE_MIC_TSX, BOT_VOICE_WAKE_TSX, BOT_VOICE_TARGET_TSX]) {
      expect(source).not.toContain("botsChatSend");
      expect(source).not.toContain("voiceSpeak");
    }
  });
});
