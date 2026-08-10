import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { TagVocabularyEntryVm, TagVocabularyVm } from "@/lib/ipc/client";

// Mock the typed IPC client so the card never touches Tauri.
vi.mock("@/lib/ipc/client", () => ({
  tagsVocabulary: vi.fn(),
}));

import { RecordingMetaCard } from "@/components/recording/recording-meta-card";
import { META_TAGS_LABEL } from "@/components/recording/recording-meta-fields";
import { tagVocabularyListId } from "@/components/tags/tag-vocabulary-input";
import { tagsVocabulary } from "@/lib/ipc/client";
import type { RecordingMetaWire } from "@/lib/stores/recording-meta";
import { consumeRecordingMeta, recordingMetaStore } from "@/lib/stores/recording-meta";

const mockVocabulary = vi.mocked(tagsVocabulary);

function entry(
  p: Partial<TagVocabularyEntryVm> & Pick<TagVocabularyEntryVm, "path">,
): TagVocabularyEntryVm {
  return { path: p.path, count: p.count ?? 1 };
}

function vocabulary(...paths: string[]): TagVocabularyVm {
  return { entries: paths.map((path) => entry({ path })) };
}

/** The suggestion values offered under the Tags field, in DOM order. */
function tagSuggestions(container: HTMLElement): string[] {
  const list = container.querySelector(`#${tagVocabularyListId("recording-meta-tags")}`);
  return Array.from(list?.querySelectorAll("option") ?? []).map((option) => option.value);
}

function typeTags(value: string): void {
  fireEvent.change(screen.getByLabelText(META_TAGS_LABEL), { target: { value } });
}

/** Take the fields the way Start does. Wrapped in `act` because consuming
 *  clears the form, which re-renders the mounted card. */
function consume(): RecordingMetaWire {
  let taken: RecordingMetaWire = {};
  act(() => {
    taken = consumeRecordingMeta();
  });
  return taken;
}

/** The store is module-level and shared with sibling suites: drain it rather
 *  than leaving a title or a tag behind for the next file. */
function drainStore(): void {
  act(() => {
    recordingMetaStore.setState({
      fields: { title: "", participants: "", note: "", tags: "", custom: [] },
      last: null,
    });
  });
}

beforeEach(() => {
  mockVocabulary.mockReset();
  mockVocabulary.mockResolvedValue(vocabulary());
  drainStore();
});

afterEach(() => {
  vi.clearAllMocks();
  drainStore();
});

describe("RecordingMetaCard tags", () => {
  it("offers a tag that exists only on notes while tagging a recording (Story 42.5 AC2)", async () => {
    // `client/acme` has never been on a recording — it is a notes tag. One
    // vocabulary means the card offers it anyway.
    mockVocabulary.mockResolvedValue(vocabulary("client", "client/acme"));
    const { container } = render(<RecordingMetaCard />);

    await waitFor(() => expect(tagSuggestions(container)).toEqual(["client", "client/acme"]));
  });

  it("keeps offering the shared vocabulary for the second tag in the field", async () => {
    mockVocabulary.mockResolvedValue(vocabulary("client/acme"));
    const { container } = render(<RecordingMetaCard />);
    await waitFor(() => expect(tagSuggestions(container)).toEqual(["client/acme"]));

    typeTags("standup, cl");

    await waitFor(() => expect(tagSuggestions(container)).toEqual(["standup, client/acme"]));
  });

  it("resolves the vocabulary against the active vault, since the recording surface has no vault of its own", async () => {
    render(<RecordingMetaCard />);

    await waitFor(() => expect(mockVocabulary).toHaveBeenCalledWith(undefined));
  });

  it("sends the tag text exactly as typed — no split, no per-tag trim, no lower-casing", () => {
    render(<RecordingMetaCard />);

    // `Client/Acme ` and a duplicate-after-normalising `acme`: every one of
    // those decisions belongs to the tag module in Rust, so the card must not
    // pre-empt any of them.
    typeTags("Client/Acme , acme");

    expect(consume().tags).toBe("Client/Acme , acme");
  });

  it("ships no tags at all from an untouched form", () => {
    render(<RecordingMetaCard />);

    expect(consume().tags).toBeUndefined();
  });

  it("ships no tags when the field holds only whitespace", () => {
    render(<RecordingMetaCard />);

    typeTags("   ");

    expect(consume().tags).toBeUndefined();
  });

  it("clears the field once a session consumes it", () => {
    render(<RecordingMetaCard />);
    typeTags("standup");

    consume();

    expect(screen.getByLabelText(META_TAGS_LABEL)).toHaveValue("");
  });
});
