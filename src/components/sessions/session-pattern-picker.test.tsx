/**
 * FR-253: the preview is the promise, so these tests are about what the user
 * is told BEFORE pressing Create — most of all about the files that do not
 * travel, which is the half a picker normally leaves silent.
 */
import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import {
  SESSION_PATTERN_COPIES_LABEL,
  SESSION_PATTERN_EMPTY_LABEL,
  SESSION_PATTERN_LABEL,
  SESSION_PATTERN_LOADING_LABEL,
  SESSION_PATTERN_SKIPS_LABEL,
  SessionPatternPicker,
} from "@/components/sessions/session-pattern-picker";
import type { SessionPatternVm } from "@/lib/ipc/client";

const NOW = Date.UTC(2026, 7, 12, 12, 0, 0);

function pattern(over: Partial<SessionPatternVm> = {}): SessionPatternVm {
  return {
    id: "_template",
    kind: "template",
    label: "Zone template",
    detail: "the zone's own skeleton — copied whole",
    mtimeMs: null,
    copies: [],
    skips: [],
    ...over,
  };
}

describe("SessionPatternPicker", () => {
  it("says it is reading rather than showing an empty picker", () => {
    render(<SessionPatternPicker patterns={null} value={null} onChange={() => {}} nowMs={NOW} />);
    expect(screen.getByText(SESSION_PATTERN_LOADING_LABEL)).toBeInTheDocument();
  });

  it("renders nothing at all in a zone with no template and no sessions", () => {
    const { container } = render(
      <SessionPatternPicker patterns={[]} value={null} onChange={() => {}} nowMs={NOW} />,
    );
    // A select with one unchosen option is furniture; create still works.
    expect(container).toBeEmptyDOMElement();
  });

  it("names what a session pattern copies AND what it leaves behind, with the reason", () => {
    const session = pattern({
      id: "01J5AAAAAAAAAAAAAAAAAAAAAA",
      kind: "session",
      label: "keeper — rolling work session",
      detail: "continues this session",
      mtimeMs: NOW - 60 * 60_000,
      copies: [
        { relPath: "prompts", isDir: true },
        { relPath: "prompts/01-scope.md", isDir: false },
        { relPath: "refs/design.md", isDir: false },
      ],
      skips: [
        {
          relPath: "artifacts/report.md",
          reason: "artifacts stay with the session that produced them",
        },
        {
          relPath: "README.md",
          reason: "the README is rebuilt from its headings — prose never travels",
        },
      ],
    });
    render(
      <SessionPatternPicker
        patterns={[session]}
        value={session.id}
        onChange={() => {}}
        nowMs={NOW}
      />,
    );

    // Files travel; the directories that hold them are plumbing, not a promise.
    const copies = screen.getByRole("list", { name: SESSION_PATTERN_COPIES_LABEL });
    expect(within(copies).getByText("prompts/01-scope.md")).toBeInTheDocument();
    expect(within(copies).getByText("refs/design.md")).toBeInTheDocument();
    expect(within(copies).queryByText("prompts")).not.toBeInTheDocument();

    // "Where did my report go" is answered before it is asked.
    const skips = screen.getByRole("list", { name: SESSION_PATTERN_SKIPS_LABEL });
    expect(within(skips).getByText("artifacts/report.md")).toBeInTheDocument();
    expect(
      within(skips).getByText(/artifacts stay with the session that produced them/),
    ).toBeInTheDocument();
    expect(within(skips).getByText(/prose never travels/)).toBeInTheDocument();
  });

  it("calls a pattern that carries nothing empty rather than showing a blank preview", () => {
    const bare = pattern({
      id: "01J5BBBBBBBBBBBBBBBBBBBBBB",
      kind: "session",
      label: "a session with only a README",
      detail: "continues this session",
      copies: [{ relPath: "workspace", isDir: true }],
      skips: [],
    });
    render(
      <SessionPatternPicker patterns={[bare]} value={bare.id} onChange={() => {}} nowMs={NOW} />,
    );
    expect(screen.getByText(SESSION_PATTERN_EMPTY_LABEL)).toBeInTheDocument();
  });

  it("puts the chosen pattern's own label on the control", () => {
    const template = pattern({ copies: [{ relPath: "README.md", isDir: false }] });
    render(
      <SessionPatternPicker
        patterns={[template, pattern({ id: "01J5CCCCCCCCCCCCCCCCCCCCCC", label: "last week" })]}
        value="_template"
        onChange={() => {}}
        nowMs={NOW}
      />,
    );
    const control = screen.getByRole("combobox", { name: SESSION_PATTERN_LABEL });
    expect(control).toHaveTextContent("Zone template");
  });
});
