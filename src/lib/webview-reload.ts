/**
 * The one full-document reload seam (Story 14.4).
 *
 * The blank-webview guard ({@link useWebviewGuard}) recovers a jettisoned/frozen
 * WKWebView by reloading the document; `window.location.reload` itself is
 * unforgeable in the DOM (and in jsdom), so the call lives behind this one-line
 * module seam — production behavior is a plain reload, and tests mock the module
 * to observe the (loop-guarded) trigger without navigating the test document.
 */
export function reloadWebview(): void {
  window.location.reload();
}
