import { spawnSync } from "node:child_process";
import { lstat } from "node:fs/promises";

// tauri-action appends `build` and its configured arguments to tauriScript.
// It discovers/packages/uploads assets only after this runner exits successfully.
const args = process.argv.slice(2);
const expected = [
  "build",
  "--config",
  "src-tauri/crates/keeper/tauri.conf.json",
  "--target",
  "aarch64-apple-darwin",
];

function execute(command, commandArgs) {
  const result = spawnSync(command, commandArgs, { stdio: "inherit" });
  if (result.error || result.signal || result.status !== 0) {
    console.error("Release build or credential gate failed; uploads are blocked.");
    process.exit(result.status || 1);
  }
}

try {
  if (args.length !== expected.length || args.some((arg, i) => arg !== expected[i])) {
    throw new Error("Unsupported release build arguments");
  }
  execute("bun", ["run", "tauri", ...args]);
  const app = "src-tauri/target/aarch64-apple-darwin/release/bundle/macos/keeper.app";
  const executables = [`${app}/Contents/MacOS/keeper`, `${app}/Contents/MacOS/keeper-rec`];
  for (const executable of executables) {
    const entry = await lstat(executable);
    if (!entry.isFile() || entry.size === 0 || (entry.mode & 0o111) === 0) {
      throw new Error("Missing app executable");
    }
  }
  execute("bun", ["scripts/check-client-secrets.ts", "dist", app, ...executables]);
  console.info(
    "Pre-upload gate passed for frontend assets and uncompressed .app contents, including keeper-rec. Compressed DMG/tar archives are not scanned.",
  );
} catch {
  console.error(
    "Release credential gate could not inspect the required app executables; uploads are blocked.",
  );
  process.exitCode = 1;
}
