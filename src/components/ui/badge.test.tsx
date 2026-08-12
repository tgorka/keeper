import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { Badge, badgeVariants } from "@/components/ui/badge";
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
 * The Badge is held to the Button's standard because it failed in the same three
 * ways, from the same copied defaults: composite hovers (`bg-primary/80`,
 * `bg-secondary/80`), a `bg-destructive/10` tint drawn in the hue of the label
 * on top of it, and `hover:bg-muted`, which is `--card` in both themes.
 *
 * A badge is only interactive when it is an anchor, which is why most of its
 * hovers are behind an `[a]:` prefix. The measurement does not care about the
 * prefix — a hover that renders has to clear the floor whoever triggers it.
 */

const VARIANTS = ["default", "secondary", "destructive", "outline", "ghost", "link"] as const;
type Variant = (typeof VARIANTS)[number];

const utilities = (variant: Variant) => colourUtilities(badgeVariants({ variant }));

/** `link` answers with an underline; every other variant paints something. */
const MUST_REACT_TO_HOVER = ["default", "secondary", "destructive", "outline", "ghost"] as const;

/** Anchor-only hovers carry the `[a]` prefix; the rest hover unconditionally. */
const HOVER_STATE: Record<Variant, string> = {
  default: "[a]:hover",
  secondary: "[a]:hover",
  destructive: "[a]:hover",
  outline: "[a]:hover",
  ghost: "hover",
  link: "hover",
};

afterEach(cleanup);

describe("what a badge variant is allowed to spend", () => {
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

  it.each(VARIANTS)("%s keeps its label at 4.5:1 at rest", (variant) => {
    for (const [themeName, theme] of Object.entries(THEMES)) {
      const u = utilities(variant);
      const label = labelIn(u, "", theme) ?? theme.foreground;
      const fill = surfaceIn(u, "", theme);
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
      const state = HOVER_STATE[variant];
      const hover = surfaceIn(u, state, theme);
      expect(hover, `${variant} has no hover surface at all`).not.toBeNull();
      const label = labelIn(u, state, theme) ?? labelIn(u, "", theme) ?? theme.foreground;
      expect(
        contrast(label, hover as string),
        `${variant} label ${label} on hover ${hover} (${themeName})`,
      ).toBeGreaterThanOrEqual(TEXT_FLOOR);
    }
  });

  /**
   * `outline` and `ghost` used to hover to `text-muted-foreground`: the ink got
   * QUIETER as the pointer arrived, which runs the affordance backwards.
   *
   * The comparison is deliberately like-for-like on the HOVER surface, because
   * the naive "hover contrast >= rest contrast" is wrong: moving a surface one
   * step toward the foreground legitimately costs a little contrast against a
   * light label (Button's outline goes 13.19 -> 11.93 and is fine). What may
   * never happen is swapping the ink itself for a quieter token.
   */
  it.each(MUST_REACT_TO_HOVER)("%s never swaps its ink for a quieter one", (variant) => {
    for (const [themeName, theme] of Object.entries(THEMES)) {
      const u = utilities(variant);
      const state = HOVER_STATE[variant];
      const restLabel = labelIn(u, "", theme) ?? theme.foreground;
      const hoverLabel = labelIn(u, state, theme) ?? restLabel;
      const hover = surfaceIn(u, state, theme) as string;
      expect(
        contrast(hoverLabel, hover),
        `${variant} ink ${restLabel} -> ${hoverLabel} on ${hover} (${themeName})`,
      ).toBeGreaterThanOrEqual(contrast(restLabel, hover));
    }
  });

  it("destructive is red by its edge and its label, and never by a tint", () => {
    for (const theme of Object.values(THEMES)) {
      const u = utilities("destructive");
      expect(surfaceIn(u, "", theme)).toBeNull();
      expect(labelIn(u, "", theme)).toBe(theme.destructive);
      expect(edgeIn(u, "")).toBe("destructive");
    }
  });
});

describe("the badge itself", () => {
  it("reports its variant on the element", () => {
    render(<Badge variant="destructive">held</Badge>);
    expect(screen.getByText("held")).toHaveAttribute("data-variant", "destructive");
  });

  it("hands its styling to the child under asChild", () => {
    render(
      <Badge asChild>
        <a href="/somewhere">go</a>
      </Badge>,
    );
    const link = screen.getByRole("link", { name: "go" });
    expect(link).toHaveAttribute("data-slot", "badge");
    expect(link.tagName).toBe("A");
  });
});
