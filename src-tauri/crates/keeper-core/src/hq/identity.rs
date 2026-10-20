//! The identity an agent wears in keeper — a shape, a colour and a mark on a
//! hexagonal cell — as the closed sets keeper accepts. `scripts/check-design.mjs`
//! reads both lists here and holds the picker in
//! `src/components/agents/identity.tsx` to the same names; the hexes live once,
//! in `src/index.css`.

/// The closed set of identity shapes, in picker order.
///
/// The lamp's own fill language (`DESIGN.md` → Shapes: filled, hollow, dashed,
/// and one with a bite taken out of it), worn on the mark's hexagonal cell
/// rather than on the lamp's 6px disc. Reused rather than invented, because a
/// second shape vocabulary is how a UI starts looking slightly wrong without
/// anybody being able to say why — and worn on the cell rather than the disc so
/// that an identity can never be misread as a status: a status in this app
/// is round and 6px, and an agent is a cell.
///
/// Every one of the four survives a greyscale screenshot, which is the property
/// that makes the shape — not the colour — the identity's primary carrier.
pub const BOT_SHAPES: [&str; 4] = ["filled", "hollow", "dashed", "notched"];

/// The bounded colour palette, as token names, in wheel order.
///
/// Seven, and each one a material a workroom actually holds. The wheel is
/// deliberately missing the lichen band (OKLCH hue 110–160): the accent is
/// singular by rule (`DESIGN.md` → "No second green"), so no identity may wear
/// it. Purple (hue 260–330) is absent for the same reason it is absent
/// everywhere else in the product — it is banned outright, and
/// `scripts/check-design.mjs` re-checks that too.
///
/// The hex values are NOT here. They live once, in `src/index.css`, as
/// `--bot-ink-*` in both themes, so the design gate is the single authority on
/// what passes AA — this crate has no business holding a second copy of a
/// number the gate would then have to agree with.
pub const BOT_COLOURS: [&str; 7] = [
    "clay",
    "ochre",
    "olive",
    "verdigris",
    "steel",
    "lapis",
    "madder",
];

/// Whether `name` is a shape this build draws.
pub fn is_bot_shape(name: &str) -> bool {
    BOT_SHAPES.contains(&name)
}

/// Whether `name` is a member of the bounded palette.
pub fn is_bot_colour(name: &str) -> bool {
    BOT_COLOURS.contains(&name)
}
