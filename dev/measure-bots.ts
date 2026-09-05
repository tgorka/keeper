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
 *   bun run dev/measure-bots.ts [--unfold] [--phone | --phone-landscape]
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
 *
 * # The phone modes (Epic 65, AD-195)
 *
 * `--phone` is an iPhone 14 Pro Max upright, 430×932; `--phone-landscape` is
 * the same phone rotated, 932×430. Both emulate a mobile device and load the
 * app with `?platform=phone`, which makes `dev/mock-shell.ts` answer the
 * iPhone's `CapabilitiesVm` — every desktop flag off, `bots` on — because
 * since AD-189 that answer, not the width, is what chooses the tier. The run
 * reports the tier that rendered (the stack's `data-level="0"` present, or
 * the desktop's `Views` navigation), the transcript's share of the Bots
 * conversation level, and whether that level's bottom edge is inside the
 * viewport — the owner's report was a rotated phone whose pane was clipped.
 * The stack's Bots conversation is two taps in: the header's Bots button,
 * then the first session.
 *
 * After the first measurement a phone run rotates the emulated device and
 * measures again: AD-189 says a rotation is a resize the stack handles, so the
 * level that was open must still be open, in the phone tier.
 *
 * Measured on hesperia's Chrome, 2026-09-05, the Bots conversation open:
 *
 *   430×932 before  phone tier    transcript 742 of 932px = 79.6%
 *   932×430 before  desktop tier  transcript  48 of 430px = 11.2%, Talk/Send clipped
 *   430×932 → 932×430 before      desktop tier, the conversation level gone, 5.6%
 *   430×932 after   phone tier    transcript 742 of 932px = 79.6%
 *   932×430 after   phone tier    transcript 240 of 430px = 55.8%, bottom at 430
 *   rotated after, either way     phone tier, the same level open, 79.6% ⇄ 55.8%
 */

import { writeFileSync } from "node:fs";

const CDP_URL = process.env.CDP_URL ?? "http://127.0.0.1:9223";
const PHONE = process.argv.includes("--phone");
const PHONE_LANDSCAPE = process.argv.includes("--phone-landscape");
/** The iPhone 14 Pro Max in points, both ways up; the desktop rig otherwise. */
const DEVICE = PHONE_LANDSCAPE
  ? { width: 932, height: 430, mobile: true }
  : PHONE
    ? { width: 430, height: 932, mobile: true }
    : { width: 1440, height: 900, mobile: false };
const APP_URL =
  (process.env.APP_URL ?? "http://127.0.0.1:8133/") + (DEVICE.mobile ? "?platform=phone" : "");
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
 *
 * On the phone tier the level is the stack's Bots conversation
 * (`data-slot="bots-phone-conversation"`) and the pane is that same column,
 * because the stack shows one level at a time; on the desktop they are the
 * `Bots` section and its transcript level. `tier` says which one rendered,
 * and `bottomInside` whether the pane ends inside the viewport at all.
 */
const MEASURE = `(() => {
  const round = (el) => Math.round(el.getBoundingClientRect().height);
  const phone = document.querySelector('[data-level="0"]') !== null;
  const level = phone
    ? document.querySelector('[data-slot="bots-phone-conversation"]')
    : document.querySelector('[data-slot="bots-transcript-level"]');
  const pane = phone ? level : document.querySelector('section[aria-label="Bots"]');
  if (!pane || !level) return null;
  const bands = [...level.children].map((child) => ({
    label: child.getAttribute('aria-label') ?? child.getAttribute('data-slot') ?? child.tagName.toLowerCase(),
    flexible: child.classList.contains('flex-1'),
    height: round(child),
  }));
  const voice = level.querySelector('#bots-voice-wake')?.closest('section')
    ?? document.querySelector('section[aria-label="Wake phrase"]');
  const disclosure = voice?.querySelector('[data-slot="sidebar-group-fold"]') ?? null;
  const rect = pane.getBoundingClientRect();
  return {
    tier: phone ? 'phone' : 'desktop',
    viewport: { width: window.innerWidth, height: window.innerHeight },
    pane: round(pane),
    paneBottom: Math.round(rect.bottom),
    bottomInside: rect.bottom <= window.innerHeight + 0.5,
    level: round(level),
    bands,
    transcript: bands.find((band) => band.flexible)?.height ?? 0,
    voice: voice ? round(voice) : 0,
    voiceFolded: disclosure ? disclosure.getAttribute('aria-expanded') === 'false' : null,
  };
})()`;

interface Measured {
  tier: "phone" | "desktop";
  viewport: { width: number; height: number };
  pane: number;
  paneBottom: number;
  bottomInside: boolean;
  level: number;
  bands: { label: string; flexible: boolean; height: number }[];
  transcript: number;
  voice: number;
  voiceFolded: boolean | null;
}

function report(label: string, measured: Measured): void {
  const share = ((100 * measured.transcript) / measured.pane).toFixed(1);
  console.log(
    `### ${label} — ${measured.viewport.width}×${measured.viewport.height}${DEVICE.mobile ? " (mobile)" : ""}, ${measured.tier} tier`,
  );
  console.log(
    `pane ${measured.pane}px, bottom at ${measured.paneBottom}px (${measured.bottomInside ? "inside" : "OUTSIDE"} the viewport), transcript level ${measured.level}px`,
  );
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

/**
 * What the tier that rendered has to say about the run: on the desktop rig a
 * phone tier is a defect of the width rule, and on the phone rig a desktop
 * tier is the owner's report. The page tells us which is up as soon as the
 * shell has mounted either its stack or its drawer.
 */
const TIER = `(document.querySelector('[data-level="0"]') ? 'phone' : document.querySelector('nav[aria-label="Views"]') ? 'desktop' : null)`;

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
      width: DEVICE.width,
      height: DEVICE.height,
      deviceScaleFactor: 1,
      mobile: DEVICE.mobile,
    });
    if (DEVICE.mobile) {
      await session.send("Emulation.setTouchEmulationEnabled", { enabled: true });
    }
    await session.send("Page.navigate", { url: APP_URL });
    await waitFor(session, "the shell", TIER);
    const tier = await session.evaluate<"phone" | "desktop">(TIER);
    console.log(`shell mounted as the ${tier} tier`);
    // The header's Bots button on the stack, the sidebar's on the desktop:
    // the same name, the same view.
    await waitFor(
      session,
      "the Bots control",
      `document.querySelector('button[aria-label="Bots"]')`,
    );
    await session.evaluate(`document.querySelector('button[aria-label="Bots"]').click()`);
    if (tier === "phone") {
      // Level 1 is the list; the conversation is one more tap, on a session.
      await waitFor(
        session,
        "a session row",
        `document.querySelector('section[aria-label="Bots"] li button')`,
      );
      await session.evaluate(
        `document.querySelector('section[aria-label="Bots"] li button').click()`,
      );
      await waitFor(
        session,
        "the conversation level",
        `document.querySelector('[data-slot="bots-phone-conversation"]')`,
      );
    } else {
      await waitFor(
        session,
        "the transcript level",
        `document.querySelector('[data-slot="bots-transcript-level"]')`,
      );
    }
    // The level can still be sliding in (a 250ms push on the stack), and the
    // voice block waits on two reads of its own; wait until the pane measures
    // at all, then give the mock a beat.
    await waitFor(session, "a measurable pane", MEASURE);
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

    if (DEVICE.mobile) {
      // Rotate: the same page, the other way up. AD-189 says this is a resize
      // the stack handles and never a tier change, so the conversation level
      // that was open must still be open, in the phone tier, with its bottom
      // inside the new viewport.
      await session.send("Emulation.setDeviceMetricsOverride", {
        width: DEVICE.height,
        height: DEVICE.width,
        deviceScaleFactor: 1,
        mobile: true,
      });
      await sleep(500);
      const rotated = await session.evaluate<Measured | null>(MEASURE);
      if (rotated === null) {
        const tierNow = await session.evaluate<string | null>(TIER);
        console.log(`### rotated — the conversation level is GONE; tier now ${tierNow}`);
      } else {
        report(`${LABEL} rotated`, rotated);
        await shoot(session, `${LABEL}-rotated`);
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
