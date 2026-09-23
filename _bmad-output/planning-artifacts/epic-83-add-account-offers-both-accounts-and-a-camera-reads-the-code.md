# Epic 83 — Add account offers both accounts, and a camera reads the setup code

created: '2026-09-23'
source: the owner's request of 2026-09-23 (verbatim below), made while using the 0.8.32 build with epic 82 installed on hesperia. The coordinator grounded it in the repository at `origin/main` `6d61281` and with three measurements on hesperia (below) before pinning anything.
binds: FR-708…FR-712 and NFR-96 (allocated here); AD-317…AD-319; UX-DR117; DW-298…DW-299. **FR-707, NFR-95, AD-316, UX-DR116, DW-297 and D-25 were the previous ceilings** (epic 82). A repo-wide grep for `epic-83`, FR-708, NFR-96, AD-317, UX-DR117 and DW-298 found nothing before this plan.
see-also: epic 82 (the organisation account, `SetupLinkField`, the one setup sheet, UX-DR116, AD-308, AD-311); epic 20 (the camera usage string and the Camera settings deep link); AD-27 (absent rather than disabled); AD-53 (every destination disclosed); `docs/account.md` § *One input*.

## The owner's ask

Verbatim:

> i want on desktop use the qrcode based on the camera. I want to utulize add account in the low right corner to have function of add not only the matrix account but also add/change keeper account

In other words: on a desktop, read the setup QR code with the camera. The *Add account* control at the foot of the sidebar should add a Matrix account, and also add or change the keeper (organisation) account.

## What the triage found

| Need | Verdict | Evidence |
| --- | --- | --- |
| A camera QR reader on desktop | **absent** | keeper renders QR codes (`bridges/login.rs:42`) but reads none. `docs/account.md` said "a Mac, which cannot scan a QR code". |
| Camera access from the webview on macOS | **reachable** (measured) | wry grants WebKit's media-capture request (`wry-0.55.1 wkwebview/class/wry_web_view_ui_delegate.rs:126-137`). A WKWebView probe on hesperia (macOS 27) at `tauri://localhost` reports `isSecureContext: true`. `navigator.mediaDevices` is **absent** in a bare binary and **present** in an `.app` whose Info.plist has `NSCameraUsageDescription`. keeper's Info.plist has one (epic 20), and its bundle is signed with `com.apple.security.device.camera` (`tauri.conf.json` `bundle.macOS.entitlements`, confirmed with `codesign -d --entitlements`). |
| Camera access in the iOS app | **absent, deliberately** | The iOS plist has no camera usage string, so WebKit exposes no `mediaDevices`. The iPhone Camera app already opens `keeper://setup` (epic 82, AD-311). |
| A decoder that reads a real setup code | **measured** | The owner's real inline setup link is 1447 characters, a version-33-class QR. `zxing-wasm` 3.1.4 (MIT, zxing-cpp) decoded it at 600, 450, 360 and 300 px (about 2 px per module) in about 4 ms each; `jsqr` 1.4.0 decoded only the 360 px copy. WebKit has no `BarcodeDetector`. |
| Add account adds only Matrix | **present, as asked** | `account-footer.tsx:788-812` → `addAccountStore.openAddAccount()` → the Matrix login overlay. |
| Changing the keeper account | **half-present** | Rust already replaces an account on confirm (`account_ipc.rs` `account_setup_confirm`: sign the old one out, forget it locally, keep its repository files). Nothing on screen says so before Continue. |

## Decisions this epic takes

- **AD-317: A setup code is scanned in the window. The frames never leave it.**

  **Binds:** FR-708, FR-709, FR-710; NFR-96; Story 83.2; UX-DR117.

  **Prevents:**
  - a second parser in TypeScript (the grammar is `account_setup_resolve`'s);
  - camera frames crossing IPC;
  - a CDN fetch of the decoder (zxing-wasm defaults to jsDelivr), an undisclosed destination under AD-53;
  - a camera left running behind a closed surface;
  - a *Scan* button that can never work (iOS, or a desktop with no camera API).

  **Rule:**
  - `getUserMedia` (video only) feeds a muted inline `<video>`. About every 150 ms, a frame (long side at most 1280 px) goes to zxing-wasm's reader.
  - The reader's `.wasm` ships in the bundle: `locateFile` is overridden with the Vite asset URL before the first read. The scanner module is lazy-loaded.
  - The first decoded text stops every track and goes to `accountStore.openSetup(text)`, the same path as a paste.
  - The tracks also stop on Cancel, on unmount and on close.
  - The control exists only where `navigator.mediaDevices?.getUserMedia` exists (AD-27). WebKit ties that to the app's camera usage string, which is the right signal.
  - Refusal: a sentence naming System Settings › Privacy & Security › Camera, and *Open Camera settings* (epic 20's `open_camera_settings`). No camera, and any other failure, are one sentence each. The paste field stays usable throughout.
  - macOS `NSCameraUsageDescription` is reworded to cover both uses.

- **AD-318: *Add account* offers both kinds of account.**

  **Binds:** FR-711; Story 83.1; UX-DR117.

  **Prevents:**
  - a second entry field for the keeper account;
  - a keeper account reachable only from Settings;
  - a relabelled footer control that breaks the measured sidebar widths.

  **Rule:**
  - The footer's *Add account* (folded icon and expanded row; label and accessible name unchanged) opens a menu:
    - *Matrix account…* opens the existing Matrix overlay;
    - *keeper account…*, which reads *Change keeper account…* once one is configured, opens a dialog.
  - The dialog is mounted once at the root and hosts the existing `SetupLinkField`.
  - Submitting a link closes the dialog and opens the one setup sheet: one surface at a time.

- **AD-319: The confirmation sheet says which account a link replaces.**

  **Binds:** FR-712; Story 83.3; UX-DR117.

  **Prevents:**
  - a Continue that silently signs someone out of another account;
  - the replace rule living in the shell (it moves to keeper-core).

  **Rule:**
  - `AccountDescriptor::replaces(previous)` in keeper-core is the rule, moved unchanged from the shell: another id, or any sign-in or repository change; a new display name alone is not a replacement.
  - `AccountSetupVm.replaces: Option<String>` carries the current account's name when confirming would replace it. It is computed by `state::setup_vm(.., previous)`, and the shell passes the stored descriptor.
  - The sheet shows one sentence before Continue when it is set. The *Change keeper account…* dialog shows the same fact when it opens.

- **UX-DR117.**
  - Footer menu: *Matrix account…* then *keeper account…* / *Change keeper account…*; opens `side="right"` from the folded rail and upward from the expanded footer.
  - Keeper dialog title: *Set up a keeper account* / *Change keeper account*.
  - Scanner: a rounded 16:9 preview inside the field's width, the hint "Hold the setup code up to the camera.", *Cancel*, and "Starting the camera…" with a spinner. The failure sentences are as in AD-317.
  - The replacement sentence is plain body text above Continue, never a warning colour: changing the account is a choice, not an error.

## Requirements allocated here

- **FR-708:** On a desktop whose window can reach a camera, the setup field offers *Scan a QR code*.
- **FR-709:** A scanned setup code is handled exactly like the same text pasted.
- **FR-710:** A refused, missing or failing camera says why and leaves pasting available. A refusal offers to open the Camera settings pane.
- **FR-711:** *Add account* adds a Matrix account or sets up / changes the keeper account.
- **FR-712:** Before Continue, keeper says which configured account a setup would replace.
- **NFR-96:** Scanning sends nothing off the device: no frame over IPC, no network fetch for the decoder, and no camera left running after the scanner's surface is gone.

## Stories

- **83.1 Add account offers a Matrix or a keeper account** (AD-318). ACs:
  - both menu items work from the folded rail, the expanded footer and the phone drawer;
  - the keeper item's label follows `vm.configured`;
  - the dialog hosts `SetupLinkField`, and a submitted link hands over to the setup sheet.
- **83.2 A camera reads the setup code** (AD-317). ACs:
  - the control is absent without `mediaDevices`;
  - decoded text opens the sheet with exactly that text and stops every track;
  - Cancel stops every track;
  - refusal shows the sentence and the settings button;
  - the wasm is bundled, not fetched;
  - the macOS usage string covers scanning.
- **83.3 A change says which account it replaces** (AD-319). ACs:
  - `replaces` is `None` for a first setup, the same account, or a rename;
  - it is `Some(name)` for any other replacement;
  - the sheet renders the sentence iff it is set;
  - the shell's rule is keeper-core's, and the old shell copy and its test are gone.

## What stays out

- **DW-298:** no in-app scanner on iOS; the Camera app opens the link (see the triage).
- **DW-299:** an inline descriptor makes a dense code. A hosted descriptor, or a compressed inline form, would shorten it. zxing reads the current density at about 2 px per module, so it is not needed to make scanning work.

## Review-wave amendments

One `bmad-review` lane read the whole diff. Every finding was accepted.

- **A1 (R83-1, major): a hidden window stops the camera.**
  - The defect: keeper's window-close handler hides the window (`prevent_close` then `hide`), so the React tree, and with it the scanner, stays mounted. A scan left open would keep the camera running behind a closed window, against NFR-96.
  - The fix: the scanner stops every track and returns to the idle field on `visibilitychange` to `hidden`, and on `pagehide`.
  - Measured on hesperia (macOS 27) with a WKWebView probe: ordering its window out changes `document.visibilityState` from `visible` to `hidden`.
- **A2 (R83-2):** a track that ends mid-scan (camera unplugged, taken by another app, access revoked) shows `SCAN_FAILED` instead of a frozen preview.
- **A3 (R83-3):** the camera-specific sentences (denied, no camera) apply only to the `getUserMedia` rejection. A later failure (`play()`, the decoder, the canvas) shows `SCAN_FAILED`, so an autoplay refusal no longer sends the person to a Camera setting that is already on.
- **A4 (R83-4): the contract's change-dialog copy claimed a replacement Rust may not perform.** A rename or the same account again replaces nothing.
  - The dialog now reads: "A link for another account replaces *name* on this device; the next step says so before anything is written. Its files in the settings repository are kept."
  - The certain fact stays with the sheet (`AccountSetupVm.replaces`).
- **A5 (R83-5, R83-6):** tests for three cases: a stream that arrives after Cancel, a `play()` rejection, and the phone drawer reaching *Matrix account…* through the menu.

## Stack

Three rungs, by layer as in epics 80–82:
1. `epic83-plan`: this document and the ledgers.
2. `epic83-core`: keeper-core `replaces` and `AccountSetupVm.replaces`, the regenerated binding, and the TypeScript fixtures that must name the new field.
3. `epic83-surface`: the shell's call site, the Info.plist string, zxing-wasm, the footer menu, the dialog, the scanner, the sheet sentence, the harness and `docs/account.md`.
