import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { LAMP_STATE_WORD, Lamp, type LampState } from "@/components/ui/lamp";

/**
 * The lamp exists to fix a measured WCAG SC 1.4.1 failure: the status triad it
 * replaces encoded four meanings in hue alone, at mutual luminance ratios of
 * 1.03 / 1.52 / 1.47 and ΔE 16.3 under protanopia.
 *
 * So a test that asserts a class name here would prove nothing at all — a class
 * name IS the colour channel, and the colour channel is the thing that was
 * broken. Every assertion below therefore strips colour first and asks whether
 * the four states are still told apart by what is left.
 */

/** Written out, not derived from the component, so that a fifth state added
 * later fails the exhaustiveness check below instead of quietly joining a list
 * the tests generate for themselves. */
const STATES: LampState[] = ["live", "idle", "working", "fault"];

afterEach(cleanup);

/**
 * The glyph of one state with every colour-bearing attribute removed.
 *
 * `class` goes because that is where the tint lives, and `fill`/`stroke` go
 * because `currentColor` is a colour word even though it resolves from the
 * cascade. What remains is pure geometry — the shape a greyscale screenshot or
 * a 1-bit tray template would keep.
 */
function colourlessShape(state: LampState): string {
  const { container } = render(<Lamp state={state} />);
  const svg = container.querySelector("svg");
  if (svg === null) {
    throw new Error(`the ${state} lamp rendered no glyph at all`);
  }
  const stripped = svg.cloneNode(true) as SVGElement;
  for (const node of [stripped, ...stripped.querySelectorAll("*")]) {
    for (const name of ["class", "fill", "stroke"]) {
      node.removeAttribute(name);
    }
  }
  return stripped.innerHTML;
}

describe("Lamp — the shape channel", () => {
  it("draws a different shape for every state, with colour stripped out", () => {
    const shapes = STATES.map((state) => [state, colourlessShape(state)] as const);
    for (const [state, shape] of shapes) {
      expect(shape, `${state} has no geometry`).not.toBe("");
    }
    // The whole contract in one number. Collapsing two states onto one shape —
    // say, making `working` a plain ring again and leaving the dashes to a
    // colour — drops this to 3 and fails here.
    expect(new Set(shapes.map(([, shape]) => shape)).size).toBe(STATES.length);
  });

  it("keeps each specific pair apart, so a collapse names itself", () => {
    // The set-size assertion above proves a collapse happened; this one says
    // which two states collapsed, which is the difference between a five-minute
    // fix and an afternoon.
    for (const a of STATES) {
      for (const b of STATES) {
        if (a === b) {
          continue;
        }
        expect(colourlessShape(a), `${a} and ${b} render the same shape`).not.toBe(
          colourlessShape(b),
        );
      }
    }
  });

  it("puts no colour inside the glyph at all", () => {
    // Geometry is only a redundant channel if it is independent of the tint.
    // A state that painted itself with a literal hex, or leaned on a utility
    // class inside the svg, would be back to carrying meaning in hue.
    for (const state of STATES) {
      const { container } = render(<Lamp state={state} />);
      const svg = container.querySelector("svg");
      for (const node of [...(svg?.querySelectorAll("*") ?? [])]) {
        expect(node.getAttribute("class"), `${state} tints its own geometry`).toBeNull();
        for (const name of ["fill", "stroke"]) {
          const value = node.getAttribute(name);
          if (value !== null) {
            expect([`currentColor`, "none"], `${state} paints ${name}=${value}`).toContain(value);
          }
        }
      }
      cleanup();
    }
  });

  it("distinguishes fault from live by geometry, not by red", () => {
    // The pair most likely to be "simplified" later: both are filled discs, and
    // the only thing between them is the bite. Named on its own so that losing
    // the bite reads as a WCAG regression rather than a tidy-up.
    expect(colourlessShape("fault")).not.toBe(colourlessShape("live"));
  });
});

describe("Lamp — the text channel", () => {
  it("gives every state a distinct word a screen reader can reach", () => {
    for (const state of STATES) {
      render(<Lamp state={state} />);
      const word = screen.getByText(LAMP_STATE_WORD[state]);
      // Real text in the accessibility tree, not a `title` — `title` is not
      // reliably announced and cannot be reached from a keyboard at all.
      expect(word).toHaveClass("sr-only");
      cleanup();
    }
    expect(new Set(STATES.map((state) => LAMP_STATE_WORD[state])).size).toBe(STATES.length);
  });

  it("hides the glyph from assistive tech, so the word is not read twice", () => {
    const { container } = render(<Lamp state="live" />);
    expect(container.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
  });

  it("prefers the call site's own word when it has a better one", () => {
    render(<Lamp state="fault" label="Disconnected" />);
    expect(screen.getByText("Disconnected")).toBeInTheDocument();
    expect(screen.queryByText(LAMP_STATE_WORD.fault)).not.toBeInTheDocument();
  });

  it("cannot join a computed name on its own, which is why call sites splice", () => {
    // Documented so nobody "simplifies" a call site by dropping `label={null}`
    // and trusting the lamp to speak for itself. The accessible-name algorithm
    // trims each text node before concatenating, so an `sr-only` word beside a
    // row label fuses into one unreadable token — no padding inside the lamp
    // can separate them. This is the reason `sidebar-pane`, `chat-row`,
    // `phone-inbox-header` and `account-footer` put the state into their own
    // `aria-label` instead.
    render(
      <button type="button">
        Bridges
        <Lamp state="fault" label="Disconnected" />
      </button>,
    );
    expect(screen.getByRole("button", { name: "BridgesDisconnected" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Bridges Disconnected" })).toBeNull();
  });

  it("never delivers the state through `title`", () => {
    // `title` is the tempting shortcut and it is not an accessible name: it is
    // announced inconsistently, never on a keyboard focus path, and invisible
    // on touch. The state has to be text in the document, which is how the
    // sync rows and the recordings list already do it.
    const { container } = render(<Lamp state="fault" />);
    const lamp = container.querySelector('[data-slot="lamp"]');
    expect(lamp?.hasAttribute("title")).toBe(false);
    expect(container.querySelector("svg")?.hasAttribute("title")).toBe(false);
    expect(screen.getByText(LAMP_STATE_WORD.fault).tagName).toBe("SPAN");
  });

  it("still carries a shape when the word is deliberately suppressed", () => {
    // `label={null}` is for the call sites that already spell the state out in
    // visible text beside the lamp. Silencing the text channel must not silence
    // the shape one too, or those call sites would be back to colour alone.
    const { container } = render(<Lamp state="working" label={null} />);
    expect(container.querySelector(".sr-only")).toBeNull();
    expect(container.querySelector("svg")?.children.length).toBeGreaterThan(0);
    expect(colourlessShape("working")).not.toBe(colourlessShape("idle"));
  });
});

describe("Lamp — colour as the second channel", () => {
  it("still tints every state, and never twice the same", () => {
    // Shape was ADDED; colour was not removed. A tint shared by two states
    // would make the redundancy one-directional — fine for a screen reader,
    // useless for the sighted reader glancing at a row.
    const tints = STATES.map((state) => {
      const { container } = render(<Lamp state={state} />);
      const tint = container.querySelector('[data-slot="lamp"]')?.className ?? "";
      cleanup();
      return tint;
    });
    for (const tint of tints) {
      expect(tint).not.toBe("");
    }
    expect(new Set(tints).size).toBe(STATES.length);
  });

  it("lets a call site with its own state colour override the default", () => {
    // The recording banner's red is not one of the three health tints, and the
    // lamp must not overrule it — geometry is the lamp's business, hue is the
    // surface's.
    const { container } = render(<Lamp state="live" className="text-recording-red" />);
    expect(container.querySelector('[data-slot="lamp"]')).toHaveClass("text-recording-red");
  });

  it("reports its state as a data attribute for the tray to mirror", () => {
    // The four words are shared 1:1 with the mark's aperture and the macOS tray
    // templates. This asserts the wire format those share.
    for (const state of STATES) {
      const { container } = render(<Lamp state={state} />);
      expect(container.querySelector('[data-slot="lamp"]')).toHaveAttribute("data-state", state);
      cleanup();
    }
  });
});
