---
title: 'Give the window back its 96 px, and let the drawer reach its own bottom'
type: 'bugfix'
created: '2026-07-28'
status: 'review'
review_loop_iteration: 0
baseline_revision: '5c40a22'
followup_review_recommended: true
context: []
warnings:
  - 'The 56 px webview overhang is NOT fixed by this story. Research (a) established it is an
    upstream WebKit/AppKit defect that no `tauri.conf.json` key and no available dependency
    version can reach. The epic''s proposed fallback (drop `titleBarStyle`/`hiddenTitle`) is
    rejected here with source citations because it would make the overhang by-design rather
    than anomalous. Escalation path and the runtime discriminator are in Design Notes.'
---

<intent-contract>

## Intent

**Problem:** Three independent defects stack at the top and bottom of the window.

1. **56 px of webview overhang.** Measured on hesperia (macOS 26.5, keeper 0.6.2): the window is
   `(200, 200) 1280x800`, and `HTML content` is `(200, 256) 1280x800` — offset 56 px down yet still
   a full 800 tall, so it overhangs the window bottom by 56 px. `100vh` is therefore 56 px taller
   than the visible area and everything bottom-anchored is pushed out of sight. The `Add account`
   button reports y=1012 against a window bottom of y=1000.
2. **Two elements pay for one inset.** The 28 px band at `app-shell.tsx:141` and a second `pt-3
   pl-[78px]` at `sidebar-pane.tsx:106` both reserve room for the same 78x12 px of traffic lights.
   The band is also a single full-width `bg-background` strip sitting above a `bg-sidebar` drawer,
   which is a seam in light mode and a black bar in dark — the reported "black strip".
3. **The drawer cannot reach its own bottom.** `sidebar-pane.tsx`'s `<nav>` was the only pane in the
   layout with neither `min-h-0` nor a scroll container, while its root is `h-screen
   overflow-hidden`. Eight Spaces in a 600 px window overflow the pane on their own and push the
   `mt-auto` footer past the clip, with no scrollbar to recover it.

**Approach:** Defect 1 is diagnosed, not patched: research established that `titleBarStyle:
"Overlay"` is *already* the correct and safest configuration, and that the overhang is WKWebView's
automatic top obscured content inset — a WebKit-computed value that no config key reaches (full
evidence chain and the rejection of the epic's fallback are in Design Notes). `tauri.conf.json` is
therefore left untouched. Defect 2 is fixed by replacing the full-width band with **one** band
painted per column (`bg-sidebar` across the drawer's width, `bg-background` across the rest, both
carrying `data-tauri-drag-region`), rendered only where the platform floats window controls over
the webview, and by deleting the sidebar's duplicate inset. That platform fact enters through a new
`CapabilitiesVm.overlayTitleBar` flag — the existing single source of platform truth — never a
user-agent sniff. Defect 3 is fixed by pairing the `<nav>`'s `min-h-0` with the `ScrollArea
min-h-0 flex-1` pattern every other pane already uses, with the footer left outside the scroller.

## Boundaries & Constraints

**Always:**
- Exactly **one** element reserves the window-control inset, and it is the drag region (AD-34-2).
- The band is painted **per column** so each column matches the pane beneath it (AD-34-3); both
  columns carry `data-tauri-drag-region` so the whole band moves the window.
- The band renders **only** where the platform floats window controls over the webview. That fact
  comes from `useCapabilitiesStore` reading the Rust-authored `CapabilitiesVm`, so the existing
  `src/test/no-user-agent-gating.test.ts` convention test stays green (AD-35).
- Any pane whose content grows with user data pairs `min-h-0` with a scroll container, and the
  `mt-auto` footer stays **outside** that container so it cannot be scrolled away (AD-34-4).
- `keeper-core` stays free of `cfg(target_os)` (AD-26): the new field is a plain `bool` on the VM;
  the `cfg!` that populates it lives in the shell crate's `capabilities()` command.
- The hand-maintained `src/lib/ipc/gen/CapabilitiesVm.ts` must stay byte-identical to ts-rs output
  so `bun run bindings:check` reports no drift.

**Block If:**
- A fix for defect 1 would require a second authorised `unsafe` FFI site. The repo declares
  `unsafe_code = "deny"` with a single audited exception recorded in
  `docs/constraints-and-limitations.md` under a dated coordinator policy amendment. The only
  reliable remedy for the obscured-content-inset defect is exactly that (see Design Notes), so it
  is escalated rather than taken unilaterally.

**Never:**
- **Never subtract a hard-coded native-chrome pixel constant to compensate for the 56 px.** No
  `calc(100vh - 56px)`, no padded `height` in `tauri.conf.json`, no magic offset anywhere. A
  hard-coded chrome constant is how this bug returns on the next macOS release, and the measured
  56 px matches no documented titlebar height on any macOS version.
- Do not set `titleBarStyle` to `Visible`/`Transparent` or drop the key. `Visible` is the value
  that *enables* the inset (see Design Notes); `Transparent` trades it for a titlebar strip painted
  in an uncontrollable window background colour.
- Do not set `decorations: false`. It does remove the inset, but by removing the native title bar
  and traffic lights entirely — a different product, far outside this story.
- Do not add a second platform-truth surface beside `CapabilitiesVm`, and do not reuse `recording`
  as a macOS proxy: `recording` is floored at macOS 13.0 while `minimumSystemVersion` is 11.0, so a
  macOS 11/12 user would lose the drag band and be left with an immovable window.
- Do not put the account footer inside the new scroller, and do not give the Spaces/Networks groups
  their own separate scrollers — the epic specifies one scroller over the views plus both groups.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Desktop macOS, expanded drawer | `overlayTitleBar: true`, `phone: false`, `sidebarCollapsed: false` | Two drag regions: `bg-sidebar w-[260px]`, then `bg-background flex-1` | n/a — pure render |
| Desktop macOS, collapsed rail | same but `sidebarCollapsed: true` | Drawer column narrows to `w-12`; band still two columns | n/a |
| Desktop macOS, phone tier | `overlayTitleBar: true`, `phone: true` (< 768 px) | One drag region, `bg-background` only — no drawer is rendered, so a `bg-sidebar` column would be the same seam mirrored | n/a |
| Linux / Windows desktop | `overlayTitleBar: false` | No band at all; content starts flush under the OS's real title bar | n/a |
| iOS | all flags `false` | No band; `isReducedCapabilityPlatform` still true (the new flag is `false` there, so the all-absent predicate is unaffected) | n/a |
| Pre-hydration | `DEFAULT_CAPABILITIES`, `hydrated: false` | No band — the safe default never advertises a surface the platform may lack | n/a |
| Eight Spaces at 600 px height | Spaces list overflows the drawer | Views + Spaces + Networks scroll inside the `ScrollArea`; the account footer stays pinned and reachable | n/a |
| No Spaces, no Networks | both groups return `null` | Scroller holds only the view list; footer still pinned at the bottom via `flex-1` + `mt-auto` | n/a |
| Phone leading drawer | `SidebarPane` inside a `SheetContent side="left"` (`inset-y-0 h-full flex flex-col`) | Same bounded-height + scroller behaviour; the drawer's footer becomes reachable too | n/a |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/vm.rs:92-131` -- `CapabilitiesVm`; new `pub overlay_title_bar:
  bool` appended after `sync`, doc-commented with the coupling to the two config keys.
- `src-tauri/crates/keeper/src/ipc.rs:1286-1310` -- `capabilities()`; populates the new flag as
  `cfg!(all(desktop, target_os = "macos"))`, the shell being the platform adapter layer.
- `src/lib/ipc/gen/CapabilitiesVm.ts` -- GENERATED by ts-rs; extended to match the Rust struct
  exactly (verified, see Verification).
- `src/lib/stores/capabilities.ts:24-35` -- `DEFAULT_CAPABILITIES`; `overlayTitleBar: false` safe
  default. `isReducedCapabilityPlatform` needs no change: it already derives from `Object.values`.
- `src/components/layout/app-shell.tsx` -- reads `overlayTitleBar`; the full-width band at the old
  `:141` becomes the gated two-column band; imports `SIDEBAR_WIDTH_CLASS` and `cn`.
- `src/components/layout/sidebar-pane.tsx` -- new exported `SIDEBAR_WIDTH_CLASS`; `<nav>` gains
  `min-h-0` and consumes that constant; the duplicate traffic-light inset is deleted; the view list
  plus `SpacesGroup` plus `NetworksGroup` move inside `<ScrollArea className="min-h-0 flex-1">`.
- `src/components/ui/scroll-area.tsx` -- unchanged; the `data-slot="scroll-area{,-viewport}"` hooks
  are what the new tests assert against.
- `src/components/layout/{app-shell,sidebar-pane}.test.tsx` -- new coverage.
- 12 `DESKTOP_CAPABILITIES` fixtures + `src/lib/stores/capabilities.test.ts` -- extended for the new
  field (adding a field to the VM is a typed fan-out; every full literal must carry it).

## Tasks & Acceptance

**Execution:**
- [x] **(a) Research and decide, change nothing.** Establish from Tauri/tao/wry/WebKit source and
  Tauri's own docs whether `titleBarStyle: "Overlay"` takes effect and what produces the 56 px.
  Outcome: `Overlay` is correct and is the *only* value that suppresses the mechanism; the epic's
  fallback is rejected; `tauri.conf.json` is left untouched. -- Records the decision the epic asked
  for and prevents a change that would cement the defect.
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- add `pub overlay_title_bar: bool` to
  `CapabilitiesVm`. -- The honest platform fact, on the one surface that carries platform truth.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- `capabilities()` returns
  `overlay_title_bar: cfg!(all(desktop, target_os = "macos"))`. -- Keeps the `cfg!` in the shell.
- [x] `src/lib/ipc/gen/CapabilitiesVm.ts` + `src/lib/stores/capabilities.ts` -- extend the generated
  binding and the all-absent safe default. -- Drift-free bindings; no optimistic default.
- [x] `src/components/layout/app-shell.tsx` -- replace the full-width band with one band painted per
  column, gated on `overlayTitleBar`, drawer column dropped on the phone tier. -- AD-34-2/AD-34-3.
- [x] `src/components/layout/sidebar-pane.tsx` -- export `SIDEBAR_WIDTH_CLASS`; add `min-h-0` to the
  `<nav>`; delete the duplicate traffic-light inset; wrap the view list and both groups in
  `<ScrollArea className="min-h-0 flex-1">`, leaving the `mt-auto` footer outside. -- AD-34-2 (one
  inset) and AD-34-4 (every pane reaches its own bottom).
- [x] `src/components/layout/app-shell.test.tsx` -- four cases: no band off macOS; two columns with
  the correct backgrounds and drawer width; no `pl-[78px]` survivor; one column on the phone tier.
- [x] `src/components/layout/sidebar-pane.test.tsx` -- two cases: with eight Spaces the views and
  both groups are inside the scroll viewport while `Add account` is outside it; the `<nav>` carries
  `min-h-0` and the scroller carries `min-h-0 flex-1`.
- [x] 12 `DESKTOP_CAPABILITIES` fixtures -- add `overlayTitleBar: false` (they already model a
  generic desktop with `recording`/`sync` off, so macOS-only stays off).
- [x] `src/lib/stores/capabilities.test.ts` -- extend both literals; replace the hand-written flag
  list with `Object.keys(DEFAULT_CAPABILITIES)` so it can never drift again (it already had:
  `sync` was missing).

**Acceptance Criteria:**
- Given no hard-coded native-chrome pixel constant appears anywhere in the change, when the diff is
  read, then the only pixel values are the pre-existing 28 px band height and the drawer's own
  widths — reservations the app chooses, not compensations for measured chrome geometry.
- Given `overlayTitleBar` is true, when the shell renders, then the top of the window is one
  continuous colour per column in both light and dark themes, and both columns drag the window.
- Given `overlayTitleBar` is false, when the shell renders, then no drag band exists at all.
- Given the whole app, when queried for elements reserving the traffic-light inset, then exactly one
  exists (the band) and `pl-[78px]` appears nowhere.
- Given a 600 px window height and eight Spaces, when the drawer renders, then the account footer is
  visible and the view list plus both groups scroll.
- Given `bun run bindings:check`, then `src/lib/ipc/gen/CapabilitiesVm.ts` shows no drift.
- **Not satisfied by this story:** the web viewport's reported height still exceeds the window's
  visible height by 56 px, so `Add account` may still report a frame partly outside the window at
  small heights. Final confirmation of every criterion above — and of the residual 56 px — is a
  re-measurement of the accessibility tree on the real app, which the parent performs.

## Design Notes

### (a) What the 56 px actually is, and why `tauri.conf.json` is left alone

The epic offered two hypotheses — the setting is not taking effect, or it takes effect and 56 px is
a titlebar area macOS reserves anyway. **Neither is correct**, and the fallback the epic authorised
would have made the defect permanent. Evidence, all read from pinned source (tauri 2.11.5,
tauri-utils 2.9.3, tauri-runtime-wry 2.11.4, wry 0.55.1, tao 0.35.3):

1. **The setting takes effect.** `TitleBarStyle`'s `Deserialize` lowercases and matches
   `"overlay"` (`tauri-utils-2.9.3/src/lib.rs:191-202`), so `"Overlay"` is accepted, not silently
   defaulted. `WindowBuilderWrapper::new()` pre-sets `Visible`
   (`tauri-runtime-wry-2.11.4/src/lib.rs:838-850`) but `with_config` applies the configured style
   afterwards (`:862-869`), so `Overlay` wins.
2. **Nothing overrides it at runtime.** The repo's only window calls are
   `show`/`hide`/`set_focus`/`unminimize` (`hotkey.rs:213-242`, `tray.rs:159-163`); `src-tauri`
   contains no `set_decorations`, `set_resizable`, `set_title_bar_style`, `WebviewWindowBuilder`, or
   `inner_size` call site. tao's style-mask mutators are all read-modify-write and would preserve
   `FullSizeContentView` regardless.
3. **The content view really is the full window.** `Overlay` sets
   `NSWindowStyleMask::FullSizeContentView` (`tauri-runtime-wry:1211-1214` → `tao window.rs:242`),
   under which `contentRect == frameRect` (Apple: "the window's `contentView` consumes the full size
   of the window"). The measured window height is exactly the configured 800, which is only true
   with that mask set. So 56 px is **not** a reserved titlebar area inside the content view.
4. **56 px is not any titlebar height.** Tahoe's titlebar is ~32 pt (GitHub Desktop #21135; VS Code
   #279769 measures a +2 pt traffic-light shift), and the epic's own numbers agree — a 16x16 close
   button at an 8 pt inset is 8+16+8 = 32. A Tahoe-era Tauri reporter (tauri#15136) still
   compensates with 28. Apple published no window-metric change in the macOS 26 release notes.
5. **The mechanism is WKWebView's automatic top obscured content inset.** Every macOS WKWebView opts
   in at init (`WKWebView.mm:690`, `setAutomaticallyAdjustsContentInsets(true)`), and wry never opts
   out (grep of wry 0.55.1 for `obscuredContentInsets|automaticallyAdjustsContentInsets|safeArea|
   contentLayoutRect` returns nothing). The value is computed in
   `PageClientImplMac.mm:1115-1127`, gated on `(styleMask & FullSizeContentView) && ![window
   titlebarAppearsTransparent] && ![view enclosingScrollView]`. Apple's own commit message for that
   code (`291818@main`, bug 289350): "WKWebView automatically computes a top content inset using the
   content layout rect of its window... In the common case, this inset ends up being the height of
   the window's titlebar" — and, for the failure mode, "the inset update occurs with incomplete
   constraints, ultimately resulting in a top content inset equal to the height of the window...
   AppKit does not report the change via KVO, leaving WKWebView stuck in state. This lack of update
   is a clear AppKit bug."

   This is the one mechanism whose signature matches the measurement exactly: an obscured content
   inset moves the rendered page and the `AXWebArea` (which is what "HTML content" is in the AX
   tree — not the `NSView`) **while leaving the layout viewport at the view's full height**. That is
   precisely why `100vh` stays 800, why the bottom 56 px is unreachable, and why there is no
   scrollbar to recover it. It also means no CSS unit can see the problem: `100vh`, `100dvh` and
   `100%` all read the same 800 px layout viewport.

6. **Therefore the epic's fallback is a regression, not a fix.** tao calls
   `setTitlebarAppearsTransparent(true)` only for `Transparent`/`Overlay` (`tao window.rs:266-268`),
   and that flag is exactly WebKit's suppression gate. So:

   | `titleBarStyle` | FullSizeContentView | titlebarAppearsTransparent | WebKit auto top inset |
   |---|---|---|---|
   | `Visible` (default; = dropping the key) | on | **false** | **APPLIED** |
   | `Transparent` | off | true | suppressed |
   | `Overlay` (today) | on | true | suppressed |

   Dropping `titleBarStyle`/`hiddenTitle` moves the window onto the *applied* side of the gate: it
   would make the overhang guaranteed and by-design instead of anomalous, and additionally draw an
   opaque title bar over the top of a content view that still spans the full window. `Transparent`
   does suppress the inset, but its titlebar strip paints the *window* background, which Tauri
   cannot set today (tauri-utils' own note: "Will be more useful when Tauri lets you set a custom
   window background color") — a light strip above a dark app, i.e. the same "black strip" defect
   inverted, and unreachable from CSS because the strip is outside the webview. **`Overlay` is
   already the best of the three. It stays.**

7. **No dependency bump helps.** wry 0.55.1, tao 0.35.3 and tauri 2.11.5 are the newest published
   crates; wry's unreleased tip is byte-identical in the frame/autoresizing/`setContentView` code.
   No upstream issue exists for this symptom in tauri, wry or tao — confirming it would make us the
   first reporter.

**Escalation for the residual 56 px, in preference order.** All require the runtime discriminator
first, because it separates "wrong `NSView` frame" (a wry bug to file) from "wrong obscured inset"
(the WebKit/AppKit bug above): log `[window titlebarAppearsTransparent]`, `[window
contentLayoutRect]`, `[webView _obscuredContentInsets]` and the `WKWebView`'s own `frame` in
contentView coordinates.
- If it is the obscured inset: disable it on the window's `WKWebView` via `tauri`'s `with_webview`.
  The public `obscuredContentInsets` setter clears the automatic flag, but only on macOS 26.0+
  (keeper's floor is 11.0) and it early-returns when the new value equals the current one
  (`WKWebView.mm:3929-3934`), so assigning zero-while-zero is a no-op; the reliable form is the SPI
  `_setAutomaticallyAdjustsContentInsets:NO` (`WKWebViewPrivate.h:874`). Either way this is a second
  `unsafe` objc FFI site and needs the coordinator policy amendment the Block-If names.
- File the wry issue with the AX numbers and the four runtime values.
- Accept it, and keep the drag band, which is unaffected either way.

**Why no speculative workaround was committed.** Re-asserting the style at setup
(`WebviewWindow::set_title_bar_style(Overlay)`, safe public API, no constant) would fire a fresh KVO
notification on `titlebarAppearsTransparent`, which WebKit observes
(`WebViewImpl.mm:422-423, 613-614`) and which re-runs `updateContentInsetsIfAutomatic`. It was
considered and rejected: when the gate returns `nullopt` WebKit computes no new inset, and whether
it *clears* a previously stuck one is not determinable from the available source. Shipping it would
put a call in the tree that looks like a fix, cannot be verified from this workstation, and would
become cargo cult if it turns out to be inert.

**Accounting against the story title.** Of the 96 px, this story returns the 12 px duplicate inset
and makes the remaining 28 px honest (it is now a correctly-painted, single-purpose drag band rather
than half a seam). The 56 px is diagnosed, attributed, and escalated. The story's other two defects
are fixed outright.

### (b) and (c) notes

**Why a new capability flag.** `overlayTitleBar` is a pure platform fact (`cfg!`), not a runtime
probe like `recording`/`sync`, because `titleBarStyle`/`hiddenTitle` are macOS-only keys that Tauri
applies under `#[cfg(target_os = "macos")]`. The flag and those two keys are two halves of one fact,
which is why the field's doc comment says so: change the keys and this flag changes with them. The
alternative — reusing `recording` — is dishonest and dangerous: it is floored at macOS 13.0 while
`minimumSystemVersion` is 11.0, so macOS 11/12 users would lose the band and be left with a window
they cannot move.

**`SIDEBAR_WIDTH_CLASS`.** The band's drawer column and the drawer must be the same width or the
seam returns in a new place. Exporting the width from `sidebar-pane.tsx` (which already exports
`OFFLINE_PILL_TEXT`) makes that a single source of truth rather than a duplicated literal.

**Phone tier.** `phone` is a viewport-width fact, not a platform one, so it is reachable on macOS by
narrowing the window below 768 px. There the drawer is not rendered and every stack level is
`bg-background`, so the band collapses to its single `bg-background` column.

**The scroller also fixes the phone leading drawer.** `SidebarPane` is reused verbatim inside a
`SheetContent side="left"`, which is `fixed inset-y-0 h-full flex flex-col` — a definite height. The
new `min-h-0` + `flex-1` scroller therefore bounds correctly there too, so the drawer's account
footer becomes reachable with many Spaces. That was the same latent defect, in a second place.

**`mt-auto` is kept.** With the scroller at `flex-1` the footer is pinned by the flex line anyway,
but `mt-auto` remains the honest statement of intent and costs nothing.

## Verification

**Commands: none run.** The build, linters, formatters and test suites were **deliberately not run
by this story**, per the batch constraint: six agents were editing this repo concurrently, so a
suite run here would attribute their in-flight edits to this change. The parent runs
`bun run check`, `bun run check:rust`, `bun run test:rust` and `bun run bindings:check` once at the
end.

**What was actually verified, and how:**
- **Diagnosis (a)** — read from vendored pinned sources under
  `/usr/local/cargo/registry/.../{tauri-2.11.5,tauri-utils-2.9.3,tauri-runtime-wry-2.11.4,wry-0.55.1,tao-0.35.3}`,
  plus Tauri's own config schema/reference, Apple's `fullSizeContentView` /
  `titlebarAppearsTransparent` / `safeAreaInsets` / `obscuredContentInsets` documentation, WebKit
  trunk (`PageClientImplMac.mm`, `WebPageProxyMac.mm`, `WebViewImpl.mm`, `WKWebView.mm`,
  `WKWebViewPrivate.h`), Apple's WebKit commit `291818@main` (bug 289350), and an exhaustive
  enumeration of every "Tahoe" issue in `tauri-apps/{tauri,wry,tao}`. Line-level citations are in
  Design Notes. Two independent research passes reached the same conclusion.
- **Absence of runtime overrides** — grep of `src-tauri` for `set_decorations|set_resizable|
  set_title_bar_style|WebviewWindowBuilder|WindowBuilder|set_fullscreen|set_size|inner_size|
  set_maximizable|set_closable|set_minimizable`: no matches. Window handling in the repo is
  `show`/`hide`/`set_focus`/`unminimize` only.
- **Generated-binding fidelity** — rather than trusting hand-editing, the ts-rs rendering rule was
  reconstructed from the file's own nine pre-existing fields, re-rendered from the current Rust
  struct, and compared byte-for-byte against the committed
  `src/lib/ipc/gen/CapabilitiesVm.ts`: exact match. `bindings:check` should therefore report no
  drift.
- **Syntax** — all 19 edited TS/TSX files parse cleanly through Bun's TSX transpiler (a parse check
  of this story's own edits, not a project build).
- **Fan-out completeness** — grepped `src` and `src-tauri/crates` for every `CapabilitiesVm`
  construction site (`trayIcon:` / `tray_icon:` / `CapabilitiesVm {`). Found and updated 12
  `DESKTOP_CAPABILITIES` fixtures plus both literals in `capabilities.test.ts`; the only Rust
  construction site is `capabilities()`, and no Rust test asserts the VM's full shape. Each of the
  13 insertions was re-read in context to confirm it landed inside the intended literal at the
  intended indent.
- **Predicate safety** — `isReducedCapabilityPlatform` derives from `Object.values(...).every(...)`,
  and the new flag is `false` on iOS, so the all-absent reduced-platform predicate is unaffected.
- **Layout reasoning for (c)** — traced why the footer is clipped today: the `<nav>`'s children have
  `min-height: auto` in a column flex container, so they refuse to shrink below content; the nav has
  no `overflow`, so content spills and the ancestor `h-screen overflow-hidden` clips it. Confirmed
  against the measured numbers: nav `260x718` at y=338 ends at y=1056 = 256+800, i.e. the nav is
  correctly sized to the (over-tall) viewport, which is why defect 3 is genuinely separate from
  defect 1 and why it only bites at small heights or with many Spaces.

**What remains unverified and needs the device:** whether the band renders as one continuous colour
per column in both themes, whether the window drags from both columns, and the residual 56 px
itself. All three are accessibility-tree/visual facts on macOS. The parent's re-measurement on
hesperia is the confirmation, and the four runtime values named in Design Notes are what would close
the last step of the (a) diagnosis.
