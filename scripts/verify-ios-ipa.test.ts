import { describe, expect, it } from "vitest";
import { FORBIDDEN, findForbiddenSymbols, parseForbiddenCrates } from "./verify-ios-ipa";

// Real `cargo tree -p keeper --target <t> -e normal` output uses box-drawing glyphs and
// `<crate> vX.Y.Z` lines. These fixtures reproduce the shapes the verifier must classify
// so the fail-closed branches — which a structurally-enforced seam never triggers in-repo
// — stay locked against regression.

const CLEAN_IOS_TREE = `keeper v0.1.0 (/repo/src-tauri/crates/keeper)
├── tauri v2.9.0
│   ├── tauri-plugin-deep-link v2.4.4
│   └── serde v1.0.0
└── keeper-core v0.1.0 (/repo/src-tauri/crates/keeper-core)`;

const DESKTOP_TREE = `keeper v0.1.0 (/repo/src-tauri/crates/keeper)
├── tauri v2.9.0
│   └── tray-icon v0.24.1
├── tauri-plugin-autostart v2.5.1
├── tauri-plugin-global-shortcut v2.3.2
├── tauri-plugin-process v2.3.1
└── tauri-plugin-updater v2.10.1`;

const LEAKED_IOS_TREE = `keeper v0.1.0 (/repo/src-tauri/crates/keeper)
├── tauri v2.9.0
└── tauri-plugin-updater v2.10.1`;

describe("parseForbiddenCrates", () => {
  it("finds no forbidden crates in a clean iOS tree", () => {
    expect(parseForbiddenCrates(CLEAN_IOS_TREE, FORBIDDEN).size).toBe(0);
  });

  it("finds all five forbidden crates in the desktop tree (differential control)", () => {
    const present = parseForbiddenCrates(DESKTOP_TREE, FORBIDDEN);
    expect(present.size).toBe(FORBIDDEN.length);
    for (const { crate } of FORBIDDEN) {
      expect(present.has(crate)).toBe(true);
    }
  });

  it("detects a leaked crate in the iOS tree", () => {
    const present = parseForbiddenCrates(LEAKED_IOS_TREE, FORBIDDEN);
    expect(present.has("tauri-plugin-updater")).toBe(true);
    expect(present.has("tauri-plugin-autostart")).toBe(false);
  });

  it("does not flag the cross-platform tauri-plugin-deep-link", () => {
    expect(parseForbiddenCrates(CLEAN_IOS_TREE, FORBIDDEN).has("tauri-plugin-deep-link")).toBe(
      false,
    );
    // deep-link is present in the clean tree yet is not in the forbidden set at all.
    expect(FORBIDDEN.some((f) => f.crate === "tauri-plugin-deep-link")).toBe(false);
  });

  it("does not match a crate whose name is only a substring of another line", () => {
    // `tauri-plugin-process` must not be matched by an unrelated `some-process-thing` line.
    const tree = `keeper v0.1.0\n└── some-process-helper v1.0.0`;
    expect(parseForbiddenCrates(tree, FORBIDDEN).has("tauri-plugin-process")).toBe(false);
  });
});

describe("findForbiddenSymbols", () => {
  it("returns nothing for a dump without forbidden symbols", () => {
    const dump = "_ZN5tauri3run17habc\n_ZN12tauri_plugin_deep_link8register";
    expect(findForbiddenSymbols(dump, FORBIDDEN)).toEqual([]);
  });

  it("matches a length-prefixed mangled Rust symbol (substring, not word-boundary)", () => {
    // Rust mangles as `_ZN27tauri_plugin_global_shortcut...` — a `\b` word boundary would
    // miss it, so the matcher must be a plain substring test.
    const dump = "_ZN27tauri_plugin_global_shortcut8Shortcut3new17h0000E";
    expect(findForbiddenSymbols(dump, FORBIDDEN)).toContain("tauri_plugin_global_shortcut");
  });

  it("reports every distinct forbidden symbol present", () => {
    const dump = "tray_icon::TrayIcon\n_ZN20tauri_plugin_updater5check";
    const found = findForbiddenSymbols(dump, FORBIDDEN);
    expect(found).toContain("tray_icon");
    expect(found).toContain("tauri_plugin_updater");
    expect(found).toHaveLength(2);
  });
});
