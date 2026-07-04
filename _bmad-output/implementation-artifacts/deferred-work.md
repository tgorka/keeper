# Deferred Work

Items surfaced during review that are out of scope for their originating story.

- source_spec: `_bmad-output/implementation-artifacts/spec-1-3-password-login-with-sliding-sync-verification.md`
  summary: The non-SSS error's "Learn more about Simplified Sliding Sync" link uses a plain `<a target="_blank" rel="noreferrer">`, whose open-in-system-browser behavior inside the Tauri webview is unverified.
  evidence: In a Tauri WKWebView, `target="_blank"` often does nothing or navigates the webview rather than opening the default browser; the app bundles `tauri_plugin_opener` but the link does not route through it. The component test only asserts the `href` attribute exists, not that activation opens the browser. Needs a manual `tauri dev` check and, if broken, wiring the link through the opener plugin.
- source_spec: `_bmad-output/implementation-artifacts/spec-1-3-password-login-with-sliding-sync-verification.md`
  summary: The `unsupportedLoginType` classification is unreliable — a homeserver with password login disabled typically returns `M_FORBIDDEN`, which `auth::map_login_error` maps to `InvalidCredentials`, so the user sees "Wrong username or password" instead of "password login not supported."
  evidence: `matrix_auth().login_username()` does not pre-fetch the `/login` flow list, and Synapse returns `M_FORBIDDEN` for a password-login-disabled server (same errcode as a wrong password). The spec's error-kind mapping (`Forbidden`→`InvalidCredentials`, `Unrecognized`/`InvalidParam`→`UnsupportedLoginType`) therefore cannot reliably satisfy the I/O-matrix row for "unsupported login type." A robust fix is a pre-login `matrix_auth().login_types()` / supported-flows check to detect `m.login.password` before attempting login. Low user impact (uncommon scenario, both outcomes are non-retriable password-login failures); deferred rather than re-deriving the flow — the spec consciously chose error-kind mapping and did not mandate a flow pre-check.
