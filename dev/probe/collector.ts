/**
 * Receives the layout probe's beacons and writes one file per label.
 *
 * It exists because `--dump-dom` prints at the load event — long before the probe
 * has finished driving anything — and the only flag that postpones it,
 * `--virtual-time-budget`, makes headless Chrome 152 on the measuring host print
 * the DOM and then never exit (measured: 176s of wall time for a 25s budget,
 * killed by hand). Driving the page in real time and having it hand its own lines
 * back is both faster and deterministic: `measure.sh` waits for `done=true` in
 * the file and tears the browser down the moment it lands.
 *
 *   bun run dev/probe/collector.ts
 *
 * `node:http` and not `Bun.serve`, so this typechecks under the repo's own
 * `tsc` without adding `@types/bun` to a production dependency list for the sake
 * of a harness.
 */
import { mkdirSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";

const OUT_DIR = process.env.PROBE_OUT ?? "/tmp/probe-out";

mkdirSync(OUT_DIR, { recursive: true });

createServer((req, res) => {
  const label = decodeURIComponent((req.url ?? "/").slice(1)) || "run";
  const chunks: Buffer[] = [];
  req.on("data", (chunk: Buffer) => chunks.push(chunk));
  req.on("end", () => {
    // The label reaches the filesystem, so it is constrained rather than escaped.
    writeFileSync(`${OUT_DIR}/${label.replace(/[^\w.-]/g, "_")}.txt`, Buffer.concat(chunks));
    // `*` because the page is served by the vite dev server on another port.
    res.writeHead(200, { "access-control-allow-origin": "*" }).end("ok");
  });
}).listen(8134, "127.0.0.1", () => {
  console.log(`collector on 127.0.0.1:8134 → ${OUT_DIR}`);
});
