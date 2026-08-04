/**
 * The quick-capture window's entry point (AD-60, NFR-27, UX-DR35).
 *
 * Deliberately the smallest React application in the repo. It imports the IPC
 * wrappers, one hook and the stylesheet — no editor, no mermaid, no router, no
 * theme provider, no `StrictMode` double-mount. Every one of those would be
 * bytes parsed before the first keystroke can land, and the panel's entire
 * promise is that it is already there.
 *
 * What is on the panel: a textarea. What is deliberately not, and must never
 * be added: a title field, a folder picker, a template picker, a save button, a
 * close button, a character count, a discard affordance. Escape saves; nothing
 * anywhere on this window discards text.
 */
import ReactDOM from "react-dom/client";
import { useNotesCapture } from "@/hooks/use-notes-capture";
import "./index.css";

export function CapturePanel() {
  const { text, error, textareaRef, setText, commit } = useNotesCapture();
  return (
    <div className="flex h-screen flex-col bg-background text-foreground">
      <textarea
        ref={textareaRef}
        value={text}
        onChange={(event) => setText(event.target.value)}
        onKeyDown={(event) => {
          // Escape and ⌘W are the same act: save and hide (UX-DR35).
          if (event.key === "Escape" || (event.key === "w" && event.metaKey)) {
            event.preventDefault();
            commit();
          }
        }}
        aria-label="Quick capture"
        placeholder="Catch a thought"
        spellCheck={false}
        className="flex-1 resize-none bg-transparent p-4 text-sm leading-relaxed outline-none"
      />
      {error === null ? null : (
        // The panel stays open and the text stays put: the one thing capture
        // may never do is swallow words.
        <p role="alert" className="border-t px-4 py-2 text-xs text-destructive">
          {error}
        </p>
      )}
    </div>
  );
}

// Guarded so the module can be imported by a test without mounting a root.
const container = document.getElementById("root");
if (container) {
  ReactDOM.createRoot(container).render(<CapturePanel />);
}
