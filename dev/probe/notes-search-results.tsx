import { mockIPC } from "@tauri-apps/api/mocks";
import ReactDOM from "react-dom/client";
import { NoteRow } from "@/components/notes/note-row";
import { SearchSettingsSection } from "@/components/notes/search-settings";
import type { NoteRowVm } from "@/lib/ipc/client";
import "../../src/index.css";

let names = ["index.md", "agents.md"];
mockIPC((command, payload) => {
  const data = (payload ?? {}) as Record<string, unknown>;
  switch (command) {
    case "notes_service_file_names_get":
      return names;
    case "notes_service_file_names_set":
      names = data.names as string[];
      return null;
    case "notes_embedding_model_get":
      return { provider: "local", model: "unknown" };
    case "notes_embedding_model_set":
      return null;
    case "bots_providers_list":
      return [{ id: "local", name: "Local provider" }];
    case "bots_models_list":
      return [
        { id: "unknown", embedding: null },
        { id: "embedding", embedding: true },
      ];
    default:
      return null;
  }
});
const params = new URLSearchParams(location.search);
const root = document.getElementById("root");
if (!root) throw new Error("Missing root");
root.style.width = `${Number(params.get("width") ?? 320)}px`;
if (params.get("theme") === "dark") document.documentElement.classList.add("dark");
const row: NoteRowVm = {
  id: "note",
  path: "note.md",
  title: "Annual financial outlook",
  snippet: "Ordinary excerpt",
  tags: ["work", "long/project/tag", "another"],
  updatedMs: Date.now(),
  pinned: false,
  archived: false,
  unread: true,
  conflict: false,
  origin: "agent",
  predicates: [],
  unresolvedTarget: "",
  headRev: "r",
  order: { value: 0, source: "default" },
  hit: { snippet: "ał😀tax end", marks: [[1, 7]], why: "words", score: 1 },
};
const props = {
  selected: false,
  tabIndex: 0,
  canReveal: false,
  onSelect: () => {},
  onSelectBeside: () => {},
  onToggleTag: () => {},
  onVerb: () => {},
};
ReactDOM.createRoot(root).render(
  <>
    <NoteRow {...props} row={row} />
    <NoteRow
      {...props}
      row={{
        ...row,
        id: "meaning",
        hit: { snippet: "Financial outlook", marks: [], why: "meaning", score: 1 },
      }}
    />
    <SearchSettingsSection open />
  </>,
);
const output = document.createElement("pre");
output.id = "PROBE";
output.hidden = true;
document.body.append(output);
const lines: string[] = [];
function emit(key: string, value: unknown) {
  lines.push(`PROBE ${key}=${JSON.stringify(value)}`);
  output.textContent = lines.join("\n");
}
function measure() {
  const input = root?.querySelector<HTMLInputElement>("#notes-service-files");
  if (!input || input.disabled) {
    setTimeout(measure, 50);
    return;
  }
  emit("width", root?.getBoundingClientRect().width);
  emit(
    "markedText",
    [...document.querySelectorAll("mark")].map((mark) => mark.textContent),
  );
  const label = [...document.querySelectorAll("span")].find(
    (element) => element.textContent === "matched by meaning",
  );
  emit(
    "meaningLabel",
    label ? { width: label.getBoundingClientRect().width, scrollWidth: label.scrollWidth } : null,
  );
  emit(
    "settingsWidth",
    document.querySelector("#notes-search-settings")?.getBoundingClientRect().width,
  );
  emit("overflow", root ? root.scrollWidth - root.clientWidth : null);
  input.focus();
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  setter?.call(input, " log.md , , custom.md ");
  input.dispatchEvent(new Event("input", { bubbles: true }));
  setTimeout(() => {
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    setTimeout(() => {
      emit("savedNames", names);
      emit("roundTrip", input.value);
      emit("done", true);
      navigator.sendBeacon(
        `http://127.0.0.1:8134/${encodeURIComponent(params.get("label") ?? "results")}`,
        new Blob([`${lines.join("\n")}\n`], { type: "text/plain" }),
      );
    }, 100);
  }, 50);
}
void document.fonts.ready.then(() => setTimeout(measure, 100));
