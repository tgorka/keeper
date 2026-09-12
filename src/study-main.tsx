import { createRoot } from "react-dom/client";
import { Study } from "./study/study";
import "./study/study.css";

// A separate document, not an app route. Nothing from App or its data stores mounts here.
// Remove accidental query/hash values before the SDK can ever load; no return URL is accepted.
window.history.replaceState(null, "", window.location.pathname);
const root = document.getElementById("study-root");
if (!root) throw new Error("Study root is missing");
createRoot(root).render(<Study />);
