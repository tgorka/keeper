---
status: ready-for-dev
story: 14-7-backup-exclusion-and-file-protection
---

# BMad Dev Auto Result

Status: blocked
Blocking condition: `isExcludedFromBackup` cannot be set without an `unsafe` FFI call, which the workspace-wide `unsafe_code = "deny"` invariant forbids — a hard-invariant vs. requirement conflict that needs a human architectural decision.

## Summary

Story 14.7 requires that every keeper database directory on iOS (matrix-sdk/crypto stores under `accounts/<ulid>/sdk`, `keeper.db`, `archive.db`) **(a)** carries the `isExcludedFromBackup` resource flag — *verified by reading the resource value back in a test* — and **(b)** uses file protection `NSFileProtectionCompleteUntilFirstUserAuthentication` (asserted in code; never `Complete`). All state must stay under the single `Platform::data_dir()` root (AD-29), and the Story 13.7 disclosure must match the actual flagging.

Investigation shows requirement **(b)** has a clean, safe implementation path, but requirement **(a)** has **no safe implementation path** in the current dependency tree and collides with a documented hard project invariant. Because both viable resolutions override an invariant or add substantial new infrastructure, this is a Block-If, not an unattended implementation choice.

## The conflict (evidence)

- **Hard invariant.** `unsafe_code = "deny"` is a workspace lint (`src-tauri/Cargo.toml:13-14`, `[workspace.lints.rust]`) that **both** crates opt into via `[lints] workspace = true` (`crates/keeper-core/Cargo.toml:10-11`, `crates/keeper/Cargo.toml:10-11`). `docs/project-context.md` states it absolutely: *"`unsafe_code = "deny"` — no unsafe blocks, ever."* There is currently **zero `unsafe`** anywhere in the codebase (grep of `crates/keeper/src` is empty).
- **The only mechanism is `unsafe`.** Setting `isExcludedFromBackup` requires `NSURL setResourceValue:forKey:NSURLIsExcludedFromBackupKey`. The only in-tree binding is `objc2-foundation 0.3.2`, where the setter is `pub unsafe fn setResourceValue_forKey_error` (`objc2-foundation-0.3.2/src/generated/NSURL.rs:1405-1407`) — an `unsafe` call. `NSFileManager::setAttributes_ofItemAtPath_error` is likewise `pub unsafe fn` (`.../NSFileManager.rs:415-417`).
- **The safe wrapper Story 12.3 used does not cover this.** `security-framework 3.7.0` (how 12.3 set the Keychain protection class without `unsafe`) is Keychain-only; it exposes no NSURL resource-value or file-attribute API.
- **No entitlement equivalent for backup exclusion.** `isExcludedFromBackup` is strictly per-URL — there is no Info.plist/entitlement toggle. (By contrast, file protection *does* have one — see below.)
- **Precedent cuts against silently adding `unsafe`.** Story 12.3 (`spec-12-3-*.md`, the directly analogous iOS-FFI story) deliberately *avoided* `unsafe`, switching from keyring to security-framework and recording *"No unsafe"* as a design achievement. Introducing the codebase's first-ever `#[allow(unsafe_code)]` is an architectural precedent, not a routine choice — exactly the kind of decision the Block-If gate exists for.

## What is safely doable now (no decision needed)

- **File protection (requirement b) via entitlement.** The iOS entitlements file (`src-tauri/crates/keeper/gen/apple/keeper_iOS/keeper_iOS.entitlements`) is currently empty (`<dict/>`). Adding `com.apple.developer.default-data-protection = NSFileProtectionCompleteUntilFirstUserAuthentication` sets that class as the default for every file the app writes — no code, no `unsafe`, and it is Apple's recommended baseline. (Note AD-32: `gen/apple` is regenerated from `project.yml`/XcodeGen, so the entitlement must be pinned where regeneration preserves it, and the value should be grep-assertable for the "asserted in code/config" AC.)
- **The Platform-port seam (AD-29) is ready.** `Platform` trait at `crates/keeper-core/src/platform.rs:19-63`; iOS impl `IosPlatform` and desktop `DesktopPlatform` in `crates/keeper/src/ipc.rs` (iOS ~`:531-680`), constructed as `Arc<dyn Platform>` in `AppState::new()`. The natural design is a new port method (e.g. `exclude_from_backup(&Path)`) called from `keeper-core` at the DB-creation sites (`registry.rs:35`, `archive/db.rs:44`, and post-`sqlite_store` build in `auth.rs` / `account.rs`), a no-op on desktop, real on iOS. keeper-core stays `cfg`-free; only the iOS impl body is the sticking point.

## Resolution options (for `/bmad-loop-resolve`)

1. **(Recommended) Grant a narrowly-scoped, audited `unsafe` FFI exception in the shell crate.** Allow a single documented `#[allow(unsafe_code)]` iOS-only function in `crates/keeper` that sets `NSURLIsExcludedFromBackupKey` via `objc2-foundation`, behind the `Platform` port. Pair it with the `default-data-protection` entitlement for file protection (defense-in-depth). Smallest change, keeps everything behind the port (AD-29), unsafe is contained and reviewable. **Cost:** breaks the "no unsafe, ever" rule for platform FFI — needs an explicit policy amendment (e.g. scope the deny to `keeper-core` + business logic, permit audited FFI unsafe in the `keeper` shell crate).
2. **Micro Swift plugin behind the Platform port.** Implement backup exclusion in Swift (consistent with AD-30's "micro Swift plugin behind the same Rust entry" pattern), invoked through `IosPlatform`. No Rust `unsafe`. **Cost:** no Swift-plugin scaffold exists yet (14.1 used the webview `visibilitychange` stopgap, not a plugin); adds real infrastructure, and it is only verifiable on device/Simulator (SM-8 / Story 12.6), **not** by the `cargo check --target aarch64-apple-ios` gate — so the "verified by reading the resource value back in a test" AC becomes device-bound rather than host-testable.
3. **Re-scope the requirement.** If the epic owner accepts it, drop the per-URL `isExcludedFromBackup` and rely solely on the file-protection entitlement, or relocate re-syncable stores to an OS-default-excluded location — but `Library/Caches` risks OS purging of `keeper.db`/`archive.db` mid-use and contradicts the single `data_dir()` root, so this is the weakest option and likely unacceptable.

**Recommendation:** Option 1 with the entitlement for file protection. It is the least infrastructure, keeps the Platform-port architecture intact, and the only open question for the human is whether to formally permit contained, audited `unsafe` for iOS FFI in the shell crate.

## Investigation confidence

High. Both objc2-foundation setters were confirmed `pub unsafe fn` by reading the vendored crate source; the lint scope was confirmed across the workspace and both crate manifests; the entitlements file was read (empty); security-framework's Keychain-only surface and Story 12.3's deliberate no-unsafe posture were confirmed from the shipped spec. Version control was clean on `main` at start (correct branch for this automation session). No intent ambiguity in the story itself — the block is purely the invariant-vs-mechanism conflict.

## Coordinator resolution (2026-07-11): OPTION 1 — decided

Grant the narrowly-scoped, audited `unsafe` FFI exception in the `keeper` shell crate:
one iOS-only function setting `NSURLIsExcludedFromBackupKey` via objc2-foundation, exposed
through a new `Platform::exclude_from_backup(&Path)` port method (no-op on desktop), called
from keeper-core at the DB-creation sites. Pair with the
`com.apple.developer.default-data-protection = NSFileProtectionCompleteUntilFirstUserAuthentication`
entitlement (pinned so XcodeGen regeneration preserves it) for requirement (b).
Policy amendment is already recorded in docs/project-context.md (Rust Rules) and the audit
inventory in docs/constraints-and-limitations.md. Requirements: function-level
`#[allow(unsafe_code)]` only, `// SAFETY:` comment, unit-testable where host allows;
resource-value read-back verification may be marked device-bound if not host-testable.
Do NOT re-raise the invariant conflict — it is settled by this policy amendment.
