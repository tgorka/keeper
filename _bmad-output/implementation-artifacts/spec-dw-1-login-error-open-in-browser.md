---
title: 'Route login-error external links through the opener plugin'
type: 'bugfix'
created: '2026-07-06'
status: 'done'
baseline_revision: 'eca58ddf0c63c68ba522f6d4b6e870d03c4d6b56'
final_revision: '4a35b55b6857ac587fd9bc78417dc204cd77f6b5'
review_loop_iteration: 0
followup_review_recommended: false
context: []
warnings: []
---

<intent-contract>

## Intent

**Problem:** The two external links in the login screen — the non-SSS error's "Learn more about Simplified Sliding Sync" and the Beeper-failure "Check Beeper status" — are plain `<a target="_blank" rel="noreferrer">`. Inside a Tauri WKWebView `target="_blank"` is unreliable: it commonly does nothing or navigates the webview itself instead of opening the user's default browser, so the links may be dead in the shipped app (DW-1).

**Approach:** Keep the anchors (they preserve semantics, the visible `href`, and the existing "link" role the tests assert), but add an `onClick` that calls `event.preventDefault()` and routes the URL through the bundled opener plugin via `openUrl` from `@tauri-apps/plugin-opener`. This deterministically hands the URL to the system default browser.

## Boundaries & Constraints

**Always:**
- Use `openUrl` from `@tauri-apps/plugin-opener` (already a dependency; `opener:default` capability already grants `allow-open-url` + `allow-default-urls`).
- Keep the `href` attribute on both anchors so the destination stays visible/copyable and existing role/href assertions keep passing.
- `event.preventDefault()` before calling `openUrl` so the webview never follows the anchor itself.
- The `openUrl` call is fire-and-forget; swallow any rejection (best-effort, mirrors the existing `void …catch(() => {})` pattern in this file) so a click never throws an unhandled rejection.

**Block If:** (none — self-contained UI wiring)

**Never:**
- Do not add a new Rust IPC command or touch `src-tauri` — the frontend `openUrl` plugin binding is sufficient and already permitted.
- Do not change the other `target="_blank"` links in `bridges-pane.tsx`, `bbctl-panel.tsx`, or `first-run-wizard.tsx` — out of scope for DW-1.
- Do not remove the anchors or change them to buttons; do not change URLs or link text.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| SSS link click | slidingSyncUnsupported error shown, user clicks "Learn more about Simplified Sliding Sync" | `event.preventDefault()` runs; `openUrl(SSS_DOC_URL)` called exactly once; webview does not navigate | `openUrl` rejection is swallowed (no throw) |
| Beeper status link click | Beeper failure state shown, user clicks "Check Beeper status" | `event.preventDefault()` runs; `openUrl(BEEPER_STATUS_URL)` called exactly once; webview does not navigate | `openUrl` rejection is swallowed (no throw) |

</intent-contract>

## Code Map

- `src/components/auth/login-screen.tsx` -- the two anchors: SSS doc link (~line 294) and Beeper status link (~line 442). Add `import { openUrl }` and an `onClick` handler.
- `src/components/auth/login-screen.test.tsx` -- existing tests assert the links' role/href; extend with click-routes-through-opener assertions. Add an `@tauri-apps/plugin-opener` mock.
- `src-tauri/crates/keeper/capabilities/default.json` -- reference only: already lists `opener:default` (no edit).

## Tasks & Acceptance

**Execution:**
- [x] `src/components/auth/login-screen.tsx` -- add `import { openUrl } from "@tauri-apps/plugin-opener";`; add a small `openExternal(url)` helper (or inline `onClick`) that calls `event.preventDefault()` then `void openUrl(url).catch(() => {})`; wire it onto both anchors while keeping their `href`, text, and `rel="noreferrer"`.
- [x] `src/components/auth/login-screen.test.tsx` -- mock `@tauri-apps/plugin-opener` with a spy `openUrl`; add tests that clicking each link calls `openUrl` with the correct URL and that `preventDefault` is honored (default not triggered). Keep the existing role/href assertions.

**Acceptance Criteria:**
- Given the slidingSyncUnsupported error is displayed, when the user clicks "Learn more about Simplified Sliding Sync", then `openUrl` is called once with `SSS_DOC_URL` and the anchor's default navigation is prevented.
- Given the Beeper failure state is displayed, when the user clicks "Check Beeper status", then `openUrl` is called once with `BEEPER_STATUS_URL` and the anchor's default navigation is prevented.
- Given either link, when rendered, then it still exposes the `link` role with its `href` attribute intact (existing assertions unbroken).
- Given `bun run check` runs, then biome lint, tsc, and vitest all pass.

## Spec Change Log

_No bad_spec loopbacks — empty._

## Review Triage Log

### 2026-07-06 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 0, medium 1, low 0)
- defer: 1
- reject: 8
- addressed_findings:
  - `[medium]` `[patch]` Dropping `target="_blank"` while keeping `href` let non-`onClick` activation paths (middle-click, aux-click, handler-not-firing) fall through to the raw `href`, which a WKWebView follows in-place and unmounts the login SPA. Re-added `target="_blank"` to both anchors (spec-sanctioned fallback: the fall-through becomes a harmless webview no-op/new-window attempt instead of destructive in-place navigation) and documented the intent in the `openExternal` doc comment. `bun run check` re-run green (949 tests). Rejected: silent `.catch` swallow (spec-mandated "Always"), unreachable empty-URL guard (URLs are module constants), keyboard-Enter gap (Enter synthesizes a preventable click through `onClick`), vestigial `rel`/`noopener` hygiene, and test-assertion-style nits.

## Design Notes

Anchor-with-onClick (not a bare button) keeps the URL visible in copy/hover and preserves the accessible `link` role the current tests rely on. The handler shape mirrors the file's existing best-effort async pattern (`void cancelOidc().catch(() => {})`): fire-and-forget, errors swallowed, so a failed `openUrl` never surfaces an unhandled rejection during a login-error state.

Example handler:

```tsx
import { openUrl } from "@tauri-apps/plugin-opener";

function openExternal(event: MouseEvent<HTMLAnchorElement>, url: string) {
  event.preventDefault();
  void openUrl(url).catch(() => {
    // Best-effort: nothing actionable if the system opener fails.
  });
}
```

Usage: `<a href={SSS_DOC_URL} rel="noreferrer" onClick={(e) => openExternal(e, SSS_DOC_URL)}>`. `target="_blank"` may be dropped (the opener owns navigation now) or kept as a harmless fallback — dropping it is cleaner since the webview no longer handles the click.

## Verification

**Commands:**
- `bun run check` -- expected: biome lint + tsc typecheck + vitest all pass, including the new opener-routing tests.

**Manual checks:**
- The DW's original ask was a `tauri dev` confirmation that the link opens the system browser. That manual step is out of scope for this unattended run; the automated tests assert the opener plugin is invoked with the correct URL, which is the wiring the manual check was meant to verify.

## Auto Run Result

Status: done

**Summary:** DW-1 resolved. The two external links in the login screen — the non-SSS error's "Learn more about Simplified Sliding Sync" and the Beeper-failure "Check Beeper status" — now route through the bundled `@tauri-apps/plugin-opener` (`openUrl`) via an `onClick` that `preventDefault`s, so activation reliably opens the system default browser instead of relying on `<a target="_blank">`, which is unreliable inside a Tauri WKWebView. `href` + `target="_blank"` are retained as a safe fallback for non-`onClick` activation paths (`opener:default` capability already grants `allow-open-url`, so no backend/capability changes were needed).

**Files changed:**
- `src/components/auth/login-screen.tsx` — added `openUrl` import + `openExternal(event, url)` helper (preventDefault → best-effort `openUrl`), wired onto both anchors; kept `href`/`target="_blank"`/`rel`.
- `src/components/auth/login-screen.test.tsx` — mocked `@tauri-apps/plugin-opener`; added two tests asserting each link routes through `openUrl` with the correct URL and prevents default navigation.

**Review findings breakdown:**
- Patches applied (1): re-added `target="_blank"` to both anchors so non-`onClick` activation (middle-click, aux-click, handler-not-firing) falls back to a harmless webview no-op/new-window attempt instead of navigating the login SPA away in-place; documented the intent in the helper doc comment.
- Deferred (1) — NOT written to the ledger per orchestrator instruction; recorded here for the orchestrator: Promote external-link opening to a shared `openExternal` utility / `<ExternalLink>` component and migrate the other `target="_blank"` links (`src/components/layout/bridges-pane.tsx`, `src/components/bridges/bbctl-panel.tsx`, `src/components/wizard/first-run-wizard.tsx`), which share the same WKWebView reliability problem. Evidence: this is the first `openUrl` usage in the app, introduced as a component-local helper; three other links still use bare `target="_blank"`.
- Rejected (8): silent `.catch` swallow (spec-mandated "Always" in the intent-contract), unreachable empty/malformed-URL guard (URLs are module constants), keyboard-Enter "gap" (Enter synthesizes a preventable click through `onClick`), vestigial `rel`/`rel="noopener"` hygiene, `fireEvent.click === false` assertion-style nit, missing `.catch` failure-path test (branch is a spec-mandated no-op swallow), and unsupported-reliability-claim doc-comment nits.

**Verification:** `bun run check` — biome (`Checked 265 files, No fixes applied`) + `tsc --noEmit` clean + vitest `949 passed (949)` + tauri-free core check — all pass after the patch.

**Residual risks:** The DW's original ask included a manual `tauri dev` confirmation that the link opens the system browser; that manual step was out of scope for this unattended run. The automated tests assert the opener plugin is invoked with the correct URL (the wiring the manual check verifies), and the retained `target="_blank"` fallback bounds the worst case for any untested activation path.
