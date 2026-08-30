/**
 * The desktop app hosts due tasks on the tick it already owns, and hands its
 * leases back when it quits — never when its window closes (Story 57.5, AD-62,
 * AD-137, NFR-42).
 *
 * **Why a TypeScript test for three Rust files.** `keeper-sync`'s own tests
 * prove the due-gate: `a_due_task_runs_on_the_tick_and_never_on_the_clock_alone`
 * runs the schedule by tick count and never by elapsed time, and
 * `releasing_this_hosts_leases_lets_another_host_claim_the_task` proves the
 * handback. Both run on every host. What they cannot prove is anything about the
 * `keeper` shell, which does not compile on Linux (AD-55, AD-56) — so on the
 * machine this epic was written on, a `tokio::time::interval` added beside the
 * tray tick, or a lease release moved onto the window-close path, is invisible
 * to every gate that runs here.
 *
 * Both are the *same* failure shape this repository has already paid for: a
 * second scheduler over one git repository (AD-62 forbids it by name, and
 * `notes_vault::cadence_tick` rides the tray tick for exactly this reason), and
 * a feature that looks alive while doing the wrong thing. So they are checked
 * where a check actually runs.
 *
 * It follows `tray-notes-labels.test.ts`, `command-registration.test.ts` and
 * `capture-capability.test.ts`, this repo's idiom for an invariant about a Rust
 * file the frontend host cannot build.
 *
 * **What this does NOT prove:** that a real task ran, or that a real lease was
 * released. `keeper-sync`'s tests own that. This proves the shell reaches them
 * from the right places and from nowhere else.
 */
import { readdirSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const SHELL_DIR = resolve(import.meta.dirname, "../../src-tauri/crates/keeper/src");

const read = (relative: string): string => readFileSync(resolve(SHELL_DIR, relative), "utf8");

/**
 * A Rust file with its comment-only lines removed.
 *
 * Load-bearing rather than tidy: this file's own subject matter is discussed at
 * length in the shell's doc comments — `finalize_for_quit`'s rationale names
 * `stop_supervisor`, and the quit arm's comment names the window-close path it
 * deliberately is not on — so a scan over raw text would find every symbol it
 * is asserting the absence of.
 */
const codeOf = (source: string): string =>
  source
    .split("\n")
    .filter((line) => !line.trim().startsWith("//"))
    .join("\n");

const LIB = codeOf(read("lib.rs"));
const SYNC = codeOf(read("sync.rs"));
const SYNC_IPC = codeOf(read("sync_ipc.rs"));

/** Every `.rs` file in the shell crate, code only. */
const shellSources = (): [string, string][] =>
  readdirSync(SHELL_DIR, { recursive: true, withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith(".rs"))
    .map((entry) => {
      const relative = resolve(entry.parentPath, entry.name);
      return [relative, codeOf(readFileSync(relative, "utf8"))] as [string, string];
    });

const occurrences = (haystack: string, needle: string): number => haystack.split(needle).length - 1;

describe("the app hosts due tasks on the tick it already owns", () => {
  it("adds no second clock anywhere in the shell", () => {
    // AD-62, counted rather than eyeballed. A poll added to the 1 Hz tray tick
    // or a `tokio::time::interval` of its own would both be a second scheduler
    // over one git repository, and the due-gate needs neither: the app already
    // starts `Engine::run`, whose own interval is the tick `run_due_tasks` is
    // called from (`keeper-sync/src/engine.rs`).
    const intervals = shellSources().flatMap(([path, code]) =>
      occurrences(code, "tokio::time::interval") > 0 ? [path] : [],
    );
    expect(intervals).toHaveLength(1);
    expect(intervals[0]).toMatch(/lib\.rs$/);
    expect(occurrences(LIB, "tokio::time::interval")).toBe(1);
  });

  it("keeps that one interval the pre-existing tray tick, with nothing about tasks in it", () => {
    // The interval is identified by what it does, not by its position: the tray
    // renderers and `notes_vault::cadence_tick` are the whole of its body, and a
    // task poll spliced in beside them is what this assertion refuses.
    const at = LIB.indexOf("tokio::time::interval");
    const block = LIB.slice(at, LIB.indexOf("\n            }", at));
    expect(block).toContain("Duration::from_secs(1)");
    expect(block).toContain("tray::apply_recording_state");
    expect(block).toContain("notes_vault::cadence_tick()");
    expect(block).not.toMatch(/task/i);
  });

  it("becomes a task host by starting the supervisor at boot, under #[cfg(desktop)]", () => {
    // This is the whole of "the app runs due tasks": `Engine::run`'s loop calls
    // `tick`, and `tick` calls `run_due_tasks`. Desktop-gated the way the rest
    // of sync is — iOS has no task surface at all (AD-137).
    const at = LIB.indexOf("sync::start_supervisor(");
    expect(at).toBeGreaterThan(0);
    expect(occurrences(LIB, "sync::start_supervisor(")).toBe(1);
    expect(LIB.slice(Math.max(0, at - 200), at)).toContain("#[cfg(desktop)]");
    // ...and the supervisor really is `Engine::run`, rather than a loop of the
    // shell's own that happens to be called the same thing.
    const body = SYNC.slice(SYNC.indexOf("pub fn start_supervisor("));
    expect(body.slice(0, body.indexOf("\n}"))).toContain("engine.run(stop_rx)");
  });
});

describe("the quit path hands this host's task leases back", () => {
  it("routes quit through the one verb that releases them", () => {
    // `stop_supervisor` alone only *signals*, and the app drops the
    // supervisor's `JoinHandle` at spawn — so `Engine::run`'s post-loop
    // `finalize()`, which is what reaches `db::release_host_leases`, raced
    // process exit and usually lost. Asserting the absence of the bare signal
    // is what stops the release being quietly dropped again.
    expect(occurrences(LIB, "sync::finalize_for_quit()")).toBe(1);
    expect(occurrences(LIB, "sync::stop_supervisor()")).toBe(0);

    const exitAt = LIB.indexOf("RunEvent::ExitRequested");
    expect(exitAt).toBeGreaterThan(0);
    expect(LIB.indexOf("sync::finalize_for_quit()")).toBeGreaterThan(exitAt);
  });

  it("settles in-flight runs before it releases, and never releases unconditionally", () => {
    const body = SYNC.slice(SYNC.indexOf("pub fn finalize_for_quit()"));
    const fn = body.slice(0, body.indexOf("\n}"));
    expect(fn).toContain("stop_supervisor()");

    // The ordering IS the fix (this story's review, finding 1). The process does
    // not exit at this point — the bounded `shutdown_all` runs after it — so a
    // supervisor already inside a task run keeps going with a git child that has
    // no `kill_on_drop`. Releasing that task's lease let the daemon on a shared
    // `sync.db` start a second concurrent run over the same working tree, so the
    // quit path must go through the engine verb that settles first and holds back
    // what is still running, never through the unconditional release.
    expect(fn).toContain("finalize_task_leases_for_quit(");
    expect(fn).toContain("TASK_QUIT_SETTLE");
    expect(fn).not.toMatch(/release_task_leases\(\)/);

    // `engine_if_open`, never `engine(...)`: quitting must not OPEN a database
    // to release leases that by definition cannot exist yet.
    expect(fn).toContain("engine_if_open()");
    expect(fn).not.toMatch(/\bengine\(/);
  });

  it("leaves tasks running when the window is merely closed", () => {
    // ⌘W / the red button run `prevent_close()` + `hide()` and keep the
    // process, the engine and the supervisor: a hidden keeper is still a task
    // host, so releasing here would stop work the user never asked to stop.
    // The region is bounded by the run-event loop that follows it, so nothing
    // from the quit arm can leak into the assertion.
    const from = LIB.indexOf("builder.on_window_event(");
    const to = LIB.indexOf("RunEvent::ExitRequested");
    expect(from).toBeGreaterThan(0);
    expect(to).toBeGreaterThan(from);
    const windowEvents = LIB.slice(from, to);

    // Proof the right region was found, before anything is asserted absent
    // from it: an indexOf that missed would otherwise pass every check below.
    expect(windowEvents).toContain("WindowEvent::CloseRequested");
    expect(windowEvents).toContain("api.prevent_close()");
    expect(windowEvents).toContain("window.hide()");

    expect(windowEvents).not.toContain("finalize_for_quit");
    expect(windowEvents).not.toContain("stop_supervisor");
    expect(windowEvents).not.toContain("release_task_leases");
  });
});

describe("the task projection reports a fault as a fault", () => {
  // The middle link of finding 3's chain is `task_vm`, and it is in the one
  // crate this host cannot build. `keeper-sync` proves the store can NAME a
  // profile row it could not read (`unreadable_profile_ids`), and `keeper-core`
  // proves the pure function turns that into a different sentence
  // (`UNHOSTED_FOLDER_UNREADABLE`, not `UNHOSTED_FOLDER_GONE`). What no test
  // running here can otherwise see is whether the shell joins them — so it is
  // checked where a check runs, exactly as the quit path above is.

  const bodyOf = (source: string, signature: string): string => {
    const at = source.indexOf(signature);
    expect(at, `${signature} must exist`).toBeGreaterThan(-1);
    const body = source.slice(at);
    return body.slice(0, body.indexOf("\n}"));
  };

  it("asks which profile rows were unreadable, and hands the answer to the verdict", () => {
    expect(SYNC_IPC).toContain("unreadable_profile_ids()");
    expect(SYNC_IPC).toContain("profile_unreadable:");

    // Both commands that compute a host verdict, not just the listing one: a
    // save is exactly when the verdict can change, and it had the same swallow.
    for (const signature of ["pub async fn sync_tasks(", "pub async fn sync_task_save("]) {
      const fn = bodyOf(SYNC_IPC, signature);
      expect(fn, signature).toContain("unreadable_profile_ids()");
      expect(fn, signature).toContain("&unreadable");
    }
  });

  it("propagates a failed read instead of inventing a fact from it", () => {
    // `unwrap_or_default()` on a profile read made EVERY folder-scoped task
    // render "it names a folder keeper does not sync"; `.ok()` on a history read
    // made a task that had run render "never run". Both are facts invented from
    // a fault, and both were one line below a sibling call that propagates.
    for (const signature of ["pub async fn sync_tasks(", "pub async fn sync_task_save("]) {
      const fn = bodyOf(SYNC_IPC, signature);
      expect(fn, signature).not.toContain("unwrap_or_default()");
      // The exact swallow that was there: a `Result` turned into an `Option`
      // and then chained, so the fault has nowhere left to go.
      expect(fn, signature).not.toMatch(/\.ok\(\)\s*\n\s*\.and_then/);
      expect(fn, signature).toContain("list_profiles()");
      expect(fn, signature).toContain("task_history(");
    }
  });

  it("keeps the daemon probe off the async runtime, with a deadline", () => {
    // `systemctl` + two `canonicalize` calls, in an `async fn` body, on a
    // runtime worker — the rule this file's own `sync_footprint` states. And
    // `.output()` waits without a deadline, so a wedged user bus parked the
    // command forever rather than reading as "no daemon".
    expect(SYNC_IPC).toContain("spawn_blocking");
    const probe = bodyOf(SYNC_IPC, "async fn daemon_presence_probe(");
    expect(probe).toContain("spawn_blocking");
    expect(probe).toContain("DaemonPresence::Absent");

    const enabled = bodyOf(SYNC_IPC, "fn daemon_unit_enabled()");
    expect(enabled).toContain("DAEMON_PROBE_TIMEOUT");
    expect(enabled).not.toContain(".output()");

    // And the two commands reach the probe only through the bounded wrapper.
    for (const signature of ["pub async fn sync_tasks(", "pub async fn sync_task_save("]) {
      const fn = bodyOf(SYNC_IPC, signature);
      expect(fn, signature).toContain("daemon_presence_probe(");
      expect(fn, signature).not.toContain("daemon_presence_here(");
    }
  });
});
