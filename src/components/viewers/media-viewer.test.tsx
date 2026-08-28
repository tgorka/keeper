/**
 * The registry's `video`, `image` and `audio` viewer, mounted the way a panel
 * mounts it (Story 45.7, FR-180, AD-87, AD-91).
 *
 * **Every test goes through `viewerComponentFor`.** Importing `MediaViewer`
 * directly would prove the component works and prove nothing about the binding
 * — and "declared and never mounted" is DW-172, which shipped green in epic 44
 * because nothing in the suite could see that the declaration was never
 * reached. A viewer bound in a table nobody exercises is that defect wearing a
 * different hat.
 *
 * **What jsdom can and cannot say here, stated up front.** jsdom decodes
 * nothing: it never fires `loadedmetadata`, never sets `readyState` above 0 and
 * never gives an `<img>` a `naturalWidth`. So every assertion below about a
 * frame or an intrinsic size drives the element the way the platform would —
 * define the property, dispatch the event — and asserts what THIS code does
 * with it. That is the same split Story 44.1 worked in: the prime's policy is
 * jsdom's to prove, and the 988 lit pixels were WebKit's. Which of these is
 * proved on which engine is spelled out in the spec's last section.
 */
import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

// **There is deliberately no `vi.mock("@/lib/ipc/client")` here, and its
// absence is an assertion.**
//
// This file originally carried one, added defensively when it was written.
// Story 45.13's rule — you need a mock exactly when the boot path reaches the
// name, so adding one "to be safe" is a lie about what the boot path does —
// says to check rather than assume, and checking removed it: all tests pass
// without it.
//
// **What the absence enforces, stated no wider than it is.** `MediaViewer`
// composes its URL synchronously from two values it was handed and reaches no
// IPC command on any path this file drives. That is the whole of it.
//
// It is NOT proof that this viewer works in the quick-capture webview, though
// it is the load-bearing half of that argument. The rest is two things read
// rather than run: that the four URI schemes are registered on the Tauri
// Builder app-wide with no window scoping (`keeper/src/lib.rs`), and that
// `capabilities/*.json` gates plugin permissions rather than `#[tauri::command]`
// functions (`sync_ipc.rs`'s own comment). Nothing has ever been executed in a
// second webview. Story 45.13's rule, taken on this file: a comment claiming a
// guarantee wider than its test is worse than no comment, because the next
// reader budgets for the wider one.
//
// If you add a test that clicks Reveal, you WILL reach the real `revealPath`
// and it will fail loudly in jsdom. That is the correct outcome: mock it in
// that test, not for the whole file, or the property above stops being tested.

import { FRAME_PRIME_SECONDS } from "@/components/notes/editor/recording-transport";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import {
  resolveViewer,
  UNKNOWN_VIEWER_OPEN_LABEL,
  UNKNOWN_VIEWER_SIZE_SLOT,
  UNKNOWN_VIEWER_TESTID,
  type ViewerFile,
  viewerComponentFor,
} from "@/lib/viewers";
import {
  MEDIA_NO_PROFILE_SENTENCE,
  MEDIA_VIEWER_ELEMENT_TESTID,
  MEDIA_VIEWER_FACTS_TESTID,
  MEDIA_VIEWER_TESTID,
  MediaViewer,
} from "./media-viewer";

function target(overrides: Partial<ViewerFile> = {}): ViewerFile {
  return {
    name: "screen-0000.mov",
    kind: "video",
    relativePath: "2026/08/screen-0000.mov",
    profileId: "01PROFILE",
    absolutePath: "/Volumes/merope/2026/08/screen-0000.mov",
    sizeLabel: "4.3 MB",
    openWith: null,
    writeCaveat: null,
    writeCaveatShort: null,
    writeRefusal: null,
    ...overrides,
  };
}

/** Mount exactly as a panel host does: ask the registry, render what it says. */
function openThroughTheRegistry(file: ViewerFile) {
  const { entry, Component } = viewerComponentFor(file);
  return { entry, ...render(<Component file={file} entry={entry} />) };
}

function element(): HTMLElement {
  return screen.getByTestId(MEDIA_VIEWER_ELEMENT_TESTID);
}

/**
 * Make an element report what a real engine would report.
 *
 * jsdom's `HTMLMediaElement` has `readyState`, `videoWidth` and `error` as
 * getters with no setter, and `HTMLImageElement.naturalWidth` likewise, so a
 * test that assigned to them would silently assign nothing and pass while
 * asserting the default.
 */
function reports(node: object, values: Record<string, unknown>): void {
  for (const [name, value] of Object.entries(values)) {
    Object.defineProperty(node, name, { configurable: true, value });
  }
}

beforeEach(() => {
  capabilitiesStore.setState({ capabilities: { ...DEFAULT_CAPABILITIES } });
});

describe("the registry's three media ids really mount this viewer", () => {
  it("resolves a video, an image and an audio file to a mounted element each", () => {
    for (const [file, tag, viewer] of [
      [target(), "VIDEO", "video"],
      [target({ name: "whiteboard.png", kind: "image" }), "IMG", "image"],
      [target({ name: "mic-0000.wav", kind: "audio" }), "AUDIO", "audio"],
    ] as const) {
      const { entry, unmount } = openThroughTheRegistry(file);
      // The row the table chose, so a failure says which half broke.
      expect(entry.viewer).toBe(viewer);
      expect(element().tagName).toBe(tag);
      expect(screen.getByTestId(MEDIA_VIEWER_TESTID)).toHaveAttribute("data-viewer", viewer);
      unmount();
    }
  });

  it("draws the same element for a file keeper refuses to write", () => {
    // `writeRefusal` is the LOCATION's verdict and rides on every `ViewerFile`
    // now. A recording inside a session's `workspace/` is exactly that case,
    // and this viewer offers no write at all — so the refusal must change
    // nothing here, and must not appear on screen as a banner about a control
    // this surface does not have.
    const { container: plain, unmount } = render(
      <MediaViewer file={target()} entry={resolveViewer(target())} />,
    );
    const html = plain.innerHTML;
    unmount();

    const fenced = target({
      writeRefusal:
        "2026/08/screen-0000.mov is inside a session's workspace — keeper reads it but " +
        "never writes there.",
    });
    const { container: refused } = render(
      <MediaViewer file={fenced} entry={resolveViewer(fenced)} />,
    );

    expect(refused.textContent).not.toContain("never writes there");
    expect(refused.innerHTML).toBe(html);
  });

  it("points every element at the profile's own scheme, never at an absolute path", () => {
    // FR-145 and AD-65 in one assertion: the URL is composed from the two
    // halves Rust supplied, and `absolutePath` — which this file carries —
    // appears nowhere.
    //
    // Story 45.20's shape: an absence over a literal is hollow unless
    // something asserts the literal was ever in the input. Without the witness
    // below, a fixture that stopped carrying an absolute path — or carried a
    // different one — would satisfy the `not.toContain` for the wrong reason
    // and this test would go on passing while FR-145 stopped being tested.
    const file = target();
    expect(file.absolutePath).toContain("/Volumes/merope");

    openThroughTheRegistry(file);

    const source = element().getAttribute("src");
    expect(source).toBe("keeper-file://01PROFILE/2026/08/screen-0000.mov");
    expect(document.body.innerHTML).not.toContain("/Volumes/merope");
  });

  it("escapes a name that would otherwise end the path", () => {
    openThroughTheRegistry(
      target({ name: "take #2.png", kind: "image", relativePath: "a b/take #2.png" }),
    );
    expect(element().getAttribute("src")).toBe("keeper-file://01PROFILE/a%20b/take%20%232.png");
  });
});

describe("a video asks for the frame `preload=metadata` does not fetch", () => {
  /**
   * Story 44.1's defect, in this surface. `preload="metadata"` settles at
   * `readyState` 1, which the HTML spec defines as having obtained no video
   * data and representing transparent black — measured in a real WKWebView as
   * zero lit pixels, for a lone video with native controls exactly as for a
   * pair. This asserts the state change that buys the frame, which is what
   * jsdom can see; the pixels were WebKit's to count.
   */
  it("moves an untouched element off zero once the metadata lands", () => {
    openThroughTheRegistry(target());
    const player = element() as HTMLVideoElement;
    expect(player.preload).toBe("metadata");
    reports(player, { readyState: 1 });

    expect(player.currentTime).toBe(0);
    fireEvent(player, new Event("loadedmetadata"));
    expect(player.currentTime).toBe(FRAME_PRIME_SECONDS);
  });

  it("buys nothing for an element that already has a frame", () => {
    openThroughTheRegistry(target());
    const player = element() as HTMLVideoElement;
    // HAVE_CURRENT_DATA: there is a frame, and the range request would be spent
    // for nothing.
    reports(player, { readyState: 2 });

    fireEvent(player, new Event("loadedmetadata"));
    expect(player.currentTime).toBe(0);
  });

  it("leaves an element the reader already moved exactly where it is", () => {
    openThroughTheRegistry(target());
    const player = element() as HTMLVideoElement;
    reports(player, { readyState: 1 });
    player.currentTime = 37.5;

    fireEvent(player, new Event("loadedmetadata"));
    expect(player.currentTime).toBe(37.5);
  });

  it("buys one frame and not one per event, even for a reader back at the top", () => {
    openThroughTheRegistry(target());
    const player = element() as HTMLVideoElement;
    reports(player, { readyState: 1 });

    fireEvent(player, new Event("loadedmetadata"));
    player.currentTime = 0;
    // A source change or a reload raises the event again. A reader who scrubbed
    // back to exactly zero is at the top and would be moved a millisecond they
    // did not ask for; `{ once: true }` is what stops it.
    fireEvent(player, new Event("loadedmetadata"));
    expect(player.currentTime).toBe(0);
  });

  it("asks an audio element for nothing, because there is no frame to buy", () => {
    openThroughTheRegistry(target({ name: "mic-0000.wav", kind: "audio" }));
    const player = element() as HTMLAudioElement;
    reports(player, { readyState: 1 });

    fireEvent(player, new Event("loadedmetadata"));
    expect(player.currentTime).toBe(0);
  });
});

describe("what the reader is told about the file", () => {
  it("exposes an image's intrinsic size once the platform has decoded it", () => {
    openThroughTheRegistry(target({ name: "whiteboard.png", kind: "image" }));
    const image = element() as HTMLImageElement;
    // Before the load there is nothing honest to say; a zero would read as a
    // fact about the file rather than about the decode.
    expect(screen.getByTestId(MEDIA_VIEWER_FACTS_TESTID)).not.toHaveTextContent("\u00D7");

    reports(image, { naturalWidth: 2560, naturalHeight: 1440 });
    fireEvent.load(image);

    expect(screen.getByTestId(MEDIA_VIEWER_FACTS_TESTID)).toHaveTextContent("2560 \u00D7 1440");
  });

  it("exposes a video's intrinsic size from the same metadata that primes it", () => {
    openThroughTheRegistry(target());
    const player = element() as HTMLVideoElement;
    reports(player, { readyState: 1, videoWidth: 1440, videoHeight: 900 });

    fireEvent(player, new Event("loadedmetadata"));

    expect(screen.getByTestId(MEDIA_VIEWER_FACTS_TESTID)).toHaveTextContent("1440 \u00D7 900");
  });

  it("says nothing about dimensions for audio, which has none", () => {
    openThroughTheRegistry(target({ name: "mic-0000.wav", kind: "audio" }));
    fireEvent(element(), new Event("loadedmetadata"));
    expect(screen.getByTestId(MEDIA_VIEWER_FACTS_TESTID)).not.toHaveTextContent("\u00D7");
  });

  it("states the name and the size Rust already formatted", () => {
    openThroughTheRegistry(target());
    const facts = screen.getByTestId(MEDIA_VIEWER_FACTS_TESTID);
    expect(facts).toHaveTextContent("screen-0000.mov");
    expect(facts).toHaveTextContent("4.3 MB");
  });

  it("gives every element an accessible name and real controls", () => {
    for (const file of [target(), target({ name: "mic-0000.wav", kind: "audio" })]) {
      const { unmount } = openThroughTheRegistry(file);
      const player = element() as HTMLMediaElement;
      expect(player.controls).toBe(true);
      expect(player.getAttribute("aria-label")).toBe(file.name);
      unmount();
    }
    // An image's accessible name is its `alt`, and it must not be empty: an
    // empty `alt` tells a screen reader the only content of the panel is
    // decorative.
    openThroughTheRegistry(target({ name: "whiteboard.png", kind: "image" }));
    expect(element()).toHaveAttribute("alt", "whiteboard.png");
  });
});

describe("a file keeper cannot decode says so, with its name and its size", () => {
  it("falls back to the named placeholder when the decoder refuses", () => {
    openThroughTheRegistry(target({ name: "clip.mkv" }));
    const player = element() as HTMLVideoElement;
    // MEDIA_ERR_DECODE.
    reports(player, { error: { code: 3 } });

    fireEvent(player, new Event("error"));

    const placeholder = screen.getByTestId(UNKNOWN_VIEWER_TESTID);
    // The reason names the file and what the platform said, not "an error
    // occurred" and not a black rectangle.
    expect(placeholder).toHaveTextContent("keeper could not decode clip.mkv");
    // And the facts AD-91 promises are all still there.
    expect(placeholder).toHaveTextContent("clip.mkv");
    expect(screen.getByTestId(UNKNOWN_VIEWER_SIZE_SLOT)).toHaveTextContent("4.3 MB");
    expect(screen.queryByTestId(MEDIA_VIEWER_ELEMENT_TESTID)).toBeNull();
  });

  it("keeps the failure when a measurement lands in the same tick after it", () => {
    // **Story 45.14's shape: two producers that run one after the other cannot
    // share one state slot.** Not a race — a sequence, where the later one
    // succeeds and erases the earlier one's failure sentence before a frame is
    // painted, which is indistinguishable from never having said anything.
    //
    // `failure` and `intrinsic` are two producers over one `reported` object.
    // They are safe because each writes its own field through a functional
    // update that carries the other — but nothing pinned that, and a plain
    // `setReported({ ... })` in either handler reads like perfectly ordinary
    // React. Probed and it SURVIVED the whole suite; this is the test that
    // fails it.
    //
    // Reachable rather than theoretical: both listeners live on the same
    // element, so a volume unplugged mid-load raises `error` and can raise
    // `loadedmetadata` in the same task. Whichever handler ran last would win,
    // and the reader would get a player that cannot play instead of the
    // sentence naming why.
    openThroughTheRegistry(target({ name: "clip.mkv" }));
    const player = element() as HTMLVideoElement;
    reports(player, { error: { code: 2 }, videoWidth: 1440, videoHeight: 900 });

    // One batch: both handlers run before React re-renders.
    act(() => {
      player.dispatchEvent(new Event("error"));
      player.dispatchEvent(new Event("loadedmetadata"));
    });

    expect(screen.getByTestId(UNKNOWN_VIEWER_TESTID)).toHaveTextContent(
      "keeper could not read clip.mkv",
    );
    expect(screen.queryByTestId(MEDIA_VIEWER_ELEMENT_TESTID)).toBeNull();
  });

  it("names each of the platform's four reasons, and never as the unknown one", () => {
    const seen = new Set<string>();
    for (const code of [1, 2, 3, 4]) {
      const { unmount } = openThroughTheRegistry(target({ name: "clip.mkv" }));
      reports(element(), { error: { code } });
      fireEvent(element(), new Event("error"));
      const placeholder = screen.getByTestId(UNKNOWN_VIEWER_TESTID);
      // A code the platform DID give must never be reported as "the platform
      // did not say why". Distinctness alone does not catch that: swapping one
      // real sentence for the unknown one leaves four distinct strings and
      // tells the reader keeper has no idea, when it was told exactly.
      expect(placeholder).not.toHaveTextContent("did not say why");
      seen.add(placeholder.textContent ?? "");
      unmount();
    }
    // Four codes, four sentences. One "could not play" for all of them would
    // make "the volume went away" and "this machine has no decoder" look like
    // the same problem, and only one of them is worth unplugging a drive over.
    expect(seen.size).toBe(4);
  });

  it("says keeper does not know why, rather than guessing, when there is no code", () => {
    // An `<img>` carries no `MediaError` at all.
    openThroughTheRegistry(target({ name: "photo.heic", kind: "image" }));
    fireEvent.error(element());
    expect(screen.getByTestId(UNKNOWN_VIEWER_TESTID)).toHaveTextContent(
      "keeper could not open photo.heic, and the platform did not say why",
    );
  });

  it("shows the placeholder rather than a player for a file inside no profile", () => {
    // 45.2 carries `profileId: null` as a fact. There is no URL that could
    // reach the file: every scheme keeper serves is contained to a root, and
    // pointing an element at the absolute path would go around the containment
    // check AD-65 exists to keep.
    openThroughTheRegistry(target({ profileId: null }));
    expect(screen.queryByTestId(MEDIA_VIEWER_ELEMENT_TESTID)).toBeNull();
    expect(screen.getByTestId(UNKNOWN_VIEWER_TESTID)).toHaveTextContent(MEDIA_NO_PROFILE_SENTENCE);
  });

  /**
   * **The payload, not the prose.** The two tests above assert what the
   * placeholder SAYS; nothing asserted what it was HANDED. Story 45.13's
   * finding — a green suite that exercises a payload's shape while never
   * checking its value reads exactly like one that does both — probed here and
   * confirmed: passing `{ ...file, openWith: null }` to either fallback
   * survived every test in this file.
   *
   * It matters most in exactly these two states. Both sentences end "hand it to
   * the application that owns it", and for a file this machine cannot decode,
   * or one outside every profile, that button IS the remedy. A placeholder that
   * says so and does not offer it is worse than one that says nothing.
   *
   * Two tests rather than one, so a regression names which fallback broke.
   */
  it("hands the undecodable file's own actions to the placeholder, not a stripped copy", () => {
    const openWith = vi.fn(async () => undefined);
    openThroughTheRegistry(target({ name: "clip.mkv", openWith }));
    reports(element(), { error: { code: 3 } });
    fireEvent(element(), new Event("error"));

    fireEvent.click(screen.getByRole("button", { name: UNKNOWN_VIEWER_OPEN_LABEL }));
    expect(openWith).toHaveBeenCalledTimes(1);
  });

  it("hands the out-of-profile file's own actions to the placeholder", () => {
    const openWith = vi.fn(async () => undefined);
    openThroughTheRegistry(target({ profileId: null, openWith }));

    fireEvent.click(screen.getByRole("button", { name: UNKNOWN_VIEWER_OPEN_LABEL }));
    expect(openWith).toHaveBeenCalledTimes(1);
  });

  it("says so when a non-media row is routed here, rather than drawing a silent audio bar", () => {
    // **Built by hand on purpose**, following `text-file-viewer.test.tsx`'s
    // precedent for the same situation: `VIEWER_COMPONENTS` binds only the
    // three media ids to this component, so the registry cannot produce this
    // input — and a guard that can only run on an input the registry cannot
    // produce is exactly the guard nothing else will ever exercise.
    //
    // Before this guard, `audio` was the else-of-an-else: any id that was not
    // `image` and not `video` rendered an `<audio>`. Binding a fourth id to
    // this component would have drawn a permanently empty audio bar and said
    // nothing, which is the silent-nothing this whole epic exists against.
    const file = target({ name: "notes.md", kind: "file", relativePath: "notes.md" });
    const wrongRow = resolveViewer({ name: file.name, kind: file.kind });
    expect(wrongRow.viewer).toBe("text");

    render(<MediaViewer file={file} entry={wrongRow} />);

    expect(screen.queryByTestId(MEDIA_VIEWER_ELEMENT_TESTID)).toBeNull();
    expect(screen.getByTestId(UNKNOWN_VIEWER_TESTID)).toHaveTextContent(
      "keeper routed notes.md to its media viewer",
    );
    // And it blames the wiring rather than the file, because the file is fine.
    expect(screen.getByTestId(UNKNOWN_VIEWER_TESTID)).toHaveTextContent(
      "a wiring mistake in keeper rather than a problem with the file",
    );
  });

  it("forgets the previous file's failure when the panel is retargeted", () => {
    const { rerender } = openThroughTheRegistry(target({ name: "clip.mkv" }));
    reports(element(), { error: { code: 3 } });
    fireEvent(element(), new Event("error"));
    expect(screen.getByTestId(UNKNOWN_VIEWER_TESTID)).toBeInTheDocument();

    const next = target({ name: "good.mov", relativePath: "2026/08/good.mov" });
    const resolved = viewerComponentFor(next);
    rerender(<resolved.Component file={next} entry={resolved.entry} />);

    // A sentence about the last file, over this one's element, is a lie the
    // reader has no way to see through.
    expect(screen.queryByTestId(UNKNOWN_VIEWER_TESTID)).toBeNull();
    expect(element().getAttribute("src")).toBe("keeper-file://01PROFILE/2026/08/good.mov");
  });
});

describe("closing a panel releases the media element", () => {
  /**
   * Removing the node is not enough. A `<video>` with a `src` holds an open
   * range-request pipeline and a decoder until it is told to let go, and a
   * panel strip a reader opens and closes all afternoon would otherwise
   * accumulate them against files on a volume they then cannot eject.
   *
   * `load()` is the call that actually aborts the selected resource — jsdom
   * does not implement it, which is why it is spied rather than observed
   * through a side effect.
   */
  it("pauses, drops the source and aborts the load on unmount", () => {
    const paused = vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
    const loaded = vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});

    const { unmount } = openThroughTheRegistry(target());
    const player = element() as HTMLVideoElement;
    expect(player.getAttribute("src")).not.toBeNull();

    unmount();

    expect(paused).toHaveBeenCalled();
    expect(player.getAttribute("src")).toBeNull();
    expect(loaded).toHaveBeenCalled();
    paused.mockRestore();
    loaded.mockRestore();
  });

  it("releases an audio element too", () => {
    const paused = vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
    const loaded = vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});

    const { unmount } = openThroughTheRegistry(target({ name: "mic.wav", kind: "audio" }));
    const player = element() as HTMLAudioElement;
    unmount();

    expect(paused).toHaveBeenCalled();
    expect(player.getAttribute("src")).toBeNull();
    expect(loaded).toHaveBeenCalled();
    paused.mockRestore();
    loaded.mockRestore();
  });

  it("releases the old source before loading the new one when the target changes", () => {
    const loaded = vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
    vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});

    const { rerender } = openThroughTheRegistry(target());
    const before = loaded.mock.calls.length;

    const next = target({ name: "camera-0000.mov", relativePath: "2026/08/camera-0000.mov" });
    const resolved = viewerComponentFor(next);
    rerender(<resolved.Component file={next} entry={resolved.entry} />);

    // The old resource is aborted rather than left streaming behind the new
    // one — two open pipelines against one panel is the leak this prevents.
    expect(loaded.mock.calls.length).toBeGreaterThan(before);
    expect(element().getAttribute("src")).toBe("keeper-file://01PROFILE/2026/08/camera-0000.mov");
    vi.restoreAllMocks();
  });

  it("stops listening, so a late event cannot resurrect a torn-down panel", () => {
    vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
    vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});

    const { unmount } = openThroughTheRegistry(target());
    const player = element() as HTMLVideoElement;
    unmount();

    // A range request in flight when the panel closed can still fail. Setting
    // state on an unmounted tree is the warning nobody reads until it is a
    // crash.
    reports(player, { error: { code: 2 } });
    expect(() => fireEvent(player, new Event("error"))).not.toThrow();
    vi.restoreAllMocks();
  });
});
