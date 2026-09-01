/**
 * The layout probe: the real shell, at a real width, in a real browser.
 *
 * Story 59.13 exists because Story 59.12 was "verified" with a DOM probe that
 * read stores and option values and never measured a box. jsdom performs no
 * layout, so neither the component suite nor that probe could see that an empty
 * panel strip had taken 59-64% of the window and squeezed the Tasks pane's
 * detail region — the host of the add form — down to 52px at 1024. Reading CSS
 * is what failed; this file measures instead.
 *
 * It renders `App` — the whole shell, not a hand-assembled subset — over
 * `dev/mock-shell.ts`, then drives the view with real gestures (`.click()` on
 * the real trigger, the native value setter plus a real `input` event on the
 * real field) and prints `PROBE key=value` lines into `#PROBE`. `#PROBE` is
 * `position: fixed; visibility: hidden`, so it contributes no layout of its own
 * and `--dump-dom` still carries every line: no browser automation has to be
 * installed on the measuring host.
 *
 * Query parameters:
 * - `view` — `tasks` (default), `files`, `notes`, `sessions`.
 * - `act` — `measure` (default), `add` (open the add form), `create` (open it,
 *   fill it, submit it, and look for the created task in the list).
 * - `tasks` — `fixture` (default) or `none`, which answers `sync_tasks` with an
 *   empty listing. The owner's own `sync.db` held zero tasks, which is the state
 *   in which the collapsed region is unusable rather than merely tight.
 *
 * Run it (from the repo root, on a host with a browser):
 *
 *   bun x vite --port 8133 &
 *   for w in 1024 1280 1550; do
 *     "$CHROME" --headless=new --window-size=$w,900 --virtual-time-budget=20000 \
 *       --dump-dom "http://127.0.0.1:8133/dev/probe/index.html?view=tasks&act=create&tasks=none" \
 *       | tr '<' '\n' | grep '^*\?PROBE '
 *   done
 */
import { ThemeProvider } from "next-themes";
import React from "react";
import ReactDOM from "react-dom/client";
import App from "../../src/App";
import "../../src/index.css";
import { PANEL_STRIP_LABEL } from "@/components/layout/panel-strip";
import { TASKS_DETAIL_LABEL, TASKS_PANE_TITLE } from "@/components/layout/tasks-pane";
import { TASK_FORM_ADD_SUBMIT_LABEL, TASK_FORM_ADD_TITLE } from "@/components/sync/task-form";
import type { TaskListingVm, TaskVm } from "@/lib/ipc/client";
import { type PrimaryView, primaryViewStore } from "@/lib/stores/primary-view";
import { installMockShell } from "../mock-shell";

const params = new URLSearchParams(window.location.search);
const view = (params.get("view") ?? "tasks") as PrimaryView;
const act = params.get("act") ?? "measure";
const listing = params.get("tasks") ?? "fixture";

installMockShell();

// The one fixture knob. The mock shell owns every answer; this replaces exactly
// one of them, because the reported defect is at zero tasks and a harness that
// could only measure the populated case would have missed the symptom that made
// the view unusable.
if (listing === "none") {
  const internals = (window as unknown as Record<string, { invoke: unknown }>).__TAURI_INTERNALS__;
  const inner = internals.invoke as (...args: unknown[]) => Promise<unknown>;
  // An EMPTY STORE, not a muted read. A knob that answered `sync_tasks` with
  // `[]` unconditionally would also swallow the task this probe creates, and
  // would then "prove" the owner's second symptom is still broken when it is the
  // harness that cannot see the row. So the mock's own state is the truth and
  // this filters it to what was created in this session: nothing at first, and
  // exactly the created task afterwards.
  const created = new Set<string>();
  internals.invoke = async (...args: unknown[]) => {
    if (args[0] === "sync_tasks") {
      // The generated wire type, not an inline shape: `sync_tasks` answers
      // exactly this and the mock shell is typed against the same declaration.
      const real = (await inner(...args)) as TaskListingVm;
      return { tasks: real.tasks.filter((task) => created.has(task.id)), unknown: [] };
    }
    const answer = await inner(...args);
    if (args[0] === "sync_task_save") {
      const saved = answer as TaskVm;
      created.add(saved.id);
    }
    return answer;
  };
}

const out: string[] = [];
const sink = document.createElement("pre");
sink.id = "PROBE";
// Fixed and invisible: the probe may not be part of what it measures.
sink.style.cssText = "position:fixed;left:0;top:0;visibility:hidden;margin:0";
document.body.appendChild(sink);

function emit(key: string, value: string | number): void {
  out.push(`PROBE ${key}=${value}`);
  sink.textContent = out.join("\n");
}

/**
 * Hand the collected lines to the collector, so a run does not depend on
 * `--dump-dom`.
 *
 * `--dump-dom` prints at the load event, which is long before this probe has
 * finished driving anything, and the only flag that postpones it —
 * `--virtual-time-budget` — makes headless Chrome 152 on this host print the DOM
 * and then never exit (measured: 176s wall for a 25s budget, killed by hand).
 * A beacon is fire-and-forget, `text/plain` so it is a CORS-simple request, and
 * it lets the run be driven in real time and torn down the moment the last line
 * lands. The `#PROBE` node is still filled, so `--dump-dom` keeps working for
 * anyone who prefers it.
 */
function report(): void {
  const label = params.get("label") ?? "run";
  navigator.sendBeacon(
    `http://127.0.0.1:8134/${encodeURIComponent(label)}`,
    new Blob([`${out.join("\n")}\n`], { type: "text/plain" }),
  );
}

// The executor form because this project's `lib` predates `Promise.withResolvers`
// (tsc: "change 'lib' to 'es2024' or later"), and a probe is not the place to
// move the whole build's target.
function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function waitFor<T>(what: string, probe: () => T | null | undefined): Promise<T | null> {
  for (let tries = 0; tries < 120; tries += 1) {
    const found = probe();
    if (found !== null && found !== undefined && found !== false) {
      return found;
    }
    await sleep(50);
  }
  emit(`missing.${what}`, "true");
  return null;
}

const width = (el: Element | null | undefined): number =>
  el ? Math.round(el.getBoundingClientRect().width) : -1;

function q(selector: string): Element | null {
  return document.querySelector(selector);
}

function byText(selector: string, text: string): HTMLElement | null {
  return (
    (Array.from(document.querySelectorAll(selector)) as HTMLElement[]).find(
      (el) => (el.textContent ?? "").trim() === text,
    ) ?? null
  );
}

/** Every box this story is about, at one phase. */
function measure(phase: string): void {
  const strip = q(`section[aria-label="${PANEL_STRIP_LABEL}"]`);
  emit(`${phase}.window`, window.innerWidth);
  emit(`${phase}.strip`, width(strip));
  emit(`${phase}.strip_present`, strip === null ? "no" : "yes");
  if (view === "tasks") {
    const pane = q(`section[aria-label="${TASKS_PANE_TITLE}"]`);
    const detail = q(`section[aria-label="${TASKS_DETAIL_LABEL}"]`);
    emit(`${phase}.pane`, width(pane));
    emit(`${phase}.list`, width(q("#column-tasks-list")));
    emit(`${phase}.detail`, width(detail));
    // The add form's host card and the form itself. The card carries the
    // `max-w-[720px]`; the form is what a person has to fill.
    const form = detail?.querySelector("form") ?? null;
    emit(`${phase}.form`, width(form));
    emit(`${phase}.form_card`, width(form?.closest('[data-slot="card"]') ?? null));
    // The narrowest control in the form. A form wider than the region but with a
    // field narrower than its own label is still unfillable.
    // Radix's `Switch` puts a form-bubbling `<input type=checkbox>` behind the
    // toggle at `aria-hidden` and 100% of a 32px button, so an unfiltered
    // minimum reports 32 and hides whether the nine real controls reached the
    // 224px they are designed at.
    const fields = (
      Array.from(form?.querySelectorAll("input,select,textarea") ?? []) as HTMLElement[]
    ).filter((f) => f.getAttribute("aria-hidden") !== "true");
    if (fields.length > 0) {
      emit(
        `${phase}.form_field_min`,
        Math.min(...fields.map((f) => Math.round(f.getBoundingClientRect().width))),
      );
      emit(`${phase}.form_fields`, fields.length);
    }
    // Which of the two classes the eye meets first, as a number rather than as
    // a reading of the JSX: story 58.7's projected rows sit inside the SAME
    // scroller as the task rows, so "the projection is above the tasks" is a
    // claim about screen position and has to be measured as one.
    const firstTask = q('[role="option"]');
    const paced = q("[data-paced-id]")?.closest("div.flex.flex-col.gap-2") ?? null;
    const pacedHeading = byText("h2", "Also paced by this host");
    emit(`${phase}.first_task_top`, Math.round(firstTask?.getBoundingClientRect().top ?? -1));
    emit(`${phase}.paced_heading_top`, Math.round(pacedHeading?.getBoundingClientRect().top ?? -1));
    emit(`${phase}.paced_height`, Math.round(paced?.getBoundingClientRect().height ?? -1));
    emit(`${phase}.paced_rows`, document.querySelectorAll("[data-paced-id]").length);
    // The form's widest label, so the region's floor can be argued from a
    // measured number instead of a guessed one.
    const labels = Array.from(form?.querySelectorAll("label") ?? []) as HTMLElement[];
    if (labels.length > 0) {
      emit(
        `${phase}.form_label_max`,
        Math.max(...labels.map((l) => Math.round(l.getBoundingClientRect().width))),
      );
      emit(`${phase}.form_label_scroll_max`, Math.max(...labels.map((l) => l.scrollWidth)));
    }
    // One-word-per-line detection over every text block in the pane: the
    // owner's screenshot symptom, as a number. A block is "shredded" when it
    // holds 4+ words and its box is narrower than its own longest word plus a
    // little — measured by comparing rendered line count to word count.
    emit(`${phase}.shredded`, shreddedBlocks(pane).join(",") || "none");
  } else {
    emit(`${phase}.list`, width(q("#column-files-tree") ?? q("#column-notes-list")));
    emit(`${phase}.rail`, width(q("#column-notes-rail")));
    emit(`${phase}.pane`, width(strip?.previousElementSibling ?? null));
  }
}

/**
 * Text blocks rendered at roughly one word per line — the shape the owner
 * photographed. A paragraph of N words laid out in more than N/1.6 lines is
 * being shredded rather than wrapped.
 */
function shreddedBlocks(root: Element | null): string[] {
  if (root === null) {
    return [];
  }
  const bad: string[] = [];
  for (const el of Array.from(root.querySelectorAll("p,h1,h2,dd,dt,span,code"))) {
    const text = (el.textContent ?? "").trim();
    const words = text.split(/\s+/).filter((w) => w.length > 0).length;
    if (words < 4 || el.childElementCount > 0) {
      continue;
    }
    const box = el.getBoundingClientRect();
    const lineHeight = Number.parseFloat(getComputedStyle(el).lineHeight);
    if (!Number.isFinite(lineHeight) || lineHeight <= 0 || box.height <= 0) {
      continue;
    }
    const lines = Math.round(box.height / lineHeight);
    if (lines > words / 1.6) {
      bad.push(`${lines}L/${words}W:${text.slice(0, 24).replace(/[,=\s]+/g, "_")}`);
    }
  }
  return bad;
}

/** React's own setter, so a typed value is a value React has seen. */
function type(el: HTMLInputElement | HTMLTextAreaElement, value: string): void {
  const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement : HTMLInputElement;
  const setter = Object.getOwnPropertyDescriptor(proto.prototype, "value")?.set;
  setter?.call(el, value);
  el.dispatchEvent(new Event("input", { bubbles: true }));
}

async function drive(): Promise<void> {
  await waitFor("shell", () => q("nav") ?? q("aside"));
  primaryViewStore.getState().setView(view);
  // Two frames plus the pane's own read. Everything the pane draws arrives from
  // one settled pass over two commands, so this waits for the pane rather than
  // for a fixed delay.
  if (view === "tasks") {
    await waitFor("tasks-pane", () => q(`section[aria-label="${TASKS_DETAIL_LABEL}"]`));
  }
  await sleep(600);
  measure("rest");

  // The three-column case: the pane's list, the pane's detail region, and a
  // panel strip holding a task. This is the arrangement the floors exist for,
  // and it is reached the way a person reaches it — a real double click on a
  // real row, not a store write.
  if (act === "beside" || act === "beside-add") {
    const row = q('[role="option"]') as HTMLElement | null;
    if (row === null) {
      emit("missing.row", "true");
      return;
    }
    row.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    await waitFor("strip", () => q(`section[aria-label="${PANEL_STRIP_LABEL}"]`));
    await sleep(700);
    measure("beside");
    if (act === "beside-add") {
      const trigger = byText("button", TASK_FORM_ADD_TITLE);
      trigger?.click();
      await waitFor("beside-form", () => q(`section[aria-label="${TASKS_DETAIL_LABEL}"] form`));
      await sleep(400);
      measure("beside_adding");
    }
  }

  if (act === "add" || act === "create") {
    const trigger = byText("button", TASK_FORM_ADD_TITLE);
    if (trigger === null) {
      emit("missing.add_trigger", "true");
      return;
    }
    trigger.click();
    await waitFor("add-form", () => q(`section[aria-label="${TASKS_DETAIL_LABEL}"] form`));
    await sleep(400);
    measure("adding");
  }

  if (act === "create") {
    const form = q(`section[aria-label="${TASKS_DETAIL_LABEL}"] form`) as HTMLFormElement | null;
    if (form === null) {
      emit("missing.form", "true");
      return;
    }
    // The id, typed rather than minted, so the row this probe looks for
    // afterwards is the row this probe created.
    // By its `id` and not by `type`: the project's `Input` sets no `type`, so
    // `input[type=text]` matches nothing at all here.
    const idField = form.querySelector('input[id$="-id"]') as HTMLInputElement | null;
    if (idField === null) {
      emit("missing.id_field", "true");
      return;
    }
    type(idField, "probe-created");
    emit("create.id_typed", idField.value);
    const submit = byText("button", TASK_FORM_ADD_SUBMIT_LABEL);
    if (submit === null) {
      emit("missing.submit", "true");
      return;
    }
    submit.click();
    const row = await waitFor(
      "created-row",
      () => q('[role="option"][data-task-id="probe-created"]') ?? rowByText("probe-created"),
    );
    emit("create.row_in_list", row === null ? "no" : "yes");
    emit("create.row_width", width(row));
    await sleep(400);
    measure("created");
  }

  emit("scenario", `${view}/${act}/${listing}`);
}

function rowByText(id: string): Element | null {
  return (
    Array.from(document.querySelectorAll('[role="option"]')).find((el) =>
      (el.textContent ?? "").includes(id),
    ) ?? null
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ThemeProvider attribute="class" defaultTheme="light" enableSystem={false}>
      <App />
    </ThemeProvider>
  </React.StrictMode>,
);

void drive()
  .catch((error) => {
    emit("error", String(error).slice(0, 200));
  })
  .finally(() => {
    emit("done", "true");
    report();
  });
