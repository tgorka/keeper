import { fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { TagVocabularyEntryVm, TagVocabularyVm } from "@/lib/ipc/client";

// Mock the typed IPC client so the field never touches Tauri.
vi.mock("@/lib/ipc/client", () => ({
  tagsVocabulary: vi.fn(),
}));

import {
  TagVocabularyInput,
  tagSuggestions,
  tagVocabularyListId,
} from "@/components/tags/tag-vocabulary-input";
import { tagsVocabulary } from "@/lib/ipc/client";

const mockVocabulary = vi.mocked(tagsVocabulary);

function entry(
  p: Partial<TagVocabularyEntryVm> & Pick<TagVocabularyEntryVm, "path">,
): TagVocabularyEntryVm {
  return { path: p.path, count: p.count ?? 1 };
}

function vocabulary(...paths: string[]): TagVocabularyVm {
  return { entries: paths.map((path) => entry({ path })) };
}

/** The suggestion values the browser would filter, in DOM order. */
function suggestions(container: HTMLElement, inputId: string): string[] {
  const list = container.querySelector(`#${tagVocabularyListId(inputId)}`);
  return Array.from(list?.querySelectorAll("option") ?? []).map((option) => option.value);
}

beforeEach(() => {
  mockVocabulary.mockReset();
  mockVocabulary.mockResolvedValue(vocabulary());
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("tagSuggestions", () => {
  it("offers each tag on its own when the field holds a single tag", () => {
    expect(tagSuggestions("cl", ["client", "client/acme"])).toEqual(["client", "client/acme"]);
  });

  it("keeps the already-typed tags ahead of the candidate, because picking a datalist suggestion replaces the whole field", () => {
    expect(tagSuggestions("standup, cl", ["client/acme"])).toEqual(["standup, client/acme"]);
  });

  it("carries through whatever spacing was typed after the comma rather than regularising it", () => {
    expect(tagSuggestions("standup,cl", ["client/acme"])).toEqual(["standup,client/acme"]);
    expect(tagSuggestions("standup,   cl", ["client/acme"])).toEqual(["standup,   client/acme"]);
  });

  it("never re-cases or re-shapes a vocabulary tag to match what was typed — normalisation lives in Rust", () => {
    expect(tagSuggestions("Client/Acme", ["client/acme"])).toEqual(["client/acme"]);
  });

  it("offers nothing when the vocabulary is empty", () => {
    expect(tagSuggestions("anything", [])).toEqual([]);
  });
});

describe("TagVocabularyInput", () => {
  it("points the input at the datalist it renders, so the browser supplies the completion", async () => {
    mockVocabulary.mockResolvedValue(vocabulary("client/acme"));
    const { container } = render(
      <TagVocabularyInput id="tags-field" value="" onChange={() => {}} />,
    );

    const input = container.querySelector("#tags-field");
    expect(input?.getAttribute("list")).toBe("tags-field-vocabulary");
    await waitFor(() => expect(suggestions(container, "tags-field")).toEqual(["client/acme"]));
  });

  it("offers every tag the shared vocabulary reports, whichever producer put it there", async () => {
    mockVocabulary.mockResolvedValue(vocabulary("client", "client/acme", "standup"));
    const { container } = render(
      <TagVocabularyInput id="tags-field" value="" onChange={() => {}} />,
    );

    await waitFor(() =>
      expect(suggestions(container, "tags-field")).toEqual(["client", "client/acme", "standup"]),
    );
  });

  it("asks for the active vault when no vault is named, which is the case on the recording surface", async () => {
    render(<TagVocabularyInput id="tags-field" value="" onChange={() => {}} />);

    await waitFor(() => expect(mockVocabulary).toHaveBeenCalledWith(undefined));
  });

  it("scopes the vocabulary to a named vault when one is given", async () => {
    render(<TagVocabularyInput id="tags-field" value="" onChange={() => {}} vaultId="vault-1" />);

    await waitFor(() => expect(mockVocabulary).toHaveBeenCalledWith("vault-1"));
  });

  it("hands the typed text back unmodified", () => {
    const onChange = vi.fn();
    const { container } = render(
      <TagVocabularyInput id="tags-field" value="" onChange={onChange} />,
    );

    const input = container.querySelector("#tags-field") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "Client/Acme , acme" } });

    expect(onChange).toHaveBeenCalledWith("Client/Acme , acme");
  });

  it("leaves an ordinary text field behind when the vocabulary will not load", async () => {
    mockVocabulary.mockRejectedValue(new Error("no vault"));
    const { container } = render(
      <TagVocabularyInput id="tags-field" value="cl" onChange={() => {}} />,
    );

    await waitFor(() => expect(mockVocabulary).toHaveBeenCalled());
    expect(suggestions(container, "tags-field")).toEqual([]);
    expect(container.querySelector("#tags-field")).not.toBeNull();
  });
});
