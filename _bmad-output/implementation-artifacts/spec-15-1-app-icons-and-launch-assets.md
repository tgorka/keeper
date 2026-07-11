---
title: 'App Icons and Launch Assets (iOS)'
type: 'feature'
created: '2026-07-11'
status: 'done'
baseline_revision: 'ec6f9c7fd74e090faf2f976c27ed148245bc0b47'
final_revision: 'dd1a40b1f25c574cac5dd98673c21003f2847edd'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
warnings:
  - oversized
---

<intent-contract>

## Intent

**Problem:** The iOS build still carries generic Tauri-default app icons (rasterized Rust logo) and a launch screen wired to iOS `systemBackgroundColor` — which paints pure white/black and does not match keeper's themed `--background` (`#0A0A0A` in dark), producing a black→dark-grey flash on cold launch. The phone build reads as sideloaded scaffolding, not a finished product.

**Approach:** Replace all 18 asset-catalog icon images with an opaque, keeper-branded set generated reproducibly from a committed master, and re-point the launch screen at a theme-aware color set (`#FFFFFF` light / `#0A0A0A` dark) so the native launch background matches the app's first webview paint in both appearances with no flash on launch or rotation. All edits land only in committed, regeneration-safe locations (asset catalogs + `LaunchScreen.storyboard`), leaving the desktop build untouched.

## Boundaries & Constraints

**Always:**
- All persistent edits go only in the committed asset catalog (`gen/apple/Assets.xcassets/**`), `gen/apple/LaunchScreen.storyboard`, and (if needed) `gen/apple/project.yml` — never in the generated `.xcodeproj`. Run `xcodegen generate` and confirm the assets/launch config survive regeneration (AD-32).
- App icons must be opaque (no alpha channel), full-bleed squares with no pre-baked rounded corners, keeper-branded (keeper-green field `#0F6E5C`), and clearly distinct from the current Tauri-default images. The 1024×1024 marketing icon must have no alpha.
- The 18 output filenames and pixel dimensions must exactly match the existing `AppIcon.appiconset/Contents.json` mapping (see Design Notes table); do not rename or re-key the catalog.
- Launch background must equal the sRGB rendering of the app's `--background` token — light `#FFFFFF`, dark `#0A0A0A` — via a color set with Any + Dark appearances. Leave `UIUserInterfaceStyle` unset (Automatic) so the launch screen follows system appearance, matching next-themes' `defaultTheme="system"`.
- Icon generation must be reproducible with tooling already on the machine (Swift toolchain + `sips`); commit the generator and master so the set can be regenerated. No new third-party build dependencies, no network fetches.
- Keep desktop assets and bundling untouched: do not modify `src-tauri/crates/keeper/icons/**` or `tauri.conf.json`'s `bundle.icon`.

**Block If:**
- The keeper brand mark cannot be produced within these constraints without a human design decision that changes product identity (e.g. a required logo asset that does not exist and cannot be derived from the documented brand colors). A simple, legible in-repo geometric mark on the keeper-green field is acceptable and does NOT trigger a block.

**Never:**
- No on-device install/visual sign-off here — final rendering on home screen/Settings/switcher and the flash-free launch are visually accepted on the owner's iPhone in Story 15.6. This story delivers and self-verifies the assets via CLI.
- No changes to Rust/TS app logic, theme system, `next-themes` config, entitlements, bundle id, or deployment target.
- No team ids, signing material, or secrets added anywhere.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Cold launch, system light | Device in light appearance | Launch background `#FFFFFF`; continuous into white webview paint, no flash | n/a |
| Cold launch, system dark | Device in dark appearance | Launch background `#0A0A0A`; continuous into dark webview paint, no black flash | n/a |
| Rotation during launch | Splash visible, device rotates | Full-bleed background fills all orientations; no white/black edge flash | Autoresizing mask covers rotation |
| Icon render on all surfaces | Installed app | Branded icon on home screen, Settings, app switcher, Spotlight | Missing/wrong-size image → asset-catalog compile fails (caught in verification) |
| `xcodegen generate` re-run | After edits | Icons, color set, launch config all still referenced; no reversion | Divergence → treat as failure |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper/gen/apple/Assets.xcassets/AppIcon.appiconset/` -- 18 committed icon PNGs (Tauri defaults) + `Contents.json` (filename→size map, unchanged).
- `src-tauri/crates/keeper/gen/apple/Assets.xcassets/LaunchBackground.colorset/` -- NEW color set (Any=`#FFFFFF`, Dark=`#0A0A0A`) for the launch background.
- `src-tauri/crates/keeper/gen/apple/LaunchScreen.storyboard` -- currently binds view `backgroundColor` to `systemBackgroundColor`; re-point at named color `LaunchBackground`.
- `src-tauri/crates/keeper/gen/apple/project.yml` -- XcodeGen source of truth; `UILaunchStoryboardName: LaunchScreen` already set. No change expected (do not set `UIUserInterfaceStyle`).
- `scripts/gen-ios-icons.swift` (+ master) -- NEW committed generator producing the branded, opaque icon set; design/regeneration source of truth.
- `src/index.css` -- reference only: `--background` light `oklch(1 0 0)`→`#FFFFFF`, dark `oklch(0.145 0 0)`→`#0A0A0A`; `body` uses `bg-background` so first paint equals launch color.

## Tasks & Acceptance

**Execution:**
- [x] `scripts/gen-ios-icons.swift` -- add a committed Swift/CoreGraphics generator (run via `swift`) that draws an opaque keeper-green (`#0F6E5C`) full-bleed icon with a simple centered white mark (no text, no alpha, no rounded corners) and emits the 18 exact filenames at the exact pixel sizes in the Design Notes table into `AppIcon.appiconset/`. Commit any master it needs. Rationale: reproducible, dependency-free branding that survives regeneration.
- [x] `gen/apple/Assets.xcassets/AppIcon.appiconset/*.png` -- regenerate all 18 images via the generator, overwriting the Tauri defaults; keep `Contents.json` unchanged. Rationale: satisfy FR-55 icon set on every surface.
- [x] `gen/apple/Assets.xcassets/LaunchBackground.colorset/Contents.json` -- add color set with universal (light) sRGB `1,1,1` and a `dark` appearance sRGB `0.039,0.039,0.039` (`#0A0A0A`). Rationale: theme-matched launch background.
- [x] `gen/apple/LaunchScreen.storyboard` -- replace the `systemBackgroundColor` view background with named color `LaunchBackground` (add matching `<namedColor>` resource); keep full-bleed autoresizing so rotation shows no flash. Rationale: FR-59 flash-free launch in light & dark.
- [x] `gen/apple/project.yml` -- verify launch/asset config only; run `xcodegen generate` and confirm no reversion. Do not add `UIUserInterfaceStyle`. Rationale: AD-32 regeneration safety.

**Acceptance Criteria:**
- Given the regenerated asset catalog, when the 18 icon files are inspected, then each matches its required pixel dimensions, has no alpha channel, is not byte-identical to the prior Tauri default, and its background is keeper-green — and the marketing 1024×1024 image has no alpha.
- Given `LaunchScreen.storyboard` and `LaunchBackground.colorset`, when the launch config is inspected, then the launch view background resolves to `#FFFFFF` in light and `#0A0A0A` in dark, and `UIUserInterfaceStyle` remains unset (Automatic).
- Given AD-32, when `xcodegen generate` re-runs, then the icons, color set, and launch storyboard binding all remain in place (no reversion) and `cargo check --target aarch64-apple-ios` still compiles.
- Given the desktop build, when the diff is reviewed, then no files under `src-tauri/crates/keeper/icons/` or `tauri.conf.json` changed and desktop/frontend quality gates stay green.

## Spec Change Log

_No bad_spec loopbacks — the spec was implemented as written; review produced only patch-level hardening (see Review Triage Log)._

## Review Triage Log

### 2026-07-11 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 7: (high 0, medium 0, low 7)
- defer: 0
- reject: 7
- addressed_findings:
  - `[low]` `[patch]` Storyboard declared a `System colors in document resources` capability that was orphaned once the `systemColor` was removed — swapped it for the `Named colors` capability the file now actually uses.
  - `[low]` `[patch]` Icon generator wrote to a hardcoded relative `outDir` with no existence check — added a guard that fails loudly instead of silently creating a stray `AppIcon.appiconset` at the wrong path.
  - `[low]` `[patch]` Generator drew in `DeviceRGB` (untagged) — switched to explicit sRGB so the brand color is color-managed/exact and consistent with the sRGB `LaunchBackground` colorset/storyboard; regenerated all 18 icons (now tagged `sRGB IEC61966-2.1`).
  - `[low]` `[patch]` Generator asserted no-alpha/dimensions only via the header comment — added a `verifyPNG` self-check that re-reads every written file and fails the run on wrong size or any alpha (tripwire for the App-Store opaque-icon rule).
  - `[low]` `[patch]` Removed dead `interpolationQuality = .high` (nothing is resampled; the mark is drawn as vectors at each size).
  - `[low]` `[patch]` Documented the `#0F6E5C` brand-green source (mirrors the app `--primary` token; a native generator cannot import the CSS value).
  - `[low]` `[patch]` Added a `project.yml` comment pinning the load-bearing invariant that `UIUserInterfaceStyle` must stay unset (system-following launch theming), mirroring the existing entitlements-comment convention.
- rejected (noise/idiomatic/refuted): forced `AppIcon-512@2x` filename (dictated by the pre-existing `Contents.json`); byte-reproducibility CI check (committed assets are the source of truth by design); small-size mark legibility (refuted — pixel sampling shows a consistent 37–39% white glyph at 20/29/80 px; on-device visual acceptance is Story 15.6); colorset universal+dark Any/Dark structure (idiomatic Xcode); storyboard inline `<namedColor>` white fallback (idiomatic IB hint, catalog is the runtime source); write-only stale-file concern (theoretical, set matches the catalog).

## Design Notes

**Icon filename → pixel-size map** (from `Contents.json`; produce exactly these):

| File | px | File | px |
|------|----|------|----|
| AppIcon-20x20@1x.png | 20 | AppIcon-40x40@2x.png | 80 |
| AppIcon-20x20@2x.png | 40 | AppIcon-40x40@2x-1.png | 80 |
| AppIcon-20x20@2x-1.png | 40 | AppIcon-40x40@3x.png | 120 |
| AppIcon-20x20@3x.png | 60 | AppIcon-60x60@2x.png | 120 |
| AppIcon-29x29@1x.png | 29 | AppIcon-60x60@3x.png | 180 |
| AppIcon-29x29@2x.png | 58 | AppIcon-76x76@1x.png | 76 |
| AppIcon-29x29@2x-1.png | 58 | AppIcon-76x76@2x.png | 152 |
| AppIcon-29x29@3x.png | 87 | AppIcon-83.5x83.5@2x.png | 167 |
| AppIcon-40x40@1x.png | 40 | AppIcon-512@2x.png | 1024 |

**Storyboard color binding** (golden snippet — swap the `<color>` line and add a `<namedColor>` resource):
```xml
<color key="backgroundColor" name="LaunchBackground"/>
<!-- in <resources>: -->
<namedColor name="LaunchBackground">
  <color red="1" green="1" blue="1" alpha="1" colorSpace="custom" customColorSpace="sRGB"/>
</namedColor>
```
The Dark variant lives in the color set, not the storyboard; iOS resolves the named color per active appearance.

**Known platform limitation (document honestly, do not fix here):** a native launch screen follows the *system* appearance and cannot read a user's persisted next-themes override. In the default `system` theme it matches perfectly; if a user forces light-in-dark (or vice versa), a single-frame mismatch is possible — an inherent iOS constraint, not a defect. This matches the epic's honesty posture and is out of scope for 15.1.

**Mark guidance:** keep the glyph simple and legible at 40 px (a messenger/keep motif in white on the green field). Exact glyph is at the dev's discretion within the constraints; branding is verified by opacity + green field + divergence from defaults, not by a subjective design bar.

## Verification

**Commands:**
- `cd src-tauri/crates/keeper/gen/apple && for f in Assets.xcassets/AppIcon.appiconset/*.png; do sips -g pixelWidth -g pixelHeight -g hasAlpha "$f"; done` -- expected: each file's dimensions match the Design Notes map and `hasAlpha: no`.
- `cd src-tauri/crates/keeper/gen/apple && xcodegen generate` -- expected: succeeds; `git status` shows no reversion of icons, `LaunchBackground.colorset`, or the storyboard binding.
- `cd src-tauri && cargo check --workspace --target aarch64-apple-ios` -- expected: compiles (matches the CI iOS gate; confirms no project breakage).
- `git diff --name-only` -- expected: no paths under `src-tauri/crates/keeper/icons/` and no change to `tauri.conf.json`.
- `bun run check` -- expected: biome + tsc + vitest green (desktop/frontend unaffected).

**Manual checks (if no CLI):**
- Open `LaunchScreen.storyboard` and `LaunchBackground.colorset` and confirm the launch view resolves to `#FFFFFF` (light) / `#0A0A0A` (dark); on-device visual confirmation of icons and flash-free launch is performed in Story 15.6.

## Auto Run Result

**Status:** done

**Summary:** Replaced the generic Tauri-default iOS app icons with an opaque, keeper-branded set (green `#0F6E5C` field, white speech-bubble/"keep" mark) generated by a committed, dependency-free Swift/CoreGraphics script, and made the launch screen theme-aware (`LaunchBackground` color set, light `#FFFFFF` / dark `#0A0A0A`, matching the app's `--background`) so cold launch has no white/black flash in either appearance. `UIUserInterfaceStyle` stays unset so the launch follows system appearance. All edits live in regeneration-safe committed locations; the desktop build is untouched.

**Files changed:**
- `scripts/gen-ios-icons.swift` — new reproducible icon generator (sRGB, opaque, self-verifying; 18 exact filenames/sizes).
- `src-tauri/crates/keeper/gen/apple/Assets.xcassets/AppIcon.appiconset/*.png` — 18 regenerated branded, opaque, sRGB icons.
- `src-tauri/crates/keeper/gen/apple/Assets.xcassets/LaunchBackground.colorset/Contents.json` — new theme-aware launch color (light/dark).
- `src-tauri/crates/keeper/gen/apple/LaunchScreen.storyboard` — background re-pointed to `LaunchBackground`; `Named colors` capability.
- `src-tauri/crates/keeper/gen/apple/project.yml` — comment pinning the `UIUserInterfaceStyle`-unset launch-theming invariant (no functional change).

**Review findings:** 0 intent_gap, 0 bad_spec, 7 patches applied (all low: storyboard capability fix, generator outDir guard, sRGB color space, post-write no-alpha/size self-check, dead-config removal, brand-source comment, project.yml invariant comment), 0 deferred, 7 rejected (idiomatic/refuted/out-of-scope).

**Verification:**
- All 18 icons: exact required dimensions, `hasAlpha: no`, sRGB-tagged, keeper-green field — confirmed by the generator's self-check and an independent `sips`/PNG-decode pass (bad=0).
- `xcodegen generate`: succeeds (given the Tauri build-time `assets/` dir) with **no reversion** of the storyboard binding, `Named colors` capability, colorset, or icons, and no `.xcodeproj` churn.
- Desktop/Rust/TS untouched: no paths under `src-tauri/crates/keeper/icons/`, no `tauri.conf.json` change, no `.rs`/`.ts`/`.tsx` change vs baseline; frontend gates (`bun run check`, 1245 tests) were green and are unaffected.

**Follow-up review recommended:** false — the review changes are localized, low-consequence quality hardening (a dev tool, one storyboard line, comments) with no user-facing behavior/API/security/data impact, self-verified by the new tripwire and independent checks.

**Residual risks:**
- On-device visual acceptance (icons on home screen/Settings/switcher; flash-free launch light & dark) is deferred to Story 15.6 per the epic — this story delivers and CLI-verifies the assets only.
- Native launch screens follow system appearance; a user who forces an in-app theme opposite the system can see a one-frame launch mismatch — an inherent iOS limitation, documented, not a defect.
- `xcodegen generate` from a clean tree requires the Tauri build-time `assets/` resource dir to exist first (pre-existing `project.yml` layout from Story 12.1; untracked/build-created) — not caused by this story.
- The iOS CI gate (`cargo check --target aarch64-apple-ios`) remains red due to the pre-existing `set_badge_count` break in `ipc.rs` (red on `main` since Story 14.3, already in the deferred-work ledger) — independent of this asset-only story, which changes no Rust.
