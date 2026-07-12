# Project decisions

Durable, project-level decisions — the ones that shape what keeper will and won't build,
recorded so they stay discoverable instead of living only in planning artifacts. Each entry
cross-references its source (PRD / architecture spine) so a reader can always trace a decision
back to where it was made. Entries are numbered D-1, D-2, … as they are made.

## D-1 — Paid Apple Developer Program: deferred by decision

keeper defers the $99/yr Apple Developer Program. This is a deliberate deferral, not an
oversight and not a purchase — recorded here because the deferral is load-bearing.

- **Unlocks (only the paid program grants these):** APNs push; the Notification Service
  Extension (NSE) for background notification decryption, with its 24 MB memory ceiling and
  App-Group store-layout implications; TestFlight; App Groups; and AltStore PAL notarization for
  EU distribution. (PRD §13.5; architecture spine "Deferred")
- **Opening trigger:** the gate opens only when push becomes a product goal. (PRD §13.5; §13.8,
  the paid-program-timing open question)
- **Constraint it forces:** once push is on the table, keeper's client-only posture makes it a
  PRD-level question — push must ride a homeserver operator's gateway, Beeper's, or a user-run
  Sygnal. It must **never** ride project infrastructure. (NFR-11; PRD §13.5) This is the
  load-bearing reason the gate exists: keeper contacts only user-configured homeservers/bridges,
  Beeper's API when a Beeper account exists, and the signed-update endpoint — running push
  infrastructure would break that invariant.
- **Cheap-now mitigations already paid for:** the single `Platform::data_dir()` root keeps all
  account state under one path, so a future App Group container move (NSE era) is a path change,
  not a data migration. (AD-29; FR-65) Plan B (UniFFI + native SwiftUI shell) stays shelved with
  its revisit triggers recorded — the blank-webview bug class proving unfixable across Tauri
  releases, or NSE work beginning. (PRD §13.8)
- **Status / owner:** deferred. Revisit is owned by the PM/owner, when push demand is real.
  (PRD §13.8, the paid-program-timing open question)
