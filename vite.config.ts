import path from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const host = process.env.TAURI_DEV_HOST;

// A viewing aid for the frontend, no Rust required — see `dev/mock-shell.ts`
// for what it serves and what it deliberately does not.
//
// This injects a fake IPC shell into the DEV SERVER ONLY (`apply: "serve"`), so
// it cannot reach a production bundle by any path. It lives outside `src/`
// deliberately: platform behaviour inside `src/` must come from the Rust
// capabilities handshake and never from a build flag, and the
// `no-user-agent-gating` guard enforces exactly that. The module itself also
// declines to install whenever a real shell is present, so `tauri dev` is never
// quietly served fixtures.
const mockShell = {
  name: "keeper-dev-mock-shell",
  apply: "serve" as const,
  transformIndexHtml: () => [
    {
      tag: "script",
      attrs: { type: "module" },
      children: 'import { installMockShell } from "/dev/mock-shell.ts"; installMockShell();',
      injectTo: "head" as const,
    },
  ],
};

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react(), tailwindcss(), mockShell],

  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },

  // Four entry points, not one (AD-60, AD-185). `index.html` is the app;
  // `capture.html` is the quick-capture panel's own document, loaded by the
  // statically declared `quick-capture` window; `voice.html` is the voice
  // pill's, loaded by the window `voice_window.rs` creates when voice is a
  // real answer. Keeping them separate is what keeps the NFR-27 budget honest:
  // the capture bundle imports neither the editor nor mermaid, so the panel
  // paints without touching either lazy chunk, and the pill imports one
  // component and one listener.
  build: {
    rollupOptions: {
      input: {
        main: path.resolve(__dirname, "index.html"),
        capture: path.resolve(__dirname, "capture.html"),
        print: path.resolve(__dirname, "print.html"),
        voice: path.resolve(__dirname, "voice.html"),
      },
    },
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
