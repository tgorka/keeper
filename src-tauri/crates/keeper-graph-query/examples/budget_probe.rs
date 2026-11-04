//! Separate proof executable for the enacted accounting conversion R (story 110.1).
//! Never shipped; built only with `proof-executables`. Safe code only: it links the
//! worker library, so every allocation goes through the same `Cap<System>` with the
//! same `BUDGET`, and prints one JSON report per case.
//!
//! * `cap-boundary`: a request of exactly the remaining budget succeeds and one byte
//!   more is refused without aborting (exact R acceptance/refusal).
//! * `rust-fallback`: a 32-byte-aligned element type makes std's `System::realloc`
//!   take `realloc_fallback` (aligned alloc, copy, dealloc), on growth and on shrink;
//!   an over-budget growth is refused by Cap before System runs and the old block is
//!   retained intact.
//! * `rust-fallback-failure`: the same align-32 growth, sized well inside the remaining
//!   budget so Cap admits it, is run by the harness under a low `RLIMIT_DATA`: the
//!   aligned allocation inside `realloc_fallback` fails in System, Cap releases its
//!   reservation, and the old block keeps its pointer, capacity, charge and contents.
//!   Without the limit the identical request succeeds (control).
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn main() {
    use keeper_graph_query::allocation::{allocated, BUDGET};
    let case = std::env::args().nth(1).unwrap_or_default();
    let report = match case.as_str() {
        "cap-boundary" => {
            let before = allocated();
            let available = BUDGET - before;
            let mut exact: Vec<u8> = Vec::new();
            let exact_ok = exact.try_reserve_exact(available).is_ok();
            let at_limit = allocated();
            drop(exact);
            let after_release = allocated();
            let mut over: Vec<u8> = Vec::new();
            let over_refused = over.try_reserve_exact(BUDGET - after_release + 1).is_err();
            // Read before the report itself allocates.
            let after_refusal = allocated();
            serde_json::json!({
                "budget": BUDGET, "allocated_before": before, "exact_ok": exact_ok,
                "allocated_at_limit": at_limit, "over_refused": over_refused,
                "allocated_after_refusal": after_refusal,
            })
        }
        "rust-fallback" => {
            #[derive(Clone, Copy, PartialEq)]
            #[repr(align(32))]
            struct Wide([u8; 32]);
            let item = |i: usize| Wide([u8::try_from(i % 251).unwrap_or(0); 32]);
            let base = allocated();
            let mut v: Vec<Wide> = Vec::with_capacity(4);
            v.extend((0..4).map(item));
            let small = allocated() - base;
            // Growth: Layout align 32 > MIN_ALIGN, so System::realloc takes the fallback.
            v.reserve_exact(64);
            let grown_capacity = v.capacity();
            let grown = allocated() - base;
            let grow_preserved = (0..4).all(|i| v[i] == item(i));
            // Shrink through the same fallback.
            v.truncate(2);
            v.shrink_to_fit();
            let shrunk = allocated() - base;
            let shrink_preserved = (0..2).all(|i| v[i] == item(i));
            // Refusal: Cap refuses the growth delta before System is called; the old
            // allocation stays live, charged once and unchanged.
            let before_refusal = (allocated(), v.capacity(), v.as_ptr() as usize);
            let refused = v
                .try_reserve_exact(BUDGET / std::mem::size_of::<Wide>())
                .is_err();
            let after_refusal = (allocated(), v.capacity(), v.as_ptr() as usize);
            let retained = before_refusal == after_refusal && (0..2).all(|i| v[i] == item(i));
            serde_json::json!({
                "align": std::mem::align_of::<Wide>(),
                "charged_small": small, "charged_grown": grown, "grown_capacity": grown_capacity,
                "charged_shrunk": shrunk, "grow_preserved": grow_preserved,
                "shrink_preserved": shrink_preserved, "refused": refused, "retained": retained,
            })
        }
        "rust-fallback-failure" => {
            #[derive(Clone, Copy, PartialEq)]
            #[repr(align(32))]
            struct Wide([u8; 32]);
            let item = |i: usize| Wide([u8::try_from(i % 251).unwrap_or(0); 32]);
            let mut v: Vec<Wide> = Vec::with_capacity(4);
            v.extend((0..4).map(item));
            // 20 MiB of elements: below R minus the live charge, so Cap admits it.
            let additional = 20 * 1024 * 1024 / std::mem::size_of::<Wide>();
            let before = (allocated(), v.capacity(), v.as_ptr() as usize);
            let within_budget = additional * std::mem::size_of::<Wide>() < BUDGET - before.0;
            let failed = v.try_reserve_exact(additional).is_err();
            let after = (allocated(), v.capacity(), v.as_ptr() as usize);
            let retained = before == after && (0..4).all(|i| v[i] == item(i));
            serde_json::json!({
                "align": std::mem::align_of::<Wide>(), "within_budget": within_budget,
                "failed": failed, "retained": retained, "charged_before": before.0,
                "charged_after": after.0, "capacity_after": after.1,
            })
        }
        _ => std::process::exit(2),
    };
    println!("{report}");
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn main() {
    std::process::exit(74);
}
