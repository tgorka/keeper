# Epic 15 Context: iOS Polish & Release

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Epic 15 closes out the iOS phase by turning a working phone build into a shippable, dogfoodable product. With the walking skeleton (Epic 12), phone shell (Epic 13), and platform behavior (Epic 14) complete, this epic delivers the finishing layer: real app icons and a flash-free launch, a signing walkthrough that makes the free-signing 7-day re-arm ritual cheap, a shareable IPA path so hand-provisioned testers can install without the owner's Mac in the loop, promotion of the iOS compile check to a required CI gate, an on-the-record decision about the paid Apple Developer Program, and a final owner-run on-device acceptance that measures cold start and opens the SM-8 two-week daily-driver dogfooding window. It matters because it converts the port from "runs on a phone" into "ready to live on a phone" while keeping every distribution and honesty constraint intact.

## Stories

- Story 15.1: App Icons and Launch Assets
- Story 15.2: docs/ios.md — Free Signing Walkthrough
- Story 15.3: Shareable IPA Build Path — Unsigned Export for Re-Signing
- Story 15.4: Required iOS CI Gate and Release Hygiene
- Story 15.5: Paid-Program Decision Gate Recorded
- Story 15.6: Final Device Install and Phase Acceptance

## Requirements & Constraints

- Full iOS icon set must render on home screen, Settings, and app switcher; launch screen/window background must match the active light/dark theme with no white/black flash on launch or rotation.
- A single signing document (`docs/ios.md`) must take a Mac + iPhone to a running keeper and keep it running: toolchain prerequisites, Personal Team setup, on-device certificate trust, the 7-day re-arm ritual with its expected per-week cost in minutes, the AltServer auto-refresh option, and the Sideloadly/zsign re-sign path for installing shared IPAs without Xcode. Its limitations section must match the in-app "On this iPhone" disclosure one-to-one so app and docs never diverge.
- A repeatable build must produce a release-configuration IPA suitable for per-tester re-signing (unsigned export, or dev-signed with documented signature replacement). The artifact must contain no desktop-only plugin symbols and no signing material, team ids, or provisioning profiles must land in the repo or CI.
- The iOS compile check (`cargo check --target aarch64-apple-ios`) must be promoted to a **required** PR status via branch-protection/merge-queue configuration, remaining compile-only (no signing, no simulator) to keep PR latency acceptable; contributor docs must state its scope and how to reproduce it locally. The existing release checklist gains iOS items (IPA path exercised, docs current, cold-start measurement recorded with owner-confirmation status, egress note that iOS adds no new endpoints).
- The paid Apple Developer Program deferral must be recorded as a deliberate decision with its unlocks (push/APNs, Notification Service Extension and its 24 MB memory ceiling + App-Group store-layout implications, TestFlight, App Groups, AltStore PAL notarization), its opening trigger (push becomes a product goal), and the constraint it forces: push must ride an operator gateway, Beeper's, or a user-run Sygnal — never project infrastructure. This story changes no code.
- Final acceptance (owner, physical device, human-in-the-loop): install via the documented path including one Sideloadly re-sign flow; spot-check the Epic 13/14 surface (safe areas, keyboard avoidance, stack navigation and gestures, drawer, Search, foreground notifications and badge, lifecycle pause/resume, overnight-suspension resume); measure launch → interactive Unified Inbox cold start and have the owner confirm or adjust the 3 s bar (this measurement resolves the open question that gates whether the bar becomes release-gating); open the SM-8 window and record retrospective inputs.
- Honesty and repo hygiene are hard constraints throughout: English, honest voice/tone, no team ids/credentials/secrets in docs, repo, or CI; iOS must add no new network egress endpoints.

## Technical Decisions

- **Regeneration discipline:** the iOS Xcode project (`gen/apple`) is generated and committed, but `build/` is gitignored. Persistent edits are allowed **only** in `project.yml`, `Info.plist`, committed asset catalogs, and `*_iOS/` sources — icons, launch config, and assets must survive `.xcodeproj` regeneration. Team id stays out of git (set via `bundle.iOS.developmentTeam` or `TAURI_APPLE_DEVELOPMENT_TEAM`); bundle id is stable and shared with macOS; minimum iOS 16.0.
- **Distribution posture:** free Personal Team signing (7-day profiles re-armed from the owner's Mac, ~3 devices, blocked entitlements); test IPAs shared via per-tester re-signing (Sideloadly/zsign); AltServer auto-refresh optional. The paid program is an explicit deferred decision gate, not an omission.
- **Desktop/iOS seam must hold in the shipped binary:** the IPA must carry no desktop-only plugin symbols — the single-shell-crate approach cfg-gates tray, global-shortcut, autostart, updater, window-state, and desktop deep-link registration out of the iOS build. Verify against the actual artifact, not just source.
- **App Group readiness without migration:** all account state already lives under one `Platform::data_dir()` root, so a future App Group move (unlocked only by the paid program) is a path change, not a data migration — record this as an existing mitigation, not new work.
- **CI stays cheap:** the required iOS gate is compile-only on the existing macOS runner; no signing or simulator in CI. iOS introduces no new egress endpoints, so the egress-diff posture from desktop packaging is unchanged.
- The desktop build must remain unaffected by all icon/asset/bundling changes, with quality gates green.

## Cross-Story Dependencies

- Story 15.2 depends on the flow validated in Story 12.6 (the on-device path being documented must first be proven). Its limitations section ties to the "On this iPhone" capability disclosure from Story 14.2.
- Story 15.3 depends on Epic 12 (build seam) and Story 15.2 (re-sign steps appended to the doc); its re-signed install is verified on-device as part of Story 15.6.
- Story 15.4 depends on Story 12.5 (the existing `cargo check --target aarch64-apple-ios` job it promotes to required) and extends the Epic 11 release checklist/process.
- Story 15.1 and 15.5 depend on Epic 12 (the phase reality to render icons against / record decisions against).
- Story 15.6 is the phase capstone: it depends on Stories 15.1–15.4 **and** on Epics 13 and 14 being complete, since it spot-checks the full phone surface. It is human-in-the-loop (owner's physical iPhone) — the second and final device step of the phase, deferred to the coordinator by the automation loop. It measures NFR-15 cold start (resolving the owner-confirmation open question) and opens the SM-8 dogfooding window whose checklist folds in the on-device soaks deferred from Epic 14 (jetsam soak 14.5, airplane/handover 14.6, lock-screen store access 14.7) plus a zero-silent-loss watch.
