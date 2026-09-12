import { createReadStream } from "node:fs";
import { lstat, readdir } from "node:fs/promises";
import { basename, join, relative } from "node:path";

// Scan built output without ever printing matching bytes. Public phc_ ingestion
// tokens are intentionally allowed: they are not authorization credentials.
const patterns = [
  { name: "PostHog personal key", value: /phx_[A-Za-z0-9_-]{20,}/ },
  { name: "PostHog secure flag key", value: /phs_[A-Za-z0-9_-]{20,}/ },
  // Encoded JSON capsules distinguish service tokens from ordinary ops_ identifiers.
  { name: "1Password service credential", value: /ops_eyJ[A-Za-z0-9+/_-]{250,}={0,3}/ },
  { name: "GitHub credential", value: /(?:gh[pousr]_|github_pat_)[A-Za-z0-9_]{20,}/ },
  { name: "private key", value: /-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----/ },
  {
    name: "minisign private key",
    value: /untrusted comment: [^\r\n]{0,256}(?:secret|private) key/i,
  },
  { name: "Matrix access token", value: /syt_[A-Za-z0-9_-]{20,}/ },
];
const sentinel = "KEEPER_PRIVILEGED_KEY_MUST_NOT_SHIP_71";
// Match available privileged environment values too, including legacy 1Password
// sessions. Values and decoded updater material are never included in output.
const updaterKey = process.env.TAURI_SIGNING_PRIVATE_KEY;
const privateValues = [
  ...Object.entries(process.env)
    .filter(([name]) =>
      /^(?:OP_SERVICE_ACCOUNT_TOKEN|OP_CONNECT_TOKEN|POSTHOG_PERSONAL_API_KEY|GH_TOKEN|GITHUB_TOKEN)$|^OP_SESSION_/.test(
        name,
      ),
    )
    .map(([, value]) => value),
  updaterKey,
  ...(updaterKey ? [Buffer.from(updaterKey, "base64").toString("latin1")] : []),
].filter((value): value is string => typeof value === "string" && value.length >= 20);
const overlapBytes = Math.max(512, ...privateValues.map((value) => value.length));

async function scanFile(path: string): Promise<string | null> {
  let overlap = "";
  for await (const chunk of createReadStream(path, { highWaterMark: 64 * 1024 })) {
    const text = overlap + (chunk as Buffer).toString("latin1");
    if (text.includes(sentinel)) return "privileged-key build sentinel";
    if (privateValues.some((value) => text.includes(value)))
      return "privileged environment credential";
    for (const pattern of patterns) {
      if (pattern.value.test(text)) return pattern.name;
    }
    overlap = text.slice(-overlapBytes);
  }
  return null;
}

async function scan(root: string, path = root): Promise<number> {
  const entry = await lstat(path);
  if (entry.isSymbolicLink()) return 0;
  if (entry.isDirectory()) {
    let checked = 0;
    for (const child of await readdir(path)) checked += await scan(root, join(path, child));
    return checked;
  }
  if (!entry.isFile()) return 0;
  const found = await scanFile(path);
  if (found !== null) {
    console.error(
      `Credential exclusion failed: ${found} in ${relative(root, path) || basename(path)}`,
    );
    process.exitCode = 1;
  }
  return 1;
}

const roots = process.argv.slice(2);
if (roots.length === 0) {
  console.error("Usage: bun scripts/check-client-secrets.ts <built-file-or-directory> [...]");
  process.exitCode = 2;
} else {
  try {
    let checked = 0;
    for (const root of roots) {
      const count = await scan(root);
      if (count === 0) throw new Error("Requested root contains no regular files");
      checked += count;
    }
    if (process.exitCode === 1) {
      console.error(`Credential exclusion failed across ${checked} inspected files.`);
    } else {
      console.info(
        `Credential exclusion passed for ${roots.length} roots (${checked} files); matched values are never printed.`,
      );
    }
  } catch {
    console.error("Credential exclusion could not inspect every requested artifact.");
    process.exitCode = 2;
  }
}
