/**
 * Measure the phone's Files surface in a real browser (Story 66.3, AD-200):
 * does the listing fit a 430 pt column, and does a document opened from it?
 *
 * jsdom lays nothing out, so `files-phone-pane.test.tsx` can only guard the
 * structure — the rows, the order of the calls, which control is present.
 * "No horizontal overflow at 390 pt" is a number, and this is where it comes
 * from. Same rig as `measure-bots.ts`: the real app over `dev/mock-shell.ts`,
 * driven through Chrome's DevTools protocol with nothing but `fetch` and
 * `WebSocket`. Chrome is usually on the Mac and the ports are tunnelled:
 *
 *   bun x vite --port 8133 --host 127.0.0.1            # here
 *   ssh -N -R 8133:127.0.0.1:8133 -L 9223:127.0.0.1:9222 hesperia &
 *   ssh hesperia '"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
 *     --headless=new --remote-debugging-port=9222 "--remote-allow-origins=*" \
 *     --user-data-dir=/tmp/chrome-measure --no-first-run --disable-gpu \
 *     --window-size=1440,900 about:blank &'
 *   bun run dev/measure-files.ts [--shot]
 *
 * The device is an iPhone 14 Pro Max upright, 430×932, loaded with
 * `?platform=phone` so the mock answers the iPhone's `CapabilitiesVm` (`sync`
 * and `shareOut` on, every desktop flag off) and the stack renders. The run
 * opens the drawer, taps Files, and measures the listing; then taps the first
 * text row and measures the document view. For each: the pane's width against
 * the viewport, its `scrollWidth` against its `clientWidth` (the overflow
 * question), and the widest descendant — the element to look at when the
 * answer is not zero.
 *
 * Exit status is the verdict: non-zero if either state overflows.
 */

import { writeFileSync } from "node:fs";

const CDP_URL = process.env.CDP_URL ?? "http://127.0.0.1:9223";
const DEVICE = { width: 430, height: 932, mobile: true };
const APP_URL = `${process.env.APP_URL ?? "http://127.0.0.1:8133/"}?platform=phone`;
const SHOT = process.argv.includes("--shot");
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

  // The executor form, as `measure-bots.ts` spells it: the project's lib
  // target predates `Promise.withResolvers`.
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
 * What the page measures, inside the page: the Files section, the body slot
 * that is up (`listing` or `document`), each one's overflow, and the widest
 * element under the section — the one to look at when there is overflow.
 */
const MEASURE = `(() => {
  const pane = document.querySelector('section[aria-label="Files"]');
  if (!pane) return null;
  const slot = pane.querySelector('[data-slot="files-phone-document"]')
    ?? pane.querySelector('[data-slot="files-phone-listing"]')
    ?? pane.querySelector('[data-slot="files-phone-profiles"]');
  const rect = pane.getBoundingClientRect();
  let widest = { label: '', right: 0 };
  for (const el of pane.querySelectorAll('*')) {
    const r = el.getBoundingClientRect();
    if (r.width === 0) continue;
    if (r.right > widest.right) {
      widest = {
        label: (el.getAttribute('data-slot') ?? el.getAttribute('data-testid') ?? el.tagName.toLowerCase())
          + (el.className && typeof el.className === 'string' ? '.' + el.className.split(' ').slice(0, 3).join('.') : ''),
        right: Math.round(r.right),
      };
    }
  }
  return {
    viewport: { width: window.innerWidth, height: window.innerHeight },
    pane: { width: Math.round(rect.width), right: Math.round(rect.right), height: Math.round(rect.height) },
    paneOverflow: pane.scrollWidth - pane.clientWidth,
    slot: slot ? slot.getAttribute('data-slot') : null,
    slotOverflow: slot ? slot.scrollWidth - slot.clientWidth : null,
    documentOverflow: document.documentElement.scrollWidth - document.documentElement.clientWidth,
    rows: pane.querySelectorAll('[data-testid^="files-phone-row-"]').length,
    rowHeights: [...pane.querySelectorAll('[data-testid^="files-phone-row-"]')].map((r) => Math.round(r.getBoundingClientRect().height)),
    widest,
  };
})()`;

interface Measured {
  viewport: { width: number; height: number };
  pane: { width: number; right: number; height: number };
  paneOverflow: number;
  slot: string | null;
  slotOverflow: number | null;
  documentOverflow: number;
  rows: number;
  rowHeights: number[];
  widest: { label: string; right: number };
}

function report(label: string, m: Measured): boolean {
  const overflow = m.paneOverflow > 0 || (m.slotOverflow ?? 0) > 0 || m.documentOverflow > 0;
  console.log(
    `### ${label} — ${m.viewport.width}×${m.viewport.height} (mobile), ${m.slot ?? "no body"}`,
  );
  console.log(
    `pane ${m.pane.width}px wide, right edge at ${m.pane.right}px of ${m.viewport.width}px, ${m.pane.height}px tall`,
  );
  console.log(
    `overflow: pane ${m.paneOverflow}px, body ${m.slotOverflow ?? "—"}px, document ${m.documentOverflow}px → ${overflow ? "OVERFLOWS" : "none"}`,
  );
  if (m.rows > 0) {
    const min = Math.min(...m.rowHeights);
    console.log(
      `${m.rows} rows, heights ${min}–${Math.max(...m.rowHeights)}px (44 pt floor ${min >= 44 ? "held" : "BROKEN"})`,
    );
  }
  console.log(`widest element: ${m.widest.label}, right edge at ${m.widest.right}px`);
  return overflow;
}

async function shoot(session: Session, label: string): Promise<void> {
  if (!SHOT) {
    return;
  }
  const { data } = (await session.send("Page.captureScreenshot", { format: "png" })) as {
    data: string;
  };
  const file = `/tmp/measure-files-${label}.png`;
  writeFileSync(file, Buffer.from(data, "base64"));
  console.log(`screenshot ${file}`);
}

const TIER = `(document.querySelector('[data-level="0"]') ? 'phone' : document.querySelector('nav[aria-label="Views"]') ? 'desktop' : null)`;

async function main(): Promise<void> {
  const created = await fetch(`${CDP_URL}/json/new?about:blank`, { method: "PUT" });
  const target = (await created.json()) as Target;
  const session = await Session.open(target.webSocketDebuggerUrl);
  let overflowed = false;
  try {
    await session.send("Page.enable");
    await session.send("Runtime.enable");
    await session.send("Network.clearBrowserCookies");
    await session.send("Emulation.setDeviceMetricsOverride", {
      width: DEVICE.width,
      height: DEVICE.height,
      deviceScaleFactor: 1,
      mobile: true,
    });
    await session.send("Emulation.setTouchEmulationEnabled", { enabled: true });
    await session.send("Page.navigate", { url: APP_URL });
    await waitFor(session, "the shell", TIER);
    const tier = await session.evaluate<string>(TIER);
    console.log(`shell mounted as the ${tier} tier`);
    if (tier !== "phone") {
      throw new Error("the phone tier did not render; is the mock answering ?platform=phone?");
    }

    // The drawer, then its Files row.
    await session.evaluate(
      `document.querySelector('button[aria-label="Open navigation"]').click()`,
    );
    await waitFor(
      session,
      "the Files row",
      `[...document.querySelectorAll('nav[aria-label="Views"] button')].some((b) => b.textContent.startsWith('Files'))`,
    );
    await session.evaluate(
      `[...document.querySelectorAll('nav[aria-label="Views"] button')].find((b) => b.textContent.startsWith('Files')).click()`,
    );
    await waitFor(
      session,
      "the Files section",
      `document.querySelector('section[aria-label="Files"]')`,
    );
    // The mock has three profiles: tap the first so the listing is up.
    await waitFor(
      session,
      "a profile or a listing",
      `document.querySelector('[data-slot="files-phone-listing"]') || document.querySelector('[data-testid^="files-phone-row-"]')`,
    );
    if (
      !(await session.evaluate<boolean>(
        `Boolean(document.querySelector('[data-slot="files-phone-listing"]'))`,
      ))
    ) {
      await session.evaluate(`document.querySelector('[data-testid^="files-phone-row-"]').click()`);
    }
    await waitFor(
      session,
      "the listing's rows",
      `document.querySelector('[data-slot="files-phone-listing"] [data-testid^="files-phone-row-"]')`,
    );
    await sleep(800);
    const listing = await session.evaluate<Measured | null>(MEASURE);
    if (listing === null) {
      throw new Error("the Files pane is not on screen");
    }
    overflowed = report(`${LABEL} listing`, listing) || overflowed;
    await shoot(session, `${LABEL}-listing`);

    // A markdown row and then a PDF row, each opened full-screen: the text
    // viewer and the document viewer are the two frames the surface reuses,
    // and each has its own idea of a header row.
    for (const [kind, pattern] of [
      ["markdown", "\\.md$"],
      ["pdf", "\\.pdf$"],
    ] as const) {
      const opened = await session.evaluate<boolean>(
        `(() => { const row = [...document.querySelectorAll('[data-slot="files-phone-listing"] [data-testid^="files-phone-row-"]')].find((r) => /${pattern}/.test(r.getAttribute('data-testid'))); if (!row) return false; row.click(); return true; })()`,
      );
      if (!opened) {
        throw new Error(`the listing has no ${kind} row to open`);
      }
      await waitFor(
        session,
        `the ${kind} view`,
        `document.querySelector('[data-slot="files-phone-document"]')`,
      );
      await sleep(1500);
      const doc = await session.evaluate<Measured | null>(MEASURE);
      if (doc === null) {
        throw new Error(`the ${kind} view is not on screen`);
      }
      overflowed = report(`${LABEL} ${kind}`, doc) || overflowed;
      await shoot(session, `${LABEL}-${kind}`);
      // Back to the listing for the next row.
      await session.evaluate(
        `[...document.querySelectorAll('section[aria-label="Files"] header button')].find((b) => /^Back to/.test(b.getAttribute('aria-label') ?? '')).click()`,
      );
      await waitFor(
        session,
        "the listing again",
        `document.querySelector('[data-slot="files-phone-listing"] [data-testid^="files-phone-row-"]')`,
      );
    }
  } finally {
    session.close();
    await fetch(`${CDP_URL}/json/close/${target.id}`);
  }
  if (overflowed) {
    process.exit(2);
  }
}

main().catch((error: unknown) => {
  console.error(String(error));
  process.exit(1);
});
