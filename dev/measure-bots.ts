/**
 * Measure the Bots pane's height budget in a real browser (Story 61.14's
 * question, asked again by Story 64.1, AD-184): of the pane's height, how much
 * is the transcript, and how much is chrome above and below it.
 *
 * jsdom lays nothing out, so `bots-pane.test.tsx` can only guard the STRUCTURE
 * the numbers were measured over. This is where the numbers come from. It
 * drives the real app — `bun run dev`, which serves the real components over
 * `dev/mock-shell.ts` — through Chrome's DevTools protocol, with nothing but
 * `fetch` and `WebSocket`, so it needs no dependency this repo does not have.
 *
 * The Linux dev container has no Chrome that will start (`libnspr4.so` is
 * missing and nobody has root), so the browser is usually on the Mac and the
 * ports are tunnelled:
 *
 *   bun x vite --port 8133 --host 127.0.0.1            # here
 *   ssh -N -R 8133:127.0.0.1:8133 -L 9223:127.0.0.1:9222 hesperia &
 *   ssh hesperia '"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
 *     --headless=new --remote-debugging-port=9222 "--remote-allow-origins=*" \
 *     --user-data-dir=/tmp/chrome-measure --no-first-run --disable-gpu \
 *     --window-size=1440,900 about:blank &'
 *   bun run dev/measure-bots.ts [--unfold]
 *
 * `CDP_URL` (default `http://127.0.0.1:9223`) is where Chrome's debugging port
 * is reachable from HERE; `APP_URL` (default `http://127.0.0.1:8133/`) is where
 * the dev server is reachable from CHROME — with the `-R` tunnel above, that is
 * the same number on the other machine. `--unfold` clicks the voice block's
 * disclosure after the first measurement and measures again, so one run
 * reports both states of Story 64.1's fold.
 *
 * Every number is a `getBoundingClientRect` height at a 1440×900 viewport,
 * rounded to a pixel. "Share" is the transcript's height over the pane's.
 */

import { writeFileSync } from "node:fs";

const CDP_URL = process.env.CDP_URL ?? "http://127.0.0.1:9223";
const APP_URL = process.env.APP_URL ?? "http://127.0.0.1:8133/";
const WIDTH = 1440;
const HEIGHT = 900;
const UNFOLD = process.argv.includes("--unfold");
/** `--shot`: also write `/tmp/measure-bots-<label>.png` for each state —
 *  the epic's proof is a picture, and the numbers alone cannot show a line
 *  that reads wrong. Written HERE, from the bytes Chrome hands back. */
const SHOT = process.argv.includes("--shot");
/** The first non-flag argument names the run in the output: `before`, `after`. */
const LABEL = process.argv.slice(2).find((arg) => !arg.startsWith("--")) ?? "measured";

interface Target {
  id: string;
  webSocketDebuggerUrl: string;
}

/** One CDP session over one WebSocket: send a command, await its reply. */
class Session {
  private seq = 0;
  private readonly pending = new Map<
    number,
    { resolve: (value: unknown) => void; reject: (reason: Error) => void }
  >();

  private constructor(private readonly socket: WebSocket) {
    socket.onmessage = (event) => {
      const message = JSON.parse(String(event.data)) as {
        id?: number;
        result?: unknown;
        error?: { message: string };
      };
      if (message.id === undefined) {
        return;
      }
      const waiter = this.pending.get(message.id);
      if (waiter === undefined) {
        return;
      }
      this.pending.delete(message.id);
      if (message.error !== undefined) {
        waiter.reject(new Error(message.error.message));
      } else {
        waiter.resolve(message.result);
      }
    };
  }

  static open(url: string): Promise<Session> {
    return new Promise((resolve, reject) => {
      const socket = new WebSocket(url);
      socket.onopen = () => resolve(new Session(socket));
      socket.onerror = () => reject(new Error(`could not open ${url}`));
    });
  }

  send(method: string, params: Record<string, unknown> = {}): Promise<unknown> {
    this.seq += 1;
    const id = this.seq;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.socket.send(JSON.stringify({ id, method, params }));
    });
  }

  /** Evaluate `expression` in the page and hand back its JSON value. */
  async evaluate<T>(expression: string): Promise<T> {
    const result = (await this.send("Runtime.evaluate", {
      expression,
      returnByValue: true,
      awaitPromise: true,
    })) as { result: { value: T }; exceptionDetails?: { text: string } };
    if (result.exceptionDetails !== undefined) {
      throw new Error(`page threw: ${result.exceptionDetails.text}`);
    }
    return result.result.value;
  }

  close(): void {
    this.socket.close();
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** Poll `expression` in the page until it is truthy, or give up. */
async function waitFor(session: Session, what: string, expression: string): Promise<void> {
  for (let tries = 0; tries < 60; tries += 1) {
    if (await session.evaluate<boolean>(`Boolean(${expression})`)) {
      return;
    }
    await sleep(500);
  }
  throw new Error(`gave up waiting for ${what}`);
}

/**
 * What the page measures. Runs INSIDE the page, so it is a string: the pane,
 * the level that holds the transcript, every band in that level with its
 * height, and the one `flex-1` child — the transcript, or the empty state
 * standing where it would be.
 */
const MEASURE = `(() => {
  const round = (el) => Math.round(el.getBoundingClientRect().height);
  const pane = document.querySelector('section[aria-label="Bots"]');
  const level = document.querySelector('[data-slot="bots-transcript-level"]');
  if (!pane || !level) return null;
  const bands = [...level.children].map((child) => ({
    label: child.getAttribute('aria-label') ?? child.getAttribute('data-slot') ?? child.tagName.toLowerCase(),
    flexible: child.classList.contains('flex-1'),
    height: round(child),
  }));
  const voice = level.querySelector('#bots-voice-wake')?.closest('section')
    ?? document.querySelector('section[aria-label="Wake phrase"]');
  const disclosure = voice?.querySelector('[data-slot="sidebar-group-fold"]') ?? null;
  return {
    pane: round(pane),
    level: round(level),
    bands,
    transcript: bands.find((band) => band.flexible)?.height ?? 0,
    voice: voice ? round(voice) : 0,
    voiceFolded: disclosure ? disclosure.getAttribute('aria-expanded') === 'false' : null,
  };
})()`;

interface Measured {
  pane: number;
  level: number;
  bands: { label: string; flexible: boolean; height: number }[];
  transcript: number;
  voice: number;
  voiceFolded: boolean | null;
}

function report(label: string, measured: Measured): void {
  const share = ((100 * measured.transcript) / measured.pane).toFixed(1);
  console.log(`### ${label} — ${WIDTH}×${HEIGHT}`);
  console.log(`pane ${measured.pane}px, transcript level ${measured.level}px`);
  for (const band of measured.bands) {
    console.log(`  ${band.flexible ? "flex-1" : "shrink-0"} ${band.height}px ${band.label}`);
  }
  console.log(
    `voice block ${measured.voice}px (${measured.voiceFolded === null ? "no disclosure" : measured.voiceFolded ? "folded" : "unfolded"})`,
  );
  console.log(`transcript ${measured.transcript}px of ${measured.pane}px = ${share}%`);
}

async function shoot(session: Session, label: string): Promise<void> {
  if (!SHOT) {
    return;
  }
  const { data } = (await session.send("Page.captureScreenshot", { format: "png" })) as {
    data: string;
  };
  const file = `/tmp/measure-bots-${label}.png`;
  writeFileSync(file, Buffer.from(data, "base64"));
  console.log(`screenshot ${file}`);
}

async function main(): Promise<void> {
  // `PUT`: Chrome refuses `GET /json/new` since 111.
  const created = await fetch(`${CDP_URL}/json/new?about:blank`, { method: "PUT" });
  const target = (await created.json()) as Target;
  const session = await Session.open(target.webSocketDebuggerUrl);
  try {
    await session.send("Page.enable");
    await session.send("Runtime.enable");
    // A fresh keeper: the fold is a cookie, and a run must measure the
    // default rather than whatever the last run left in the profile.
    await session.send("Network.clearBrowserCookies");
    await session.send("Emulation.setDeviceMetricsOverride", {
      width: WIDTH,
      height: HEIGHT,
      deviceScaleFactor: 1,
      mobile: false,
    });
    await session.send("Page.navigate", { url: APP_URL });
    await waitFor(session, "the sidebar", `document.querySelector('button[aria-label="Bots"]')`);
    await session.evaluate(`document.querySelector('button[aria-label="Bots"]').click()`);
    await waitFor(
      session,
      "the transcript level",
      `document.querySelector('[data-slot="bots-transcript-level"]')`,
    );
    // The voice block waits on two reads of its own; give the mock a beat.
    await sleep(1500);

    const before = await session.evaluate<Measured | null>(MEASURE);
    if (before === null) {
      throw new Error("the Bots pane is not on screen");
    }
    report(LABEL, before);
    await shoot(session, LABEL);

    if (UNFOLD && before.voiceFolded === true) {
      await session.evaluate(
        `document.querySelector('#bots-voice-wake')?.closest('section')?.querySelector('[data-slot="sidebar-group-fold"]')?.click()`,
      );
      await sleep(300);
      const after = await session.evaluate<Measured | null>(MEASURE);
      if (after !== null) {
        report("unfolded", after);
        await shoot(session, "unfolded");
      }
    }
  } finally {
    session.close();
    await fetch(`${CDP_URL}/json/close/${target.id}`);
  }
}

main().catch((error: unknown) => {
  console.error(String(error));
  process.exit(1);
});
