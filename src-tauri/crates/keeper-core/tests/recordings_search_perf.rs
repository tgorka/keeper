//! CI performance gate for Story 42.2 (FR-140, AC3): searching a 10 000-session
//! recordings archive must return inside the budget recorded here.
//!
//! Story 42.1 made a session a row and 42.2 made the row findable. The epic's
//! third acceptance criterion is not "search works" — the matrix row already
//! covers that — it is that search still works *at scale*, and that the scale it
//! was proven at is written down where a later change has to walk past it. This
//! file is that record.
//!
//! **It is a normal test, not a `#[ignore]`d one and not a `criterion` bench**,
//! for the same reason `archive_search_perf.rs` (Story 5.3/11.3) and
//! `cold_start_perf.rs` are: a gate nobody runs guards nothing. The corpus is
//! built once by bulk-seeding in a single transaction — we are measuring *query*
//! latency, not build throughput, so a few seconds to seed is acceptable — and
//! then a representative mix of query shapes is timed against a fresh read-only
//! connection, exactly as a Story 42.3 surface will open one.
//!
//! **Seeded through the production write path.** Every row goes in through
//! [`upsert_recording`] rather than a hand-written `INSERT`, so the index
//! maintenance 42.2 added to that function is exercised 10 000 times and the
//! thing being searched is the index the app actually builds. The events gate
//! can get away with a bulk insert plus one FTS5 `'rebuild'` because
//! `events_fts` is an external-content table that can be rebuilt from its
//! content table; `recordings_fts` deliberately is NOT one (the spec's "Never"
//! list, and DW-48 records the desynchronisation hazard external content
//! carries), so it owns its own copy of the text and the only way to fill it is
//! the write path. That is a feature of this bench, not a cost: a regression
//! that breaks incremental index maintenance shows up here as a search that
//! finds nothing, and the non-empty assertions below turn that into a failure
//! rather than a suspiciously fast pass.
//!
//! **The seed runs inside one caller-managed transaction.** That is supported
//! rather than merely tolerated: `recordings::in_transaction` is reentrancy-safe
//! — it checks `Connection::is_autocommit` and, when a transaction is already
//! open, simply runs its closure and lets the outer transaction own atomicity.
//! [`upsert_recording`] wraps its row write and its index write in that helper,
//! so at top level it is one transaction and inside the `BEGIN` below it adds no
//! nested one. It has to be that way for production too: `write_rebuilt_session`
//! already calls [`upsert_recording`] from inside a transaction of its own.
//! Ten thousand autocommit writes against a WAL database at the default
//! `synchronous=FULL` would be ten thousand fsyncs, and a bench that spends
//! minutes seeding is a bench that gets marked `#[ignore]` within a month.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use keeper_core::archive::db::{open_archive_db, open_readonly_archive_db};
use keeper_core::archive::recordings::{durability_label, upsert_recording, RecordingRow};
use keeper_core::archive::recordings_fts::{search_recordings, RecordingFilter};
use keeper_core::vm::RecordingDurabilityState;
use rusqlite::Connection;

/// Corpus size — the epic's own scale figure for AC3.
const CORPUS: i64 = 10_000;

/// The first session's start instant: 2024-01-01T00:00:00Z, in ms since the Unix
/// epoch. An arbitrary but fixed point, so the date-range shape below names the
/// same window on every machine and in every year.
const BASE_TS: i64 = 1_704_067_200_000;

/// Two hours between consecutive sessions, so the corpus spreads over roughly
/// 833 days rather than piling every row into one afternoon. A date-range
/// predicate against a corpus with no spread is a predicate that selects
/// everything or nothing, and neither measures the index.
const STEP_MS: i64 = 7_200_000;

/// The p95 latency budget for a search over 10 000 sessions.
///
/// **MEASURED, on the machine named below, not derived.** The first draft of
/// this file carried an estimate and said so; these are the numbers that
/// replaced it.
///
/// Reference reading — `electra`, the Linux dev container (6 CPU cgroup quota,
/// 8 GiB, `cargo test -p keeper-core --test recordings_search_perf --
/// --nocapture`, dev profile, the same unoptimised build CI runs):
///
/// ```text
/// recording search p95 32.427443ms (max 33.81999ms, n=100) over 10000 sessions
/// per-shape worst:
///   free text, many hits        10.098585ms
///   free text, few hits            420.661µs
///   sub-trigram LIKE fallback    33.81999ms   <- the shape that sets the budget
///   tag prefix                    1.670069ms
///   date range + durability        668.737µs
/// ```
///
/// So the whole gate is really a promise about the fallback: the FTS-served
/// shapes finish in single-digit milliseconds or less, and the `< 3`-character
/// path — which by construction cannot use the trigram index and must scan —
/// costs 33 ms over this corpus. Everything else is noise beside it.
///
/// 50 ms is that p95 with about 1.5x headroom. Tight enough that losing the
/// index, or making a predicate non-sargable, turns the fallback's 33 ms into a
/// full-table scan and blows straight through; loose enough that a cold page-in
/// or a noisy neighbour does not flake a clean commit — and the machine scaling
/// below covers the rest of that.
///
/// The number is in the test's name on purpose. A budget you have to rename a
/// test to change is a budget nobody edits absent-mindedly to make CI quiet.
const BUDGET: Duration = Duration::from_millis(50);

/// How many full aggregate scans of `recordings` make up one yardstick reading.
///
/// Twelve, so the yardstick visits 120 000 rows — the same row count the events
/// gate's single scan visits, which is what makes [`REFERENCE_SCAN`] and its
/// 32 ms cousin over there comparable numbers rather than two unrelated
/// constants that happen to share a name. One scan of a 10 000-row table lands
/// in the low single-digit milliseconds, where scheduler noise is a larger term
/// than the work, and a yardstick that jitters hands out slack at random.
const SCAN_PASSES: usize = 12;

/// The yardstick's median on reference hardware.
///
/// The budget above is a promise about what a person experiences, so it cannot
/// simply be raised until CI stops complaining. But an absolute wall-clock
/// assertion on a shared runner is not measuring keeper at all — Story 11.3
/// learned that the hard way when the same commit passed at 150 ms and failed at
/// 266 ms on consecutive runs of identical code, because GitHub's macOS runners
/// are shared and several times slower than the machine the number was set on.
/// Scaling the budget by how much slower THIS box is keeps the gate honest in
/// both directions: a regression still fails everywhere, and a slow neighbour no
/// longer fails a clean commit. `archive_search_perf.rs` carries the same
/// mechanism, and this is deliberately the same mechanism rather than a second
/// one that could disagree with it.
///
/// **MEASURED on the same run as [`BUDGET`]:** the yardstick medianed
/// **49.34 ms** on `electra`, against the 45 ms that had been estimated from the
/// events gate's 32 ms — the estimate was 10% low, which is the direction that
/// hands out slack, so it is replaced here with the observed value. A machine
/// that scans as fast as this one now scores exactly 1.00x and gets the raw
/// 50 ms budget; a slower one is scaled up, and a faster one floors at 1.0 and
/// is held to the raw budget rather than being handed a stricter one it never
/// agreed to.
const REFERENCE_SCAN: Duration = Duration::from_millis(49);

/// A scratch directory no sibling test can collide with.
///
/// The process-wide counter is not decoration: six helpers once named their temp
/// directory from the pid plus a nanosecond stamp, two threads asked inside one
/// clock tick, and `cargo test --workspace` failed on macOS with a duplicate
/// column and a UNIQUE violation that looked like schema bugs. A counter makes
/// the name unique per CALL, which is what the helpers in `archive/db.rs` and
/// `recording.rs` already do.
fn temp_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "keeper-recordings-search-perf-{tag}-{}-{}-{n}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    dir
}

/// Removes the seeded temp dir on scope exit — including on an assertion unwind —
/// so a failing gate never leaks its 10 000-session archive in the system temp
/// dir. Copied from `cold_start_perf.rs` for the same reason it exists there.
struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The title of session `i`.
///
/// A rotating vocabulary keeps the trigram index varied instead of storing one
/// string ten thousand times, which would compress into a single posting list
/// and measure nothing. `"review"` appears in one of seven titles, which is what
/// makes the "many hits" shape below genuinely return many. One arm is CJK
/// because the trigram tokenizer is language-agnostic by construction (any
/// 3-scalar window) and a corpus of pure ASCII would never prove it.
fn title_for(i: i64) -> String {
    let base = match i % 7 {
        0 => "weekly design review",
        1 => "onboarding walkthrough",
        2 => "incident review and retrospective",
        3 => "customer discovery call",
        4 => "sprint planning session",
        5 => "架构 评审 会议",
        _ => "pair programming on the importer",
    };
    format!("{base} {i}")
}

/// The free-text note of session `i`.
///
/// `"pricing"` — the spec's own acceptance-criterion word — is sprinkled into one
/// note in five hundred, giving the "few hits" shape a result set of twenty out
/// of ten thousand. A needle that appears in every row and a needle that appears
/// in none both measure the same nothing.
fn note_for(i: i64) -> String {
    let base = match i % 11 {
        0 => "walked through the migration plan and agreed the cutover date",
        1 => "recorded for the people who could not make the call",
        2 => "open question about the retention policy is still open",
        3 => "screen share of the importer failing on a malformed ledger",
        4 => "agreed to split the epic and revisit the estimate next week",
        5 => "went over the incident timeline minute by minute",
        6 => "demo of the new destination picker, mostly positive",
        7 => "notes are rough, the second half is the useful part",
        8 => "follow up with the vendor before the renewal window closes",
        9 => "討論 了 下 一 步 的 計劃",
        _ => "no agenda, just a working session on the backlog",
    };
    if i % 500 == 0 {
        format!("{base}; pricing was the whole conversation")
    } else {
        base.to_owned()
    }
}

/// Who session `i` was with, as the one free-text line the manifest carries.
///
/// Stored as that text's JSON *string* encoding, which is what the
/// `participants_json` column holds today — see the column's note on
/// `ensure_recordings_schema` for why the shape is JSON rather than plain text.
fn participants_json_for(i: i64) -> String {
    const NAMES: [&str; 8] = [
        "Ada Lovelace",
        "Grace Hopper",
        "Katherine Johnson",
        "Barbara Liskov",
        "Radia Perlman",
        "Margaret Hamilton",
        "Karen Spärck Jones",
        "Frances Allen",
    ];
    let first = NAMES[(i % 8) as usize];
    let second = NAMES[((i / 8) % 8) as usize];
    let line = format!("{first}, {second}");
    serde_json::to_string(&line).expect("a string always encodes as JSON")
}

/// The tags of session `i`, as the JSON array of strings the column holds.
///
/// The five-way rotation is built around the spec's tag rule rather than around
/// variety for its own sake: `client/acme/renewal` and `client/acme` must both
/// match `tag:client/acme`, while `client/acmecorp/intro` and
/// `client/other/kickoff` must not. Seeding the near-misses at the same order of
/// magnitude as the matches is what makes the tag shape below do real
/// discrimination work instead of returning everything it looks at.
fn tags_json_for(i: i64) -> String {
    let client = match i % 5 {
        0 => "client/acme/renewal",
        1 => "client/acmecorp/intro",
        2 => "client/other/kickoff",
        3 => "internal/planning",
        _ => "client/acme",
    };
    let quarter = match (i / 5) % 4 {
        0 => "quarter/2026q1",
        1 => "quarter/2026q2",
        2 => "quarter/2026q3",
        _ => "quarter/2026q4",
    };
    serde_json::to_string(&[client, quarter]).expect("a string array always encodes as JSON")
}

/// The row for session `i`: a plausible session, not a placeholder.
///
/// Everything a real row varies over is varied — the durability state cycles all
/// four of epic 41's, a third of the sessions live under a sync profile and the
/// rest in a plain folder, and `started_ts` walks forward two hours at a time —
/// because a corpus where every row agrees on the low-cardinality columns lets
/// SQLite serve the predicates from a single index page and reports a latency no
/// user will ever see.
fn session_row(i: i64) -> RecordingRow {
    let durability = match i % 4 {
        0 => RecordingDurabilityState::Local,
        1 => RecordingDurabilityState::Committed,
        2 => RecordingDurabilityState::Pushed,
        _ => RecordingDurabilityState::Verified,
    };
    let device_id = format!("01DEVICE{:04}", i % 37);
    // Under a sync profile for a third of the corpus, a plain folder for the
    // rest — and `root_kind` and `profile_id` agree, because a "profile" root
    // with no profile id is a row the app cannot produce.
    let profile_id = (i % 3 == 0).then(|| format!("01PROFILE{:04}", i % 9));
    let root_kind = if profile_id.is_some() {
        "profile"
    } else {
        "folder"
    };
    let started_ts = BASE_TS + i * STEP_MS;
    RecordingRow {
        session_id: format!("{device_id}-01SESSION{i:06}"),
        device_id: Some(device_id),
        relative_path: format!("2026/{:02}/session-{i:06}", i % 12 + 1),
        root_kind: root_kind.to_owned(),
        profile_id,
        started_ts: Some(started_ts),
        ended_ts: Some(started_ts + 600_000 + (i % 17) * 60_000),
        title: Some(title_for(i)),
        participants_json: Some(participants_json_for(i)),
        note: Some(note_for(i)),
        tags_json: Some(tags_json_for(i)),
        custom_json: (i % 3 == 1).then(|| {
            serde_json::json!([{ "name": "deal", "value": format!("D-{i:06}") }]).to_string()
        }),
        codec: Some(if i % 2 == 0 { "h264" } else { "hevc" }.to_owned()),
        width: None,
        height: None,
        fps: Some(if i % 2 == 0 { 30 } else { 60 }),
        durability: durability_label(durability).to_owned(),
        manifest_version: 1,
    }
}

/// Seed the corpus through the production write path, in one transaction, and
/// checkpoint the WAL so a fresh read-only connection sees every row.
fn seed_sessions(dir: &Path) {
    let conn = open_archive_db(dir).expect("open archive.db for the seed");
    conn.execute_batch("BEGIN")
        .expect("begin the seed transaction");
    for i in 0..CORPUS {
        upsert_recording(&conn, &session_row(i)).expect("seed one session row");
    }
    conn.execute_batch("COMMIT")
        .expect("commit the seed transaction");
    conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")
        .expect("checkpoint the seeded WAL");
    drop(conn);
}

/// Time one search, returning its wall-clock latency.
fn time_search(conn: &Connection, filter: &RecordingFilter) -> Duration {
    let start = Instant::now();
    let hits = search_recordings(conn, filter).expect("search recordings");
    let elapsed = start.elapsed();
    // Touch the result so the query is not optimized away.
    std::hint::black_box(hits.len());
    elapsed
}

/// Time the yardstick: [`SCAN_PASSES`] full scans of `recordings`, aggregating a
/// value the planner cannot serve from an index.
///
/// This touches the same subsystem as the queries under test — the same pages,
/// the same SQLite, the same disk — but none of the code the gate is guarding,
/// so it measures the MACHINE and nothing else. The `COALESCE`es are not
/// defensive noise: `LENGTH(NULL)` is `NULL` and `NULL + x` is `NULL`, so a
/// single unset column would make `SUM` skip the row, and a corpus that ever
/// grows an all-`NULL` column would make it return `NULL` and fail the typed
/// `get` instead of reporting a scan.
fn time_scan(conn: &Connection) -> Duration {
    let start = Instant::now();
    let mut checksum: i64 = 0;
    for _ in 0..SCAN_PASSES {
        let (rows, bytes): (i64, i64) = conn
            .query_row(
                "SELECT COUNT(*), SUM(\
                     LENGTH(COALESCE(title, '')) + LENGTH(COALESCE(note, '')) \
                     + LENGTH(COALESCE(tags_json, ''))\
                 ) FROM recordings",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("scan the recordings table");
        checksum = checksum.wrapping_add(rows).wrapping_add(bytes);
    }
    let elapsed = start.elapsed();
    std::hint::black_box(checksum);
    elapsed
}

/// The representative mix, each shape labelled so the printout names the one
/// that got slow rather than leaving a bare percentile.
///
/// Five shapes, chosen because each one exercises a different part of
/// `search_recordings` and a regression in any of them is a regression a user
/// would feel:
///
/// - **Free text, many hits.** The trigram index at its widest: `"review"` is in
///   one title in seven, so the query has to rank and cut a four-figure candidate
///   set down to the bounded result the caller gets.
/// - **Free text, few hits.** The same path at its narrowest — twenty rows out of
///   ten thousand. Cheap when the index is doing its job, and catastrophic when
///   it is not, which is exactly what makes it worth timing.
/// - **Sub-trigram `LIKE` fallback.** Two Unicode scalars, below
///   `TRIGRAM_MIN_CHARS`, so no index can serve it and every row's text is
///   scanned. `"re"` rather than a rare bigram on purpose: the slowest thing this
///   subsystem can be asked to do is a full scan that also matches nearly
///   everything it reads, and a budget is only worth having on the worst case.
/// - **Tag prefix.** The hierarchical segment-boundary match, against a corpus
///   seeded with two near-misses (`client/acmecorp/…`, `client/other/…`) for
///   every match.
/// - **Date range + durability, empty query.** The pure-predicate path, which
///   takes no free text at all and must come off `idx_recordings_durability`
///   rather than a scan. The window is the middle half of the corpus so the range
///   selects rather than waves through.
fn query_shapes() -> Vec<(&'static str, RecordingFilter)> {
    vec![
        (
            "free text, many hits",
            RecordingFilter {
                query: "review".to_owned(),
                ..Default::default()
            },
        ),
        (
            "free text, few hits",
            RecordingFilter {
                query: "pricing".to_owned(),
                ..Default::default()
            },
        ),
        (
            "sub-trigram LIKE fallback",
            RecordingFilter {
                query: "re".to_owned(),
                ..Default::default()
            },
        ),
        (
            "tag prefix",
            RecordingFilter {
                tags: vec!["client/acme".to_owned()],
                ..Default::default()
            },
        ),
        (
            "date range + durability",
            RecordingFilter {
                start_ts: Some(BASE_TS + 2_500 * STEP_MS),
                end_ts: Some(BASE_TS + 7_500 * STEP_MS),
                durability: Some(durability_label(RecordingDurabilityState::Pushed).to_owned()),
                ..Default::default()
            },
        ),
    ]
}

/// Story 42.2's AC3: searching 10 000 synthetic sessions returns within the
/// budget recorded in this bench.
#[test]
fn recording_search_p95_under_50ms_at_10k_sessions() {
    let dir = temp_dir("gate");
    let _guard = TempDirGuard(dir.clone());
    seed_sessions(&dir);
    let conn = open_readonly_archive_db(&dir).expect("open the seeded archive read-only");

    // Sanity: the corpus really is at the epic's scale.
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM recordings", [], |r| r.get(0))
        .expect("count the seeded sessions");
    assert_eq!(
        total, CORPUS,
        "the gate must measure {CORPUS} sessions, not {total}"
    );

    let shapes = query_shapes();

    // Sanity, and the reason this file is a test rather than a bench: a search
    // that returns nothing is instantaneous, so a gate that only timed things
    // would report its best-ever number the day incremental index maintenance
    // broke. Every shape must find something before any of them is timed.
    for (label, filter) in &shapes {
        let hits = search_recordings(&conn, filter).expect("search recordings");
        assert!(
            !hits.is_empty(),
            "the '{label}' shape found nothing in {CORPUS} seeded sessions — the \
             gate would be timing an empty result, which is not the thing it \
             promises to measure"
        );
    }

    // Warm up (open pages/caches) so we measure steady-state latency, then collect
    // a sample of repeated runs for a stable p95.
    for (_, filter) in &shapes {
        let _ = time_search(&conn, filter);
    }
    let mut samples: Vec<Duration> = Vec::new();
    let mut worst_by_shape: Vec<(&'static str, Duration)> = Vec::new();
    for (label, filter) in &shapes {
        let mut worst = Duration::ZERO;
        for _ in 0..20 {
            let elapsed = time_search(&conn, filter);
            worst = worst.max(elapsed);
            samples.push(elapsed);
        }
        worst_by_shape.push((*label, worst));
    }

    samples.sort();
    let p95_index = ((samples.len() as f64) * 0.95).ceil() as usize - 1;
    let p95 = samples[p95_index.min(samples.len() - 1)];
    let max = *samples.last().expect("samples non-empty");

    // Five readings, median taken: one scan can be ambushed by a neighbour on a
    // shared runner, and a yardstick that jitters would hand out slack at random.
    let mut scans: Vec<Duration> = (0..5).map(|_| time_scan(&conn)).collect();
    scans.sort();
    let scan = scans[scans.len() / 2];
    // Floored at 1: hardware faster than the reference earns no slack, it just
    // meets the budget the number was written for.
    let scale = (scan.as_secs_f64() / REFERENCE_SCAN.as_secs_f64()).max(1.0);
    let allowed = BUDGET.mul_f64(scale);
    // A gate whose verdict depends on the machine has to say which machine it
    // measured, so this line prints the reading that set BUDGET and the one this
    // run took. CI runs under nextest, which swallows a passing test's stdout, so
    // read it with `--nocapture`; the assertion message below carries the same
    // numbers for a failing run.
    println!(
        "recording search p95 {p95:?} (max {max:?}, n={}) over {total} sessions; \
         per-shape worst {worst_by_shape:?}; scan {scan:?} vs reference \
         {REFERENCE_SCAN:?} => {scale:.2}x, budget {BUDGET:?} -> {allowed:?}",
        samples.len()
    );
    assert!(
        p95 < allowed,
        "recording search p95 {p95:?} exceeded the {allowed:?} budget for this \
         machine ({BUDGET:?} x {scale:.2}, from a {scan:?} scan against a \
         {REFERENCE_SCAN:?} reference) over {total} sessions; max {max:?}, n={}, \
         per-shape worst {worst_by_shape:?}",
        samples.len()
    );
}
