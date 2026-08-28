import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { Button, buttonVariants } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Popover, PopoverTrigger } from "@/components/ui/popover";
import {
  colourUtilities,
  contrast,
  edgeIn,
  labelIn,
  resolveColour,
  SURFACES,
  surfaceIn,
  TEXT_FLOOR,
  THEMES,
} from "@/test/colour";

/**
 * The Button's variants are the app's largest single colour decision — 70 call
 * sites across 34 files reach for `outline` alone — and the two ways they go
 * wrong are both invisible to `bun run check:design`.
 *
 * The gate recomputes the contrast of every TOKEN. It cannot see a variant that
 * spends a colour which is not a token, and it cannot see a hover that lands on
 * the same colour as the thing underneath it — which is what `hover:bg-muted`
 * did on a pane header, because `--muted` IS `--card` in both themes, so the
 * pointer went over the control and nothing happened.
 *
 * So these tests do what the gate does, and ask it of the VARIANTS. A class name
 * is not evidence here; the resolved hex is.
 */

const VARIANTS = ["default", "outline", "secondary", "ghost", "destructive", "link"] as const;
type Variant = (typeof VARIANTS)[number];

const utilities = (variant: Variant) => colourUtilities(buttonVariants({ variant }));

/**
 * Written out rather than derived, so that a variant which quietly loses its
 * hover — a change that looks like nothing at all in a diff — fails here instead
 * of being excused by a list that regenerates itself. `link` is the deliberate
 * exception: it is text, and it answers with an underline.
 */
const MUST_REACT_TO_HOVER = ["default", "outline", "secondary", "ghost", "destructive"] as const;

afterEach(cleanup);

describe("what a button variant is allowed to spend", () => {
  it.each(VARIANTS)("%s spends only named tokens, in both themes", (variant) => {
    for (const theme of Object.values(THEMES)) {
      for (const u of utilities(variant)) {
        expect(
          () => resolveColour(u.value, theme),
          `${variant}: ${u.state}:${u.kind}-${u.value}`,
        ).not.toThrow();
      }
    }
  });

  it.each(VARIANTS)("%s carries no shadow — DESIGN.md keeps one raking light", (variant) => {
    const shadows = buttonVariants({ variant })
      .split(" ")
      .filter((cls) => /(^|:)shadow-/.test(cls));
    expect(shadows).toEqual([]);
  });

  it.each(VARIANTS)("%s draws its edge with --border, never --input", (variant) => {
    // `--input` is #303a34 against `--line` #28312c — 38% more luminous, so an
    // edge drawn with it is brighter than every hairline in the app.
    expect(buttonVariants({ variant })).not.toMatch(/\binput\b/);
  });
});

describe("contrast, recomputed against both themes", () => {
  it.each(VARIANTS)("%s keeps its label at 4.5:1 at rest", (variant) => {
    for (const [themeName, theme] of Object.entries(THEMES)) {
      const u = utilities(variant);
      const label = labelIn(u, "", theme) ?? theme.foreground;
      const fill = surfaceIn(u, "", theme);
      // A variant with no fill of its own is read on whatever surface it lands
      // on, so it owes the floor on all three rather than on a favourite one.
      const beneath = fill === null ? SURFACES.map((s) => theme[s]) : [fill];
      for (const surface of beneath) {
        expect(
          contrast(label, surface),
          `${variant} label ${label} on ${surface} (${themeName})`,
        ).toBeGreaterThanOrEqual(TEXT_FLOOR);
      }
    }
  });

  it.each(MUST_REACT_TO_HOVER)("%s keeps its label at 4.5:1 while hovered", (variant) => {
    for (const [themeName, theme] of Object.entries(THEMES)) {
      const u = utilities(variant);
      const hover = surfaceIn(u, "hover", theme);
      expect(hover, `${variant} has no hover surface at all`).not.toBeNull();
      const label = labelIn(u, "hover", theme) ?? labelIn(u, "", theme) ?? theme.foreground;
      expect(
        contrast(label, hover as string),
        `${variant} label ${label} on hover ${hover} (${themeName})`,
      ).toBeGreaterThanOrEqual(TEXT_FLOOR);
    }
  });

  /**
   * The regression with no visual tell in a diff. `hover:bg-muted` measured
   * 1.000:1 against a pane header, because `--muted` and `--card` are the same
   * hex in both themes. The shipped steps measure 1.07 to 1.18; the floor is
   * 1.04 because it sits comfortably under the smallest of them and comfortably
   * over nothing at all.
   */
  it.each(MUST_REACT_TO_HOVER)("%s visibly changes under the pointer", (variant) => {
    for (const [themeName, theme] of Object.entries(THEMES)) {
      const u = utilities(variant);
      const hover = surfaceIn(u, "hover", theme) as string;
      const rest = surfaceIn(u, "", theme);
      // With no fill of its own, what a hover replaces is the pane beneath it.
      const replaced = rest ?? theme.card;
      expect(
        contrast(hover, replaced),
        `${variant} hover ${hover} over ${replaced} (${themeName})`,
      ).toBeGreaterThanOrEqual(1.04);
    }
  });

  it("outline is the raised surface wearing the app's hairline", () => {
    // The owner's complaint, stated as arithmetic: the fill is the token
    // DESIGN.md calls `surface-raised`, not a composite that lands near it.
    for (const theme of Object.values(THEMES)) {
      const u = utilities("outline");
      expect(surfaceIn(u, "", theme)).toBe(theme.secondary);
      expect(edgeIn(u, "")).toBe("border");
    }
  });

  it("destructive is red by its edge and its label, and never by a tint", () => {
    // Tinting with the label's own hue moves the surface toward the text, so the
    // ratio can only fall: `bg-destructive/10` measured 4.18:1 on `--secondary`.
    for (const theme of Object.values(THEMES)) {
      const u = utilities("destructive");
      expect(surfaceIn(u, "", theme)).toBeNull();
      expect(labelIn(u, "", theme)).toBe(theme.destructive);
      expect(edgeIn(u, "")).toBe("destructive");
      // The hover shares no hue with the label, which is what makes the label's
      // contrast independent of the hover state.
      expect(surfaceIn(u, "hover", theme)).toBe(theme.secondary);
    }
  });
});

describe("the button itself", () => {
  it("reports its variant and size on the element", () => {
    render(<Button variant="outline" size="sm" />);
    const button = screen.getByRole("button");
    expect(button).toHaveAttribute("data-variant", "outline");
    expect(button).toHaveAttribute("data-size", "sm");
  });

  it("hands its styling to the child under asChild", () => {
    render(
      <Button asChild variant="link">
        <a href="/somewhere">go</a>
      </Button>,
    );
    const link = screen.getByRole("link", { name: "go" });
    expect(link).toHaveAttribute("data-slot", "button");
    expect(link.tagName).toBe("A");
  });

  it("lets a call site override the variant's colour rather than stacking on it", () => {
    render(
      <Button variant="outline" className="bg-primary">
        save
      </Button>,
    );
    const button = screen.getByRole("button");
    expect(button).toHaveClass("bg-primary");
    expect(button).not.toHaveClass("bg-secondary");
  });
});

/**
 * `aria-expanded` is two different facts wearing one attribute, and the variant
 * may only paint one of them.
 *
 * On a popover trigger `true` is transient: a menu is open right now, and a
 * pressed look is the point. On a disclosure `true` is the RESTING state — an
 * unfolded column is expanded, and unfolded is how the app opens — so painting
 * it fills every fold control in the window on first paint and inverts the
 * signal: the controls that look pressed are the ones with nothing open.
 */
describe("the open-state fill belongs to menus, not to disclosures", () => {
  const OPEN_STATE = /aria-\[haspopup\]:aria-expanded:/;

  it.each(VARIANTS)("%s only paints an open state behind aria-[haspopup]", (variant) => {
    for (const cls of buttonVariants({ variant }).split(" ")) {
      if (!cls.includes("aria-expanded:")) continue;
      expect(cls, `${variant} paints an open state on every disclosure`).toMatch(OPEN_STATE);
    }
  });

  /**
   * The spelling is the other half of the trap, and it fails silently rather
   * than loudly. `aria-haspopup:` is not one of Tailwind's `aria-*` variants, so
   * it falls back to the boolean form and compiles to `[aria-haspopup="true"]` —
   * a perfectly good-looking rule that matches nothing, because the attribute is
   * not a boolean. The two assertions below are one measurement in two halves:
   * what the app actually writes, and what the variant actually asks for.
   */
  it("Radix writes a VALUE on haspopup, never the boolean the shorthand expects", () => {
    render(
      <>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost">menu</Button>
          </DropdownMenuTrigger>
        </DropdownMenu>
        <Popover>
          <PopoverTrigger asChild>
            <Button variant="ghost">pop</Button>
          </PopoverTrigger>
        </Popover>
      </>,
    );
    const values = ["menu", "pop"].map((name) =>
      screen.getByRole("button", { name }).getAttribute("aria-haspopup"),
    );
    expect(values).toEqual(["menu", "dialog"]);
    expect(values, "the boolean spelling would match none of these").not.toContain("true");
  });

  it("no variant uses the boolean shorthand that would match none of them", () => {
    for (const variant of VARIANTS) {
      expect(buttonVariants({ variant }), `${variant} closes its own gate`).not.toMatch(
        /(^|\s)aria-haspopup:/,
      );
    }
  });

  it("a disclosure carries no haspopup, so the gate excludes it", () => {
    render(
      <Button variant="ghost" aria-expanded={true}>
        unfolded
      </Button>,
    );
    const disclosure = screen.getByRole("button", { name: "unfolded" });
    expect(disclosure).toHaveAttribute("aria-expanded", "true");
    expect(disclosure).not.toHaveAttribute("aria-haspopup");
  });
});
