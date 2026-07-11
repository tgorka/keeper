---
title: 'Backup Exclusion and File Protection'
type: 'feature'
created: '2026-07-11'
status: 'done'
review_loop_iteration: 1
followup_review_recommended: false
final_revision: 'e59a621e0c7bd07b8eea4c118e9a91057491c814'
context: ['{project-root}/docs/constraints-and-limitations.md']
warnings: ['oversized']
baseline_revision: '2f1695472301379045c0262563e8b7781744e676'
---

<intent-contract>

## Intent

**Problem:** On iOS keeper writes every database under a single `Platform::data_dir()` root — `keeper.db`, `archive.db`, and each account's matrix-sdk/crypto store at `accounts/<ulid>/sdk` — but nothing keeps those stores out of iCloud/iTunes device backups, and nothing pins their file-protection class. Story 13.7 already tells the user "This phone's Local Archive is excluded from device backup" (`settings-dialog.tsx:74-75`), so today that disclosure is a promise the code does not yet keep. Story 14.7 must make it true: every store carries `isExcludedFromBackup`, and the app's default data protection is `NSFileProtectionCompleteUntilFirstUserAuthentication` (never `Complete`, so WAL stays readable after screen lock and a resumed sync loop keeps working).

**Approach:** Add a cfg-free `Platform::exclude_from_backup(&Path)` port method — a no-op on desktop, and on iOS the codebase's single authorized `unsafe` FFI: one function-level `#[allow(unsafe_code)]` call to `NSURL setResourceValue:NSURLIsExcludedFromBackupKey` via `objc2-foundation` (behind the port, `// SAFETY:`-documented, already listed in the audit inventory). Invoke it from keeper-core right after each store is created so `keeper.db`, `archive.db`, and every `accounts/<ulid>/sdk` are flagged. Set file protection declaratively via the `com.apple.developer.default-data-protection` entitlement in the checked-in `keeper_iOS.entitlements`. Verify host-side (port wiring, desktop no-op, per-store invocation via a spy `Platform`, entitlement value); the real on-device NSURL read-back and lock-screen behavior are SM-8 device bars.

## Boundaries & Constraints

**Always:**
- `keeper-core` stays cfg-free and FFI-free — it only names the `Platform` port and calls the trait method. All iOS FFI lives in the `keeper` shell crate (AD-29).
- The iOS NSURL call is a **single** function-level `#[allow(unsafe_code)]` with a `// SAFETY:` comment citing the API contract, contained to `IosPlatform::exclude_from_backup`, and listed in the audit inventory (`docs/constraints-and-limitations.md`). This is exactly what the 2026-07-11 policy amendment authorizes.
- Backup exclusion must cover each store **and its SQLite `-wal`/`-shm` sidecars**. A `-wal` file holds committed-but-uncheckpointed rows (messages, registry) — leaking it defeats the whole story. Flagging a bare `.db` file does **not** flag its sibling `-wal`/`-shm`; flag the **containing directory** so directory-level backup exclusion covers the subtree. Concretely: flag the `data_dir` **root directory** once (covers `keeper.db`, `archive.db`, their `-wal`/`-shm`, and the whole `accounts/` subtree) and flag each `accounts/<ulid>/sdk` **directory** (covers its own sidecars; the per-account read-back target). Optionally also flag the individual `keeper.db`/`archive.db` files as defense-in-depth. Every path is flagged right after it is created, including account stores re-opened on session restore, not only fresh logins.
- Backup exclusion is **best-effort hardening, never fatal.** An `exclude_from_backup` failure must be logged (`tracing::warn` with the path) and swallowed — it must never panic the app, abort login or session-restore, or abort the archive path (which is invariant-bound to never abort startup). Do **not** `?`-propagate an exclusion error into a fatal/startup path or `expect()` it. The store stays usable; only the hardening is skipped.
- Default file protection is `NSFileProtectionCompleteUntilFirstUserAuthentication`, set via the `com.apple.developer.default-data-protection` entitlement. **XcodeGen regenerates `keeper_iOS.entitlements` from `project.yml` on every `xcodegen generate`**, so the value must live in `project.yml` under `entitlements.properties` (the true source of truth) **and** be mirrored into the checked-in `keeper_iOS.entitlements`; a host test pins both. Never `NSFileProtectionComplete`.
- All stores stay under the single `Platform::data_dir()` root (AD-29) — a future App Group move stays a path change, not a migration.
- The Story 13.7 storage disclosure (`settings-dialog.tsx:74-75`) must stay accurate; 14.7 makes it true — do not regress or contradict that copy. Any new copy follows the voice rules (sentence case, no exclamation, honest).

**Block If:**
- A required store turns out to be created **outside** the `data_dir` root (would break the single-root exclusion assumption and AD-29) — only then HALT for a design decision.
- Do **not** re-raise the `unsafe`-vs-`unsafe_code = "deny"` invariant conflict from the prior blocked run — it is **settled** by the 2026-07-11 coordinator policy amendment (Option 1) recorded in `docs/project-context.md:55-61` and the audit inventory. Proceed with the audited FFI exception.

**Never:**
- Never widen the `unsafe` allowance beyond this one `exclude_from_backup` FFI function; never introduce `unsafe` or `cfg` or FFI into `keeper-core`.
- Never use `NSFileProtectionComplete` (locks out WAL after screen lock — the epic bans it explicitly).
- Never treat the generated `.xcodeproj`/`project.pbxproj` as the source of truth or hand-edit it; edit `keeper_iOS.entitlements` and regenerate via `xcodegen`. Do not re-run `tauri ios init` (overwrites committed `project.yml`/entitlements).
- No APNs/NSE/background-sync work; no App Group container move this phase.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| iOS, directory path exists | `&Path` to the `data_dir` root or an `accounts/<ulid>/sdk` dir | `NSURLIsExcludedFromBackupKey` set true on the directory URL; directory-level exclusion covers all descendants incl. `.db`, `-wal`, `-shm`; returns `Ok(())` | NSError mapped to `CoreError::Platform(PlatformError::BackupExclusion(..))` |
| Desktop, any path | any `&Path` | No-op; returns `Ok(())` (backup exclusion is iOS-only) | none |
| Store-creation site | a store (or the root) was just created under `data_dir` | `platform.exclude_from_backup(<dir>)` invoked right after creation; the `data_dir` root flag covers `keeper.db`/`archive.db` + their sidecars, each `sdk` dir covers its own | **non-fatal**: on `Err`, `tracing::warn` the path and continue; never abort creation/login/restore/startup |
| iOS, exclusion fails (e.g. transient NSError, non-UTF-8 path) | `exclude_from_backup` returns `Err` | store remains created and usable; only the backup flag is skipped | log `warn`; do **not** panic/abort — app-container paths are ASCII so this is near-unreachable in practice |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/platform.rs` -- the `Platform` trait (plain sync, `Result<T, CoreError>`; methods at l.19-62). Add `fn exclude_from_backup(&self, path: &Path) -> Result<(), CoreError>;` and `use std::path::Path;` (only `PathBuf` imported today, l.10). Keep it cfg-free.
- `src-tauri/crates/keeper-core/src/error.rs` -- `CoreError` / `PlatformError`; add or reuse a variant for a backup-exclusion FFI failure.
- `src-tauri/crates/keeper/src/ipc.rs` -- both port impls. `DesktopPlatform impl` l.406-520 (add no-op `exclude_from_backup` → `Ok(())`). `IosPlatform impl` l.551-680 (add the `unsafe` FFI body; keychain pattern at l.568-596 is the style precedent). `AppState::new` l.252-281 constructs `Arc<dyn Platform>` (l.253-256) and `data_dir` (l.267) — the handle to thread into store creation. Flag the `data_dir` root here right after it exists (covers both top-level DBs + sidecars); **log-and-continue on `Err`, never `expect()`/panic**.
- `src-tauri/crates/keeper-core/src/registry.rs` / `archive/db.rs` -- create/open `keeper.db` / `archive.db` directly under `data_dir`. Their `-wal`/`-shm` sidecars are covered by the `data_dir` **root** directory flag (not by flagging the bare `.db` files); optional per-file flags are defense-in-depth only.
- `src-tauri/crates/keeper-core/src/account.rs` -- `AccountManager::new` (spawns `ArchiveWriter` ~l.608-609); the natural place to hold the `Platform` handle, flag the `data_dir` root, and flag each restored account's `sdk` **directory** on startup. Exclusion failures are logged, non-fatal (the archive path is invariant-bound to never abort startup).
- `src-tauri/crates/keeper-core/src/auth.rs` -- `add_account` builds `sdk_dir` l.551-553 and the store at l.576-582 with `platform: &dyn Platform` already in scope (used for `keychain_set`); exclude `sdk_dir` right after `.build()` succeeds. `sign_out_cleanup` l.758-786 (deletion symmetry; no exclusion needed).
- `src-tauri/crates/keeper/Cargo.toml` -- `[target.'cfg(target_os = "ios")'.dependencies]` (l.78-79 today has `security-framework`). Add `objc2` + `objc2-foundation` (features `NSURL`, `NSError`, `NSString`, `NSValue`) — direct dep needed (only transitive today). Must pass `cargo deny check`.
- `src-tauri/crates/keeper/gen/apple/keeper_iOS/keeper_iOS.entitlements` -- currently `<dict/>` (empty), git-tracked, referenced by `project.yml:56-57`. Add the `default-data-protection` key.
- `docs/constraints-and-limitations.md` -- audit inventory l.46-53 already lists the 14.7 `exclude_from_backup` FFI entry; confirm it stays accurate (path/mechanism/FR-65).
- `src/components/settings/settings-dialog.tsx` -- l.74-75 the 13.7 "excluded from device backup" disclosure this story fulfills; consistency check only, no change expected.
- `_bmad-output/implementation-artifacts/deferred-work.md` -- append the 14.7 SM-8 device bars.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/platform.rs` -- add `fn exclude_from_backup(&self, path: &Path) -> Result<(), CoreError>;` to the `Platform` trait with a doc comment (iOS sets `NSURLIsExcludedFromBackupKey` on a file/dir URL; directory-level exclusion covers the subtree; desktop is a no-op); add `use std::path::Path;`. No `cfg`, no FFI.
- [x] `src-tauri/crates/keeper-core/src/error.rs` -- add or reuse a `PlatformError::BackupExclusion(String)` variant (mapped into `CoreError::Platform`).
- [x] `src-tauri/crates/keeper/src/ipc.rs` (`DesktopPlatform`) -- implement `exclude_from_backup` as an honest no-op returning `Ok(())`, with a one-line comment stating backup exclusion is an iOS-only concept.
- [x] `src-tauri/crates/keeper/src/ipc.rs` (`IosPlatform`) -- implement `exclude_from_backup` with a **function-level** `#[allow(unsafe_code)]` and a `// SAFETY:` comment: build an `NSURL` file URL from `path` (prefer `fileURLWithPath_isDirectory` when the caller knows it's a directory, avoiding an extra stat), an `NSNumber` boolean `true`, and call `setResourceValue_forKey_error(Some(&value), NSURLIsExcludedFromBackupKey)`, mapping the `NSError` to `PlatformError::BackupExclusion`. `use` the objc2 types inside the method body (no crate-level import leaking to desktop), mirroring the 12.3 keychain pattern. Callers pass absolute `data_dir`-rooted paths — note this precondition.
- [x] `src-tauri/crates/keeper/Cargo.toml` -- add `objc2` and `objc2-foundation` (features `NSURL`, `NSError`, `NSString`, `NSValue`) under `[target.'cfg(target_os = "ios")'.dependencies]`, version-aligned with the in-tree `objc2-foundation` 0.3.x. Confirm `cargo deny check` stays green (license firewall only; pre-existing advisories are out of scope).
- [x] keeper-core store-creation wiring (`account.rs` / `auth.rs`) -- hold the existing `Arc<dyn Platform>` at the store-creation sites and flag **directories** so SQLite sidecars are covered: flag the `data_dir` **root** once right after it exists (covers `keeper.db`, `archive.db`, and their `-wal`/`-shm`, plus the `accounts/` subtree), and flag each `accounts/<ulid>/sdk` **directory** on fresh login (`add_account`) **and** on session-restore re-open (the central `activate()` funnel). Every exclusion call is **non-fatal**: on `Err`, `tracing::warn` the path and continue — never `?`-propagate into a fatal/startup/login/restore path, never `expect()`, never let it abort the archive path. Keep keeper-core cfg-free; do not route any store outside `data_dir`.
- [x] `src-tauri/crates/keeper/gen/apple/project.yml` **and** `.../keeper_iOS/keeper_iOS.entitlements` -- set `com.apple.developer.default-data-protection` = `NSFileProtectionCompleteUntilFirstUserAuthentication` in `project.yml` under `entitlements.properties` (the source of truth XcodeGen regenerates from) **and** mirror it into the checked-in `keeper_iOS.entitlements`, with a comment explaining the regeneration behavior. Run `xcodegen generate` in `gen/apple` and re-commit the regenerated `.xcodeproj` if `xcodegen` is available; else note regeneration is deferred to the iOS build pipeline. Never `NSFileProtectionComplete`.
- [x] Tests (host-runnable) -- (1) keeper-core unit tests with a **spy `Platform`** double asserting `exclude_from_backup` is invoked with the `data_dir` **root** and each fresh `accounts/<ulid>/sdk` directory when stores are created / an account is restored; (2) an **idempotency** test: re-activating an account flags its `sdk` dir again and both calls succeed (restore runs on every resume); (3) a **non-fatal** test: when the spy `Platform` returns `Err` from `exclude_from_backup`, store creation / account activation still succeeds (no panic, no abort); (4) assert `DesktopPlatform::exclude_from_backup` returns `Ok(())` and records nothing; (5) an entitlements-value test (Rust test under `crates/keeper/tests/`, plain file IO) asserting the resolved default-data-protection value is exactly `NSFileProtectionCompleteUntilFirstUserAuthentication` and is **not** `NSFileProtectionComplete` (exact-value + `<string>NSFileProtectionComplete</string>` substring-safe check), reading the value structurally rather than by naive positional scan.
- [x] `docs/constraints-and-limitations.md` -- verify the audit-inventory entry for `exclude_from_backup` (l.52-53) matches the shipped function (path `IosPlatform` in `crates/keeper/src/ipc.rs`, mechanism, FR-65); adjust wording only if it drifted. Do not duplicate.
- [x] `_bmad-output/implementation-artifacts/deferred-work.md` -- append (a) the 14.7 SM-8 device bars: on Simulator/device read `NSURLIsExcludedFromBackupKey` back == true on the `data_dir` root and each store path, confirm the `-wal`/`-shm` sidecars are backup-excluded via the directory flag, and confirm stores stay accessible after first unlock under the protection class (never inaccessible mid-sync after lock); and (b) the pre-existing **Story 14.3 iOS badge break** discovered while iOS-compile-checking: `set_badge_count` calls `WebviewWindow::set_badge_count`, which is `#[cfg(desktop)]`-only in tauri 2.11.5, so `cargo check --target aarch64-apple-ios` fails and the Story 12.5 iOS CI gate has been red on `main` since 14.3 — not this story's fix, flagged for the coordinator.

**Acceptance Criteria:**
- Given the iOS platform and freshly created stores, when `AccountManager::new` runs and an account is activated, then `exclude_from_backup` is invoked for the `data_dir` root directory and each `accounts/<ulid>/sdk` directory; directory-level exclusion covers `keeper.db`, `archive.db`, and every `-wal`/`-shm` sidecar. The host suite proves the invocations via the spy `Platform` (real device read-back is the SM-8 bar).
- Given a backup-exclusion failure on any platform, when a store is created or an account activated, then the failure is logged and non-fatal: the store/account is still usable, the app does not panic, and neither login, session-restore, nor the archive path is aborted.
- Given the desktop platform, when `exclude_from_backup` is called for any path, then it is a no-op returning `Ok(())` and no store creation is affected.
- Given the shipped iOS build config, when the default-data-protection value is read, then it is exactly `NSFileProtectionCompleteUntilFirstUserAuthentication` and never `NSFileProtectionComplete`; the value lives in `project.yml` `entitlements.properties` and the checked-in `keeper_iOS.entitlements`, and survives `xcodegen generate` (test-pinned).
- Given the whole change, when `bun run check:rust` and `bun run test:rust` run on host, then rustfmt/clippy (`-D warnings`) are clean and all new tests pass; the sole `unsafe` in the tree is the single `#[allow(unsafe_code)]` `exclude_from_backup` FFI function, `// SAFETY:`-documented and present in the audit inventory.
- Given the Story 13.7 disclosure ("excluded from device backup"), when 14.7 ships, then the code fulfills that promise for all stores under `data_dir` (including sidecars) and the disclosure copy remains accurate and unchanged.

## Spec Change Log

### 2026-07-11 — bad_spec loopback (review pass 1)
- **Triggering findings (both reviewers):** (1) *WAL/SHM sidecar leak* — the spec specified flagging `keeper.db`/`archive.db` as individual **files**, so their `-wal`/`-shm` sidecars (which hold committed-but-uncheckpointed message/registry rows under SQLite WAL mode) were not backup-excluded, defeating FR-65 for exactly the data it protects. (2) *Fatal error policy* — the I/O matrix mandated `?`-propagation, which at `AppState::new` became an `expect()` that panics the app at launch on any (retriable) exclusion failure and lets the archive path abort startup, contradicting the "archive never aborts startup" invariant.
- **Amended (outside `<intent-contract>`):** Boundaries/I-O-matrix/Code-Map/Tasks/Design-Notes now require flagging the **containing directory** — the `data_dir` root (covers both top-level DBs + their sidecars + the `accounts/` subtree) and each `sdk` directory — so sidecars are covered; and require exclusion failures to be **logged and non-fatal** (never panic/abort startup/login/restore/archive). Also corrected the entitlement source-of-truth: XcodeGen **regenerates** `keeper_iOS.entitlements` from `project.yml`, so the value must live in `project.yml` `entitlements.properties` mirrored into the checked-in file (test-pinned).
- **Known-bad state avoided:** uncommitted message/registry rows in `*-wal` reaching iCloud/iTunes backup; a launch-time panic / archive-invariant violation over a best-effort backup flag; a regenerated-away entitlement silently reverting to `<dict/>`.
- **KEEP (must survive re-derivation):** the single audited `#[allow(unsafe_code)]` `IosPlatform::exclude_from_backup` FFI is correct — objc2 `NSString::from_str` → `NSURL::fileURLWithPath` → `NSNumber::new_bool(true)` → `setResourceValue_forKey_error(Some(value), NSURLIsExcludedFromBackupKey)`, features `NSURL`/`NSError`/`NSString`/`NSValue`, cfg-gated, with the accurate `// SAFETY:` comment (obligation = value is a boolean `NSNumber`, satisfied). KEEP the dual-location entitlement (`project.yml` + file) with its explanatory comment and the exact-value/never-`Complete` host tests. KEEP the cfg-free `Platform` port method, `PlatformError::BackupExclusion`, `DesktopPlatform` no-op + test, the spy `FakePlatform` recorder + invocation tests, the central `activate()` funnel covering every session-restore site, the audit-inventory doc update, and the deferred-work SM-8 + pre-existing 14.3 iOS-badge-break entries. Zero new crates (objc2 already transitive).

## Review Triage Log

### 2026-07-11 — Review pass 1
- intent_gap: 0
- bad_spec: 2: (high 0, medium 2, low 0)
- patch: 0
- defer: 1: (medium 1)
- reject: 7: (low 7)
- addressed_findings:
  - `[medium]` `[bad_spec]` `keeper.db`/`archive.db` `-wal`/`-shm` sidecars not backup-excluded (file-level flag misses sibling files) → spec now mandates directory-level flagging (`data_dir` root + each `sdk` dir) so the subtree incl. sidecars is covered; code reverted and re-derived.
  - `[medium]` `[bad_spec]` fail-closed `?`/`expect` panics the app at launch and lets the archive path abort startup (invariant contradiction) → spec now mandates logged, non-fatal exclusion failures; code reverted and re-derived.
- deferred: pre-existing Story 14.3 iOS `set_badge_count` uses the `#[cfg(desktop)]`-only `WebviewWindow::set_badge_count` (tauri 2.11.5) → the iOS-target build / Story 12.5 CI gate has been red on `main` since 14.3; not this story's fix (re-ledgered in deferred-work.md during re-derivation; flagged for the coordinator).
- rejected (noise / by-design / near-unreachable): SAFETY comment "spends words on the easy half" (accurate, no defect); `fileURLWithPath` extra stat (works; `_isDirectory` nicety folded into spec); non-UTF-8 `to_str()` None (app-container paths are ASCII, and now non-fatal); `keeper.db` flagged a few registry-reads late (sub-ms startup race, closed by the root-dir flag); "exactly-once" assertion trivially satisfied (fine as-is); `registry::db_path` `pub` asymmetry (harmless); desktop no-op accepts a relative path (no caller does; absolute-path precondition noted in spec).

### 2026-07-11 — Review pass 2 (post re-derivation)
- intent_gap: 0
- bad_spec: 0
- patch: 0
- defer: 0
- reject: 13: (high 0, medium 0, low 13)
- addressed_findings:
  - none
- Verification this pass: independently re-ran gates — `bun run check:rust` clean (rustfmt + clippy `-D warnings`), `bun run test:rust` **793/793 pass** (incl. the 7 new 14.7 tests), sole `unsafe` is the single audited `IosPlatform::exclude_from_backup` FFI. Two Opus reviewers (Blind Hunter adversarial + Edge Case Hunter) ran fresh on the full diff.
- Reviewer contradiction resolved on evidence, not vote: Edge Case Hunter claimed `NSURLIsExcludedFromBackupKey` is per-inode / non-recursive (so directory flagging would miss later-created sidecars); Blind Hunter (and Apple's documented backup-skip semantics, and the pass-1 amendment rationale) confirm a **directory**-level flag excludes the whole subtree incl. files created afterward. Design is correct; on-device read-back stays ledgered as SM-8. → recursion/ordering concerns rejected.
- rejected (13, all low — production correct, all gates green): directory-subtree-not-recursive claim (unsupported; Apple docs + SM-8 device bar cover it); `sdk`-dir flag runs before matrix-sdk materializes the dir (root flag already covers the subtree; matrix-sdk creates the dir; swallowed if it ever failed — no leak); `keeper.db` created by a registry open preceding `AccountManager::new` (covered by the root-dir subtree flag); `data_dir()` temp-dir fallback (pre-existing pathological startup path, best-effort); `create_dir_all(data_dir)` failure disables archiving silently (pre-existing archive-never-aborts-startup invariant, not this story); `objc2` direct dep unused (`unused_crate_dependencies` is allow-by-default and not enabled; spec-mandated; harmless); per-`sdk` flag redundant with root flag (intentional SM-8 read-back target); exports under `data_dir` also excluded (exports land under a user-chosen `dest_root` **outside** `data_dir`); `add_account` fresh-login exclusion invocation + non-fatal login lack a **dedicated** test (spec's actual test reqs — sdk-dir invocation + non-fatal — are met via the identical shared `exclude_from_backup_best_effort` helper on the fully-tested `activate` path; a dedicated `add_account` offline test needs disproportionate mock-homeserver scaffolding absent from the crate, for a one-line inspection-verified call); `to_ipc_error` `BackupExclusion` arm is unreachable dead code (harmless defensive mapping); entitlements-test parsers give confusing messages on malformed/reformatted plist/YAML (no false-pass — xcodegen output is deterministic; ECH confirmed misdiagnosis-only); sign_out+re-add-with-new-ulid exclusion state untested (beyond spec scope; re-activation idempotency IS tested); no metric/counter for silent exclusion failures (by-design — spec mandates `warn`-level, best-effort); desktop no-op accepts a relative path with no `debug_assert` (already rejected pass 1).

## Design Notes

**The conflict is already resolved — do not re-litigate it.** The prior run blocked because `isExcludedFromBackup` has no safe binding and the workspace bans `unsafe`. The coordinator decided Option 1 (2026-07-11): a narrowly-scoped audited `unsafe` FFI exception in the `keeper` shell crate. That amendment is recorded in `docs/project-context.md:55-61` and the audit inventory. Implement it; the only job here is to do the FFI cleanly and keep it contained.

**Flag directories, not bare `.db` files — sidecars are the trap.** `NSURLIsExcludedFromBackupKey` set on a directory URL excludes that directory and its whole subtree from backup (Apple's documented "do not back up" mechanism). Set on a bare `keeper.db` file it excludes only that inode — the sibling `keeper.db-wal` / `keeper.db-shm` files are **not** covered, and under SQLite WAL mode `-wal` holds committed-but-uncheckpointed rows (message previews, registry). A backup taken between checkpoints would then capture exactly the data FR-65 exists to keep out of iCloud/iTunes. So flag the **containing directory**: the `data_dir` root (one call, covers `keeper.db` + `archive.db` + their sidecars + the whole `accounts/` subtree) and each `accounts/<ulid>/sdk` directory (covers its own store + sidecars; the per-account read-back target). The first review pass shipped file-level flags for the two top-level DBs and leaked their sidecars — this is the corrected design.

**Exclusion is best-effort — never brick the app over a backup attribute.** The flag is a privacy-hardening measure, not a correctness precondition. A failure must be logged and swallowed: bricking launch (via `expect`) or aborting login/restore over a backup attribute is far worse than a rare unexcluded store, and the archive path is invariant-bound to *never* abort startup — so `?`-propagating an exclusion error out of it (into a startup `expect`) is a direct contradiction. On a valid app-container path (always ASCII, always existing) the iOS setter effectively never fails; the `warn`-and-continue path exists for defensive robustness, not an expected case.

**Entitlement source of truth is `project.yml`, not the entitlements file.** XcodeGen **regenerates** `keeper_iOS.entitlements` from `project.yml` on every `xcodegen generate` — putting the value only in the entitlements file silently reverts it to `<dict/>` on the next regeneration (caught the first time by the value test). Put it in `project.yml` under `entitlements.properties` and mirror it into the checked-in file; a host test pins both.

**iOS FFI sketch (finalize against the real objc2 API):**
```rust
#[cfg(target_os = "ios")]
fn exclude_from_backup(&self, path: &Path) -> Result<(), CoreError> {
    use objc2_foundation::{NSNumber, NSString, NSURL, NSURLIsExcludedFromBackupKey};
    let s = NSString::from_str(path.to_str().ok_or_else(|| /* CoreError::Platform */ )?);
    let url = unsafe { NSURL::fileURLWithPath(&s) };
    let yes = NSNumber::new_bool(true);
    // SAFETY: NSURLIsExcludedFromBackupKey is an Apple-documented resource key; the
    // setter is safe for a valid file URL and NSNumber value we both own and outlive
    // the call. The path exists (caller invokes only after the store is created).
    #[allow(unsafe_code)]
    unsafe { url.setResourceValue_forKey_error(Some(&yes), NSURLIsExcludedFromBackupKey) }
        .map_err(|e| /* CoreError::Platform(PlatformError::…(e.to_string())) */ )?;
    Ok(())
}
```

**File protection is declarative.** `NSFileProtectionCompleteUntilFirstUserAuthentication` is Apple's recommended baseline and requires no code — the `default-data-protection` entitlement applies it to every file the app writes. `Complete` is banned because it makes files unreadable while the device is locked, which would stall a resumed sync loop's WAL access (the epic's explicit reason).

**Verification honesty.** The iOS `impl` body is `#[cfg(target_os = "ios")]`, so host clippy/tests never compile it — the FFI compiles only under `--target aarch64-apple-ios` (Story 12.5 CI gate) and the real NSURL read-back only runs on device/Simulator (SM-8). Host verification therefore proves everything except the FFI syscall: the port method, the desktop no-op, the per-directory invocation + idempotency + non-fatal behavior (spy `Platform`), and the entitlement value. The real subtree-exclusion-covers-sidecars behavior is confirmed on device (SM-8).

## Verification

**Commands:**
- `bun run check:rust` -- expected: `cargo fmt --check` clean + `clippy --all-targets -- -D warnings` clean (compiles desktop no-op + keeper-core wiring; the sole `#[allow(unsafe_code)]` is accepted).
- `bun run test:rust` -- expected: new spy-`Platform` exclusion tests, desktop no-op test, and the entitlements-value test all pass under nextest.
- `bun run check` -- expected: unchanged TS/JS gate stays green (no required frontend change; 13.7 disclosure untouched).
- `cargo check -p keeper --target aarch64-apple-ios` (from `src-tauri/`) -- expected: the iOS FFI block compiles **if** the iOS target is installed; otherwise this is covered by the Story 12.5 CI gate (note as CI/device-bound, not a local blocker).

**Manual checks (SM-8 / device-bound, not story-blocking):**
- On Simulator/device: after login and archive creation, read `NSURLIsExcludedFromBackupKey` back on the `data_dir` root and each `accounts/<ulid>/sdk` dir — each true — and confirm `keeper.db`/`archive.db` plus their `-wal`/`-shm` sidecars are backup-excluded via the root directory flag.
- Lock the device mid-sync: stores remain accessible (protection class is UntilFirstUserAuthentication, not Complete) and the resumed sync loop keeps working. Ledgered as SM-8 dogfooding.

## Auto Run Result

Status: done

### Summary
Re-derived and shipped Story 14.7 after the pass-1 `bad_spec` loopback (the run had been interrupted mid-loopback: spec amended + code reverted, re-derivation pending). Added a cfg-free `Platform::exclude_from_backup(&Path)` port with a log-and-continue `exclude_from_backup_best_effort` funnel; a desktop no-op impl and the codebase's single audited `#[allow(unsafe_code)]` iOS NSURL FFI (`objc2-foundation`) behind the port. Wired it to flag the `data_dir` **root directory** once (covers `keeper.db`/`archive.db` + their `-wal`/`-shm` sidecars + the `accounts/` subtree) and each `accounts/<ulid>/sdk` **directory** on both fresh login and every session-restore. Set `com.apple.developer.default-data-protection = NSFileProtectionCompleteUntilFirstUserAuthentication` in `project.yml` (XcodeGen source of truth) mirrored into `keeper_iOS.entitlements`, test-pinned.

### Files changed
- `src-tauri/crates/keeper-core/src/platform.rs` — `Platform::exclude_from_backup` trait method (cfg-free) + `exclude_from_backup_best_effort` non-fatal funnel.
- `src-tauri/crates/keeper-core/src/error.rs` — `PlatformError::BackupExclusion(String)` (→ `CoreError::Platform`).
- `src-tauri/crates/keeper/src/ipc.rs` — `DesktopPlatform` no-op; `IosPlatform` FFI (single function-level `#[allow(unsafe_code)]` + `// SAFETY:`); `AppState::new` threads platform into `AccountManager::new`; `BackupExclusion` arm in `to_ipc_error`; desktop no-op test.
- `src-tauri/crates/keeper-core/src/account.rs` — `AccountManager::new(platform, data_dir)` flags root; `activate()` flags each `sdk` dir (fresh + restore); spy `FakePlatform` recorder + 4 tests (invocation, non-fatal ×2, idempotent re-flag).
- `src-tauri/crates/keeper-core/src/auth.rs` — `add_account` flags fresh `sdk_dir` post-`.build()` (non-fatal); test doubles updated.
- `src-tauri/crates/keeper-core/src/{notify.rs,inbox.rs,bridges/health.rs}` + `tests/archive_survives_sign_out.rs` — trait method on test doubles.
- `src-tauri/Cargo.toml` + `crates/keeper/Cargo.toml` + `Cargo.lock` — `objc2`/`objc2-foundation` under `[target.'cfg(target_os = "ios")'.dependencies]` (zero new crate versions; both already transitive).
- `src-tauri/crates/keeper/gen/apple/{project.yml,keeper_iOS/keeper_iOS.entitlements,keeper.xcodeproj/project.pbxproj}` — entitlement value (dual-location + regenerated via xcodegen 2.45.4).
- `src-tauri/crates/keeper/tests/entitlements_protection.rs` — NEW: structural exact-value pin + never-`NSFileProtectionComplete` on both files.
- `docs/constraints-and-limitations.md` — audit-inventory entry names the shipped `IosPlatform::exclude_from_backup` location.
- `_bmad-output/implementation-artifacts/deferred-work.md` — 14.7 SM-8 device bars + pre-existing 14.3 iOS badge break.

### Review findings breakdown
- Pass 2 (this pass): intent_gap 0, bad_spec 0, patch 0, defer 0, reject 13 (all low). No review-driven code changes. Two Opus reviewers (Blind Hunter + Edge Case Hunter) ran fresh on the full diff; a reviewer contradiction on directory-exclusion recursion was resolved on evidence (Apple's documented directory-subtree backup-skip semantics + the pass-1 amendment rationale). See Review Triage Log pass 2 for the full rejection rationale.
- Pass 1 (prior): 2 bad_spec (WAL/SHM sidecar leak; fatal-error policy) → spec amended, code re-derived. 1 defer (14.3 iOS badge break). 7 reject.

### Verification
- `bun run check:rust` — PASS (rustfmt `--check` clean; clippy `--all-targets -- -D warnings` clean).
- `bun run test:rust` — PASS, 793/793 (incl. 7 new 14.7 tests: root flag, root non-fatal, activate flags sdk dir + idempotent re-flag, activate non-fatal, desktop no-op, 2 entitlement pins).
- `bun run check` — PASS (subagent; no frontend change; 13.7 disclosure untouched and accurate).
- `cargo check -p keeper --target aarch64-apple-ios` — new FFI compiles clean; the only error is the pre-existing Story 14.3 `WebviewWindow::set_badge_count` `#[cfg(desktop)]` break (ledgered, red-since-14.3 12.5 gate).
- Sole `unsafe` in the tree = the single `IosPlatform::exclude_from_backup` FFI, `// SAFETY:`-documented, in the audit inventory (grep-verified).

### Follow-up review recommendation
`false` — this pass made no review-driven changes; the implementation was accepted as-is.

### Residual risks
- iOS on-device NSURL read-back + lock-screen WAL access are the SM-8 device bars (host tests cannot compile the `#[cfg(target_os = "ios")]` FFI). Ledgered in deferred-work.md.
- The Story 12.5 iOS CI gate stays red until the pre-existing 14.3 `set_badge_count` break is fixed (not this story; ledgered for the coordinator).
