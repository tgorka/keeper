//! The setup link's QR code, with the keeper mark in the middle.
//!
//! A logo over a QR code covers modules, so the code has to carry enough
//! redundancy to rebuild them: this one is encoded at error-correction level
//! H, which recovers up to 30% of the codewords, and the mark covers about 5%
//! of the symbol ([`LOGO_FRACTION`] of its width, squared). The quiet zone and
//! the three finder patterns are never touched — the mark sits in the centre.
//!
//! A link too long for level H (a very large inline descriptor) still gets a
//! plain code at the default level, which holds almost twice as much, so the
//! person can still scan it. Only a link too long for that is refused, and the
//! sheet then offers the text alone.

use qrcode::{EcLevel, QrCode};

/// The mark's width as a share of the symbol's (quiet zone excluded).
const LOGO_FRACTION: f64 = 0.22;
/// The house green the app icon's tile is drawn in (`--bridge-healthy`, light).
const TILE_GREEN: &str = "#0f6e5c";
/// The paper the hex-bot is drawn in on that tile (`--background`, light).
const MARK_INK: &str = "#f4f2ec";
/// Modules of quiet zone the renderer adds on each side.
const QUIET: usize = 4;

/// The SVG for `link`: branded at level H when it fits, plain at the default
/// level when only that fits, `None` when nothing does.
pub fn setup_qr_svg(link: &str) -> Option<String> {
    match QrCode::with_error_correction_level(link.as_bytes(), EcLevel::H) {
        Ok(code) => Some(with_mark(&code)),
        Err(error) => {
            tracing::debug!(%error, "setup link too long for a branded QR code");
            crate::bridges::login::qr_svg(link)
        }
    }
}

/// The modules the mark's white plate covers along one side: odd, like every
/// QR width, so it centres on the module grid — rounded DOWN to odd, so a
/// small code is never covered more than [`LOGO_FRACTION`] asks — and never
/// fewer than five.
fn logo_modules(width: usize) -> usize {
    // A symbol is at most 177 modules wide, so the float is exact.
    let raw = (width as f64 * LOGO_FRACTION).round() as usize;
    let odd = if raw.is_multiple_of(2) {
        raw.saturating_sub(1)
    } else {
        raw
    };
    odd.max(5)
}

fn with_mark(code: &QrCode) -> String {
    // One SVG unit per module, so the mark is placed in module coordinates.
    // The sheet sizes the image; the SVG scales.
    let svg = code
        .render::<qrcode::render::svg::Color>()
        .quiet_zone(true)
        .module_dimensions(1, 1)
        .build();
    let width = code.width();
    let plate = logo_modules(width);
    let (centre, plate) = ((QUIET as f64) + width as f64 / 2.0, plate as f64);
    let tile = plate * 0.8;
    // The hex-bot hero's 44-unit grid: the cell's outline, stroke included,
    // spans 6..38 around the centre (22, 22).
    let scale = tile * 0.72 / 32.0;
    let mark = format!(
        concat!(
            r#"<g shape-rendering="geometricPrecision">"#,
            r#"<rect x="{px}" y="{px}" width="{p}" height="{p}" rx="{prx}" fill="white"/>"#,
            r#"<rect x="{tx}" y="{tx}" width="{t}" height="{t}" rx="{trx}" fill="{green}"/>"#,
            r#"<g transform="translate({c} {c}) scale({s}) translate(-22 -22)" fill="{ink}">"#,
            r#"<path d="M15 8H29L36 22L29 36H15L8 22Z" fill="none" stroke="{ink}" stroke-width="4" stroke-linejoin="round"/>"#,
            r#"<circle cx="17" cy="19" r="2.8"/><circle cx="27" cy="19" r="2.8"/>"#,
            r#"<path d="M17 24.6Q22 29.2 27 24.6L27 27.2Q22 31.8 17 27.2Z"/>"#,
            r#"</g></g>"#,
        ),
        px = centre - plate / 2.0,
        p = plate,
        prx = plate * 0.18,
        tx = centre - tile / 2.0,
        t = tile,
        trx = tile * 0.22,
        c = centre,
        s = scale,
        green = TILE_GREEN,
        ink = MARK_INK,
    );
    match svg.strip_suffix("</svg>") {
        Some(body) => format!(r#"{body}<title>keeper</title>{mark}</svg>"#),
        // The renderer always closes its document; were that ever to change,
        // a plain level-H code is still a working code.
        None => svg,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINK: &str =
        "keeper://setup?descriptor=https%3A%2F%2Felectra.siren-alsephina.ts.net%2Fkeeper%2Faccount.json";

    fn view_box_side(svg: &str) -> usize {
        let start = svg.find(r#"viewBox="0 0 "#).expect("a viewBox") + r#"viewBox="0 0 "#.len();
        svg[start..]
            .split_whitespace()
            .next()
            .and_then(|side| side.parse().ok())
            .expect("a numeric side")
    }

    #[test]
    fn a_setup_link_gets_the_mark_on_a_level_h_code() {
        let svg = setup_qr_svg(LINK).expect("the link fits");
        assert!(svg.contains(TILE_GREEN), "the mark is drawn: {svg}");
        assert!(svg.trim_end().ends_with("</svg>"));
        // The symbol is the level-H one: a level-M code of the same link is
        // smaller, and covering its modules would not be recoverable.
        let h = QrCode::with_error_correction_level(LINK.as_bytes(), EcLevel::H).expect("fits H");
        assert_eq!(view_box_side(&svg), h.width() + 2 * QUIET);
    }

    #[test]
    fn the_mark_stays_well_inside_what_level_h_recovers() {
        // Level H rebuilds up to 30% of the codewords; the mark plus the
        // format/alignment damage it can cause must stay far below that.
        for width in (21..=177).step_by(4) {
            let plate = logo_modules(width);
            let covered = (plate * plate) as f64 / (width * width) as f64;
            assert!(
                covered <= 0.07,
                "width {width}: plate {plate} covers {covered}"
            );
            assert!(
                !plate.is_multiple_of(2),
                "width {width}: plate {plate} does not centre"
            );
            assert!(
                plate + 14 <= width,
                "width {width}: plate reaches the finder patterns"
            );
        }
    }

    #[test]
    fn a_link_too_long_for_level_h_still_scans_plain() {
        // Level H holds at most 1273 bytes, the default level 2331.
        let long = format!("keeper://setup?d={}", "a".repeat(1600));
        let svg = setup_qr_svg(&long).expect("fits the default level");
        assert!(
            !svg.contains(TILE_GREEN),
            "no mark without level H's redundancy"
        );
    }

    #[test]
    fn a_link_too_long_for_any_code_has_none() {
        let huge = format!("keeper://setup?d={}", "a".repeat(3000));
        assert!(setup_qr_svg(&huge).is_none());
    }
}
