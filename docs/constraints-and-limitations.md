# Constraints & Known Limitations

Honest list of what Keeper cannot do, should not do, or will find hard. Sources: the 2026-07
technical and market research reports in `_bmad-output/planning-artifacts/` and the PRD/brief
risk sections.

## Cannot do (hard blockers)

- **Beeper on-device chats are invisible.** Since Beeper's 2025 relaunch, networks connected
  via *on-device* connections in the official Beeper apps (WhatsApp, Signal, expanding) never
  reach Beeper's Matrix homeserver — no third-party Matrix client can see them. Keeper sees
  Matrix-native chats, Beeper *cloud* bridges, and bbctl self-hosted bridges. Parity for
  on-device networks = run your own mautrix bridges.
- **iMessage without your own Mac.** Every relay/spoof approach is dead (Beeper Mini
  precedent). Supported path: `beeper/platform-imessage` (MIT) running on the user's own Mac.
- **Cross-protocol server-side drafts.** Keeper is client-only; drafts live locally +
  mirrored to per-room Matrix account data. A "draft bridge"/server app would be a separate
  project by design.

## Should not do (deliberate policy)

- **No AGPL code reuse.** Element Web/X, Cinny, gomuks, Fractal, Nheko, and mautrix bridges
  are study-only references. cargo-deny enforces an Apache/MIT-compatible dependency tree.
- **No hosted anything.** No relay servers, no credential custody, no hosted bridges — ToS
  and liability stay with the user, which is the project's legal posture.
- **No WhatsApp automation features** (bulk send, auto-reply) — ban-bait for bridge users.
- **No native VoIP implementation now.** MatrixRTC (MSC4143) is pre-spec and volatile;
  calls arrive post-MVP via the Element Call widget.

## Hard / risky (do with eyes open)

- **Beeper login (email code → JWT)** is a private, unversioned API ported from Apache-2.0
  bbctl. It can break without notice; isolated behind an auth-provider trait (AD-17).
- **WhatsApp via mautrix**: personal-use bridging rarely triggers bans, but ToS risk is real
  and sits with the user (risk-tier UI makes this explicit).
- **X/Twitter, Instagram/Messenger, LinkedIn** bridges are volatile (API lockdowns, Meta
  changes). Treated as best-effort tiers, never release gates.
- **matrix-rust-sdk 0.x churn**: breaking API changes every release; we track releases and
  pin exact versions.
- **Tauri mobile (iOS/iPad/Android)** shipped but is younger than desktop; the Rust core is
  the portable asset (AD-24), and an iOS walking-skeleton spike should happen early in the
  mobile phase.
- **E-mail, AI-bot client, terminal client** are future-phase items tracked in the PRD's
  post-MVP section, not storied yet.

## Audited `unsafe` FFI inventory (shell crate only)

Policy (2026-07-11): `unsafe_code` stays denied workspace-wide; the `keeper` shell crate may
carry function-level, audited `#[allow(unsafe_code)]` exceptions for platform FFI with no
safe binding. Current inventory:

- iOS backup exclusion (`NSURL.setResourceValue(NSURLIsExcludedFromBackupKey)`) via
  objc2-foundation, behind `Platform::exclude_from_backup` — the single function-level
  `#[allow(unsafe_code)]` in `IosPlatform::exclude_from_backup`,
  `crates/keeper/src/ipc.rs` — story 14.7 (FR-65).
