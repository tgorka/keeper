//! R603-09: `names::same_entry_folded` answers as APFS did on every name pair
//! measured on hesperia — every code point whose canonical caseless key
//! differs from itself (14,369 of them), and the pairs picked by hand around
//! them (`data/apfs-caseless-sweep.txt` says how each was measured).
//!
//! A table of hand-picked pairs missed `ﬃ` and `ﬄ`, whose fold `unicase`
//! emits out of order; this one holds every code point, so a fold that goes
//! wrong for any of them fails here. Asked both ways round, because the
//! comparison must not depend on which side the index or the agent spelled.

use keeper_sync::names::same_entry_folded;

const MEASURED: &str = include_str!("data/apfs-caseless-sweep.txt");

fn name(hex: &str) -> String {
    hex.split('-')
        .map(|point| {
            let point = u32::from_str_radix(point, 16).expect("a code point in hex");
            char::from_u32(point).expect("a scalar value")
        })
        .collect()
}

#[test]
fn the_comparison_answers_as_apfs_did_on_every_measured_pair() {
    let mut wrapped = None;
    let mut pairs = 0;
    let mut wrong = Vec::new();
    for line in MEASURED.lines() {
        match line {
            "" => continue,
            line if line.starts_with('#') => continue,
            "[bare]" => wrapped = Some(false),
            "[x…y]" => wrapped = Some(true),
            line => {
                let wrapped = wrapped.expect("a section before the first pair");
                let fields: Vec<&str> = line.split(' ').collect();
                let [answer, a, b] = fields[..] else {
                    panic!("not `same|apart a b`: {line:?}");
                };
                let apfs = match answer {
                    "same" => true,
                    "apart" => false,
                    _ => panic!("not `same` or `apart`: {line:?}"),
                };
                let (mut a, mut b) = (name(a), name(b));
                if wrapped {
                    (a, b) = (format!("x{a}y"), format!("x{b}y"));
                }
                pairs += 1;
                for (x, y) in [(&a, &b), (&b, &a)] {
                    if same_entry_folded(x, y) != apfs {
                        wrong.push(format!("{x:?} vs {y:?}: APFS said same={apfs}"));
                    }
                }
            }
        }
    }
    // The file is the evidence; a truncated one would pass over nothing.
    assert_eq!(pairs, 14_483, "the measured pairs");
    assert!(wrong.is_empty(), "{} wrong: {wrong:#?}", wrong.len());
}
