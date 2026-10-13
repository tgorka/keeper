/**
 * The one place the front writes a URL of keeper's own schemes (Epic 98, story
 * 98.3 #3; `ARCHITECTURE-SPINE.md`'s planned media-URL helper; codemap Q13).
 *
 * Tauri serves a custom protocol in one of two spellings, by operating system:
 * `<scheme>://<host>/<path>` on macOS, iOS and Linux, and
 * `http://<scheme>.localhost/<host>/<path>` on Android and Windows, whose
 * webviews cannot load a custom scheme. A URL in the other spelling is a request
 * nothing answers — an image that never loads, with nothing in any log. Which
 * spelling this build takes is Rust's fact (`keeper_core::asset_url::UrlForm::HOST`),
 * read here from the platform store, which starts from the facts the shell
 * writes into the webview before the bundle runs — so the first URL is already
 * in the right spelling — never a user-agent sniff. Rust's own composer
 * (`asset_url::compose`) is the same function on the other side; `media_url`
 * uses it for `keeper-media://`, which therefore arrives from Rust already
 * spelled.
 *
 * Every custom-scheme `src` the front composes goes through {@link mediaSrc};
 * `media-src.test.ts` scans `src/` and fails on a component that spells one
 * itself.
 */
import type { UrlForm } from "@/lib/ipc/client";
import { platformStore } from "@/lib/stores/platform";

/** keeper's custom URI schemes whose URLs the front composes. */
export type KeeperScheme = "keeper-file" | "keeper-note" | "keeper-recording";

/**
 * The URL for `rest` — `<host>/<path>`, each segment already percent-encoded by
 * the caller — under `scheme`, in `form` (this build's, from Rust, by default).
 * Adds no encoding and checks nothing: the handler that serves the URL is the
 * one that refuses.
 */
export function mediaSrc(
  scheme: KeeperScheme,
  rest: string,
  form: UrlForm = platformStore.getState().urlForm,
): string {
  return form === "localhost" ? `http://${scheme}.localhost/${rest}` : `${scheme}://${rest}`;
}
