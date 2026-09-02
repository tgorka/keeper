//! The identity a bot wears, and the hand order the strip draws it in (Story
//! 61.7, FR-383).
//!
//! Two decisions live here, and both are refusals.
//!
//! **The colour set is closed, and it is a set of token NAMES.** `DESIGN.md`
//! records the arithmetic that forces this: AA needs L\* ≤ 46.8 on warm paper
//! and L\* ≥ 51.9 on near-black, and the intersection is empty, so no colour of
//! any hue passes in both themes. A free colour picker therefore cannot be
//! written honestly — half of what a person could pick would be unreadable in
//! one of the two themes keeper ships, and the app would have let them pick it.
//! What is stored is a name out of [`BOT_COLOURS`]; the hexes live in
//! `src/index.css` and `scripts/check-design.mjs` recomputes every one of them
//! against every surface of both themes. A name this build does not know is
//! refused rather than kept, because a stored colour nothing can draw is a
//! colour the picker shows as chosen and the strip shows as absent.
//!
//! **A colour is never the only carrier.** `DESIGN.md` requires colour to be
//! paired with a shape, so a colour arriving without one is refused here — not
//! silently dropped, and not accepted to be drawn as a bare coloured dot. That
//! is the whole reason [`BOT_SHAPES`] is a closed set too: the shape is what a
//! person who cannot tell clay from olive reads the bot by, together with its
//! mark.
//!
//! The hand order gets [`plan_reorder`] for the same family of reason. The pins
//! strip learned it first (`registry::reorder_pins`): a reorder rewrites the
//! WHOLE sequence, so an order that is not a permutation of what exists would
//! renumber some rows and leave the rest at their old positions — duplicated
//! and gapped `pin_order` values that no longer describe any order anybody
//! asked for. Validating first is what makes the write in
//! [`crate::bots::store::reorder_bots`] a single transaction over a sequence
//! already known to be complete.

use crate::bots::BotIdentity;

/// The closed set of identity shapes, in picker order.
///
/// The lamp's own fill language (`DESIGN.md` → Shapes: filled, hollow, dashed,
/// and one with a bite taken out of it), worn on the mark's hexagonal cell
/// rather than on the lamp's 6px disc. Reused rather than invented, because a
/// second shape vocabulary is how a UI starts looking slightly wrong without
/// anybody being able to say why — and worn on the cell rather than the disc so
/// that a bot's identity can never be misread as a status: a status in this app
/// is round and 6px, and a bot is a cell.
///
/// Every one of the four survives a greyscale screenshot, which is the property
/// that makes the shape — not the colour — the identity's primary carrier.
pub const BOT_SHAPES: [&str; 4] = ["filled", "hollow", "dashed", "notched"];

/// The bounded colour palette, as token names, in wheel order.
///
/// Seven, and each one a material a workroom actually holds. The wheel is
/// deliberately missing the lichen band (OKLCH hue 110–160): the accent is
/// singular by rule (`DESIGN.md` → "No second green"), so no bot may wear it.
/// Purple (hue 260–330) is absent for the same reason it is absent everywhere
/// else in the product — it is banned outright, and `scripts/check-design.mjs`
/// re-checks that too.
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

/// The longest a literal mark may be, in `char`s.
///
/// Four rather than one, and it is not a licence to store a word: one grapheme
/// cluster is routinely several `char`s — a base plus a variation selector, or
/// a letter plus a combining accent — and this crate has no grapheme
/// segmenter and is not gaining a dependency for one. Four `char`s is enough
/// for one mark and far too few for a label, which is the property that
/// matters: the mark is drawn inside a 20px cell, where a second glyph is
/// already illegible.
pub const MAX_MARK_CHARS: usize = 4;

/// The longest an icon name may be. `space-icons.ts`' longest key is well
/// inside this; the bound exists so an unbounded string cannot be stored in a
/// column the picker will try to resolve on every read.
pub const MAX_ICON_NAME_CHARS: usize = 32;

/// Why a chosen identity was refused (Story 61.7).
///
/// One variant per refusable shape, each naming what happened and what to do —
/// the sentences are rendered verbatim, as `BaseUrlError`'s are.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BotIdentityError {
    /// A shape name this build cannot draw.
    #[error("keeper draws bots as one of {shapes} — it has no shape called {name}")]
    UnknownShape {
        /// What was asked for.
        name: String,
        /// The closed set, comma-separated, so the sentence can show it.
        shapes: String,
    },

    /// A colour name outside [`BOT_COLOURS`].
    ///
    /// The bounded palette IS the story: every member has been contrast-checked
    /// against both themes, and a name from outside it names no verified hex.
    #[error(
        "the bot palette is {colours} — {name} is not in it, and keeper only offers colours it has checked against both themes"
    )]
    UnknownColour {
        /// What was asked for.
        name: String,
        /// The closed palette, comma-separated.
        colours: String,
    },

    /// A colour with no shape beside it.
    #[error(
        "a colour needs a shape beside it — colour alone is not something everyone can see, so choose a shape too"
    )]
    ColourWithoutShape,

    /// A literal mark longer than [`MAX_MARK_CHARS`].
    #[error(
        "a mark is one character, or the name of an icon — {0} is too long to draw inside a bot's cell"
    )]
    MarkTooLong(String),

    /// A mark carrying whitespace or a control character: it would draw as a
    /// hole in the cell, or as nothing at all.
    #[error("a mark cannot contain a space or a control character — {0} would draw as a gap")]
    MarkNotDrawable(String),
}

/// Why a hand order was refused (Story 61.7).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BotOrderError {
    /// An id naming no bot. Refused rather than skipped: an order containing a
    /// stranger is an order computed from a stale list, and the rest of it
    /// cannot be trusted either.
    #[error("keeper cannot reorder bots it does not have — {0} names none of them")]
    UnknownBot(String),

    /// The same bot twice.
    #[error("{0} appears twice in the new order — a bot has one place in it")]
    DuplicateBot(String),

    /// Fewer ids than there are bots. The write rewrites the whole sequence, so
    /// a partial order would renumber some rows and leave others where they
    /// were.
    #[error(
        "a reorder rewrites the whole order, so it needs all {expected} bots — this one names {given}"
    )]
    Partial {
        /// How many bots exist.
        expected: usize,
        /// How many the order named.
        given: usize,
    },
}

/// Whether `name` is a shape this build draws.
pub fn is_bot_shape(name: &str) -> bool {
    BOT_SHAPES.contains(&name)
}

/// Whether `name` is a member of the bounded palette.
pub fn is_bot_colour(name: &str) -> bool {
    BOT_COLOURS.contains(&name)
}

/// Whether `mark` reads as an icon name rather than as a literal glyph.
///
/// The two are stored in one column because they are one concept — the thing
/// drawn inside the cell — and the frontend resolves it the same way
/// `spaceIcon` already resolves a Space's stored icon: a name it knows draws
/// that glyph, anything else draws as itself. Lowercase ASCII with hyphens is
/// exactly lucide's spelling, and no single grapheme can be mistaken for it
/// because an icon name is at least two characters.
pub fn is_icon_mark(mark: &str) -> bool {
    mark.len() >= 2
        && mark.len() <= MAX_ICON_NAME_CHARS
        && mark.starts_with(|c: char| c.is_ascii_lowercase())
        && mark
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !mark.ends_with('-')
}

/// Validate a chosen identity, or refuse it with a sentence.
///
/// Empty and whitespace-only fields read as "not chosen", so clearing a field
/// in the picker clears the column — the setter writes all three every time
/// ([`crate::bots::store::set_bot_identity`]) precisely so that "no colour" is
/// still reachable once a colour has been chosen.
pub fn parse_identity(
    shape: Option<&str>,
    colour: Option<&str>,
    mark: Option<&str>,
) -> Result<BotIdentity, BotIdentityError> {
    let shape = chosen(shape);
    let colour = chosen(colour);
    let mark = chosen(mark);

    if let Some(name) = shape.as_deref() {
        if !is_bot_shape(name) {
            return Err(BotIdentityError::UnknownShape {
                name: name.to_owned(),
                shapes: BOT_SHAPES.join(", "),
            });
        }
    }
    if let Some(name) = colour.as_deref() {
        if !is_bot_colour(name) {
            return Err(BotIdentityError::UnknownColour {
                name: name.to_owned(),
                colours: BOT_COLOURS.join(", "),
            });
        }
        // `DESIGN.md`: colour is paired with a shape or it is not carried at
        // all. Refused here rather than dropped, so the picker can say why
        // instead of appearing to have saved something it did not.
        if shape.is_none() {
            return Err(BotIdentityError::ColourWithoutShape);
        }
    }
    if let Some(value) = mark.as_deref() {
        if value.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(BotIdentityError::MarkNotDrawable(value.to_owned()));
        }
        if !is_icon_mark(value) && value.chars().count() > MAX_MARK_CHARS {
            return Err(BotIdentityError::MarkTooLong(value.to_owned()));
        }
    }
    Ok(BotIdentity {
        shape,
        colour,
        mark,
    })
}

/// A trimmed, non-empty choice, or `None`.
fn chosen(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

/// Check that `order` is a permutation of `known`, and hand it back.
///
/// Called before the write and not inside it, so a refused order writes
/// nothing at all — the invariant the single transaction in
/// [`crate::bots::store::reorder_bots`] then keeps under a failure it cannot
/// see coming.
pub fn plan_reorder(known: &[String], order: &[String]) -> Result<Vec<String>, BotOrderError> {
    let mut seen: Vec<&str> = Vec::with_capacity(order.len());
    for id in order {
        if !known.iter().any(|k| k == id) {
            return Err(BotOrderError::UnknownBot(id.clone()));
        }
        if seen.contains(&id.as_str()) {
            return Err(BotOrderError::DuplicateBot(id.clone()));
        }
        seen.push(id.as_str());
    }
    if order.len() != known.len() {
        return Err(BotOrderError::Partial {
            expected: known.len(),
            given: order.len(),
        });
    }
    Ok(order.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).to_owned()).collect()
    }

    #[test]
    fn the_palette_is_closed_and_bounded() {
        // 6-10 entries is the story's bound: fewer and two bots collide too
        // often, more and the names stop being memorable.
        assert!(BOT_COLOURS.len() >= 6 && BOT_COLOURS.len() <= 10);
        assert!(BOT_SHAPES.len() >= 4 && BOT_SHAPES.len() <= 6);
        for colour in BOT_COLOURS {
            assert!(is_bot_colour(colour), "{colour} must be its own member");
        }
        assert!(!is_bot_colour("hotpink"));
        assert!(!is_bot_colour("emerald"));
    }

    #[test]
    fn an_unknown_colour_token_is_refused_with_the_palette() {
        let err = parse_identity(Some("filled"), Some("hotpink"), None).expect_err("refused");
        assert_eq!(
            err,
            BotIdentityError::UnknownColour {
                name: "hotpink".to_owned(),
                colours: BOT_COLOURS.join(", "),
            }
        );
        // The sentence shows the closed set, so the next attempt can be right.
        assert!(err.to_string().contains("verdigris"));
    }

    #[test]
    fn an_unknown_shape_is_refused() {
        let err = parse_identity(Some("blob"), None, None).expect_err("refused");
        assert!(matches!(err, BotIdentityError::UnknownShape { .. }));
    }

    #[test]
    fn a_colour_without_a_shape_is_refused() {
        assert_eq!(
            parse_identity(None, Some("clay"), Some("bot")).expect_err("refused"),
            BotIdentityError::ColourWithoutShape
        );
    }

    #[test]
    fn a_colour_with_a_shape_round_trips() {
        let identity = parse_identity(Some("hollow"), Some("clay"), Some("flask-conical"))
            .expect("a paired identity is accepted");
        assert_eq!(identity.shape.as_deref(), Some("hollow"));
        assert_eq!(identity.colour.as_deref(), Some("clay"));
        assert_eq!(identity.mark.as_deref(), Some("flask-conical"));
    }

    #[test]
    fn blank_fields_clear_the_identity() {
        let identity = parse_identity(Some("  "), Some(""), None).expect("blank is not a choice");
        assert!(identity.is_empty());
    }

    #[test]
    fn a_shape_alone_is_a_whole_identity() {
        let identity = parse_identity(Some("dashed"), None, None).expect("shape alone");
        assert_eq!(identity.shape.as_deref(), Some("dashed"));
        assert!(identity.colour.is_none());
    }

    #[test]
    fn a_one_grapheme_mark_is_kept_and_a_word_is_refused() {
        assert_eq!(
            parse_identity(None, None, Some("K"))
                .expect("a letter is a mark")
                .mark
                .as_deref(),
            Some("K")
        );
        // A combining sequence is several `char`s and one mark.
        let accented = parse_identity(None, None, Some("e\u{301}")).expect("one cluster");
        assert_eq!(accented.mark.as_deref(), Some("e\u{301}"));
        assert!(matches!(
            parse_identity(None, None, Some("RESEARCH")).expect_err("refused"),
            BotIdentityError::MarkTooLong(_)
        ));
    }

    #[test]
    fn a_mark_with_a_space_is_refused() {
        assert!(matches!(
            parse_identity(None, None, Some("a b")).expect_err("refused"),
            BotIdentityError::MarkNotDrawable(_)
        ));
    }

    #[test]
    fn an_icon_name_is_told_apart_from_a_literal_mark() {
        assert!(is_icon_mark("flask-conical"));
        assert!(is_icon_mark("mic"));
        assert!(!is_icon_mark("K"));
        assert!(!is_icon_mark("Mic"));
        assert!(!is_icon_mark("mic-"));
        assert!(!is_icon_mark(&"a".repeat(MAX_ICON_NAME_CHARS + 1)));
    }

    #[test]
    fn reorder_accepts_a_permutation() {
        let known = ids(&["a", "b", "c"]);
        assert_eq!(
            plan_reorder(&known, &ids(&["c", "a", "b"])).expect("a permutation"),
            ids(&["c", "a", "b"])
        );
    }

    #[test]
    fn reorder_refuses_a_partial_order() {
        let known = ids(&["a", "b", "c"]);
        assert_eq!(
            plan_reorder(&known, &ids(&["b", "a"])).expect_err("refused"),
            BotOrderError::Partial {
                expected: 3,
                given: 2
            }
        );
    }

    #[test]
    fn reorder_refuses_a_stranger_and_a_duplicate() {
        let known = ids(&["a", "b"]);
        assert_eq!(
            plan_reorder(&known, &ids(&["a", "z"])).expect_err("refused"),
            BotOrderError::UnknownBot("z".to_owned())
        );
        assert_eq!(
            plan_reorder(&known, &ids(&["a", "a"])).expect_err("refused"),
            BotOrderError::DuplicateBot("a".to_owned())
        );
    }
}
