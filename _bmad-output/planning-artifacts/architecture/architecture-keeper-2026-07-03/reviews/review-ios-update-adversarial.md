# Review — Lens 2: adversarial incompatibility hunt, iOS-phase update 2026-07-09

Method: construct two units one level down that obey every AD to the letter yet build incompatibly.

**Verdict: PASS after 2 holes closed (fixes applied at the gate).**

## Holes found and closed
1. **Badge count fork (FR-62 vs AD-20).** A "platform behavior" unit could compute the app-icon badge from notify-engine unread events while the inbox unit computes the Unified Inbox aggregate — two counts, both AD-compliant, visibly disagreeing. **Fix applied:** AD-30 now pins the badge value to the `inbox` aggregate (AD-20), "never a second count".
2. **Clipboard/opener limbo (addendum §7 reconcile gap).** clipboard-manager and opener are desktop-tier; no AD said what replaces them on iOS — one unit could ship a third-party clipboard plugin, another could web-API it. **Fix applied:** AD-26 now fixes web Clipboard API + minimal native open call as the iOS replacements.

## Attacks that did not land
- *Two lifecycle signals fighting SyncService* (visibilitychange unit vs Swift-plugin unit): AD-30 forces one Rust lifecycle entry point regardless of detector — no double pause/resume.
- *Capability list drift between shell and UI*: AD-27's single ts-rs `CapabilitiesVm` over the handshake + "no platform sniffing in TS" convention leaves no second channel to disagree through.
- *Phone navigation as new state*: AD-31 defines the stack as a projection of existing zustand selection state and bans a router — a nav-state store fork is non-compliant by construction.
- *Sidecar reachable programmatically on iOS*: AD-27 fixes the clean Unsupported IpcError at `Platform::sidecar_path`; UI hides affordances — both ends specified.
- *gen/apple edited in .xcodeproj by one unit, project.yml by another*: AD-32 names project.yml/Info.plist/*_iOS as the only persistent-edit locations.
- *iOS-specific media URL helper introduced speculatively*: AD-28 explicitly forbids it until Android starts.

## Residual (accepted, deferred-safe)
- Exact `CapabilitiesVm` field names and the safe-area CSS var names are code-owned (AD-7/AD-8 convergence machinery covers them, same as all VM shapes).
- Backup-exclusion/file-protection application point (first-launch call) is inside the single Platform iOS impl — one owner, no divergence surface.
