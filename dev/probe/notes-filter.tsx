/** Real NoteFilterBar + shipped CSS, not a diagram. See spec-76-5 for commands. */
import { createRef } from "react";
import ReactDOM from "react-dom/client";
import { NoteFilterBar } from "@/components/notes/note-filter-bar";
import { notesFiltersStore } from "@/lib/stores/notes-filters";
import { notesSearchStateStore } from "@/lib/stores/notes-search-state";
import { notesVaultsStore } from "@/lib/stores/notes-vaults";
import { installMockShell } from "../mock-shell";
import "../../src/index.css";

installMockShell();
const params = new URLSearchParams(location.search);
const width = Number(params.get("width") ?? 240);
const phone = params.get("tier") === "phone";
const phase = params.get("phase") ?? "meaning";
const fixture = params.get("chips") ?? "long";
const root = document.getElementById("root");
if (!root) throw new Error("Missing probe root");
root.style.width = `${width}px`;
if (params.get("theme") === "dark") document.documentElement.classList.add("dark");
notesVaultsStore.setState({ activeVaultId: "probe" });
notesSearchStateStore.getState().apply({
  vaultId: "probe",
  phase,
  indexed: 42,
  total: 688,
  embedded: 15,
  embeddable: 500,
  model: phase === "words" ? "" : "embed",
  sentence: phase === "refused" ? "The embedding provider is unavailable." : "",
});
const filters = notesFiltersStore.getState();
filters.setText("budget");
if (fixture !== "none") {
  filters.setTagTerm("work", "include");
  filters.setTagTerm("draft", "exclude");
}
if (fixture === "long" || fixture === "many") {
  filters.setScope({
    kind: "folder",
    vaultId: "probe",
    path: "projects/An unusually long scope that must not move any control",
  });
  filters.setTagTerm("projects/an-extremely-long-tag-that-must-keep-its-dismiss-target", "include");
}
if (fixture === "many") {
  for (let index = 0; index < 30; index += 1) filters.setTagTerm(`tag-${index}`, "include");
}
const searchRef = createRef<HTMLTextAreaElement>();
let saves = 0;
ReactDOM.createRoot(root).render(
  <NoteFilterBar
    phone={phone}
    searchRef={searchRef}
    onSaveAsSpace={() => {
      saves += 1;
    }}
  />,
);
const output = document.createElement("pre");
output.id = "PROBE";
output.style.cssText = "position:fixed;visibility:hidden;top:0;left:0;margin:0";
document.body.append(output);
const lines: string[] = [];
function emit(key: string, value: unknown): void {
  lines.push(`PROBE ${key}=${JSON.stringify(value)}`);
  output.textContent = lines.join("\n");
}
function rect(element: Element) {
  const { x, y, width, height } = element.getBoundingClientRect();
  return { x, y, width, height };
}
function measure(): void {
  const bar = root?.querySelector<HTMLElement>('[data-slot="note-filter-bar"]');
  if (!bar) {
    setTimeout(measure, 50);
    return;
  }
  emit("tier", phone ? "phone" : "desktop");
  emit("allocatedWidth", width);
  emit("bar", rect(bar));
  for (const slot of ["filter-tag-lane", "filter-tags", "filter-actions", "notes-search-status"]) {
    const element = bar.querySelector<HTMLElement>(`[data-slot="${slot}"]`);
    emit(slot, element ? rect(element) : null);
  }
  const buttons = Array.from(bar.querySelectorAll<HTMLButtonElement>("button"));
  const boundary = bar.getBoundingClientRect();
  const overflow = Array.from(bar.querySelectorAll<HTMLElement>("*"))
    .filter((element) => {
      const box = element.getBoundingClientRect();
      // display:contents and the offscreen associated description have no visual box.
      if (!box.width || element.classList.contains("sr-only")) return false;
      return box.left < boundary.left - 0.5 || box.right > boundary.right + 0.5;
    })
    .map((element) => ({
      name: element.getAttribute("aria-label") ?? element.dataset.slot ?? element.tagName,
      ...rect(element),
    }));
  for (const [index, button] of buttons.entries()) {
    emit(`control.${index}`, {
      name: button.getAttribute("aria-label"),
      ...rect(button),
      pressed: button.getAttribute("aria-pressed"),
    });
  }
  emit(
    "missingControls",
    [
      "Add a tag filter",
      "Changed by agent",
      "Pinned only",
      "Hide service files",
      "Save as space",
      "Clear search",
    ].filter((name) => !buttons.some((button) => button.getAttribute("aria-label") === name)),
  );
  emit("searchField", searchRef.current ? rect(searchRef.current) : null);
  emit("overflow", overflow);
  emit(
    "underTarget",
    buttons
      .filter((button) => {
        const box = button.getBoundingClientRect();
        return box.width < (phone ? 44 : 24) || box.height < (phone ? 44 : 24);
      })
      .map((button) => button.getAttribute("aria-label")),
  );
  emit("chipRows", Array.from(bar.querySelectorAll('[data-slot="filter-chip"]')).map(rect));
  const actions = bar.querySelector('[data-slot="filter-actions"]');
  emit(
    "actionRows",
    actions
      ? Array.from(actions.querySelectorAll("button")).map(
          (button) => button.getBoundingClientRect().top,
        )
      : [],
  );
  searchRef.current?.focus();
  emit("searchRefFocus", document.activeElement === searchRef.current);
  const save = buttons.find((button) => button.getAttribute("aria-label") === "Save as space");
  save?.click();
  emit("saveActivated", saves === 1);
  for (const name of ["Changed by agent", "Pinned only", "Hide service files"]) {
    const button = buttons.find((candidate) => candidate.getAttribute("aria-label") === name);
    button?.click();
  }
  setTimeout(() => {
    emit(
      "togglesAfterPress",
      buttons
        .filter((button) => button.hasAttribute("aria-pressed"))
        .map((button) => ({
          name: button.getAttribute("aria-label"),
          pressed: button.getAttribute("aria-pressed"),
        })),
    );
    buttons.find((button) => button.getAttribute("aria-label") === "Clear search")?.click();
    setTimeout(() => {
      emit("clearedQuery", notesFiltersStore.getState().text);
      emit("clearRetainsFocus", document.activeElement === searchRef.current);
      emit("done", true);
      navigator.sendBeacon(
        `http://127.0.0.1:8134/${encodeURIComponent(params.get("label") ?? "notes-filter")}`,
        new Blob([`${lines.join("\n")}\n`], { type: "text/plain" }),
      );
    }, 50);
  }, 50);
}
void document.fonts.ready.then(() => setTimeout(measure, 100));
