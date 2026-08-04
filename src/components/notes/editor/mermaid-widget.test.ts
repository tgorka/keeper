import { describe, expect, it } from "vitest";
import { type MermaidModule, renderMermaidInto } from "./mermaid-widget";

const BROKEN = "graph TD\n  A --< B";

/** A mermaid stand-in whose `render` fails the way a parse error does. */
function failing(message: string): MermaidModule {
  return {
    initialize: () => {},
    render: () => Promise.reject(new Error(message)),
  };
}

describe("renderMermaidInto", () => {
  it("renders the fence's own source plus the reason when the diagram will not parse", async () => {
    const host = document.createElement("div");

    await renderMermaidInto(host, BROKEN, { load: async () => failing("Parse error on line 2") });

    expect(host.querySelector(".cm-mermaid-error-source")?.textContent).toBe(BROKEN);
    expect(host.querySelector('[role="alert"]')?.textContent).toBe("Parse error on line 2");
  });

  it("degrades to source when mermaid itself cannot be loaded", async () => {
    const host = document.createElement("div");

    await renderMermaidInto(host, BROKEN, {
      load: () => Promise.reject(new Error("chunk load failed")),
    });

    expect(host.querySelector(".cm-mermaid-error-source")?.textContent).toBe(BROKEN);
    expect(host.textContent).toContain("chunk load failed");
  });

  it("abandons a render that never resolves rather than leaving an empty node", async () => {
    const host = document.createElement("div");

    await renderMermaidInto(host, BROKEN, {
      timeoutMs: 5,
      load: async () => ({ initialize: () => {}, render: () => new Promise<never>(() => {}) }),
    });

    expect(host.querySelector(".cm-mermaid-error-source")?.textContent).toBe(BROKEN);
    expect(host.childElementCount).toBeGreaterThan(0);
  });

  it("adopts the rendered diagram when mermaid succeeds", async () => {
    const host = document.createElement("div");

    await renderMermaidInto(host, "graph TD\n  A --> B", {
      load: async () => ({
        initialize: () => {},
        render: async () => ({ svg: "<svg><title>ok</title></svg>" }),
      }),
    });

    expect(host.querySelector("svg title")?.textContent).toBe("ok");
    expect(host.querySelector(".cm-mermaid-error")).toBeNull();
  });
});
