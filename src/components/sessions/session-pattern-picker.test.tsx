/**
 * FR-253: the preview is the promise, so these tests are about what the user
 * is told BEFORE pressing Create — most of all about the files that do not
 * travel, which is the half a picker normally leaves silent.
 */
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  SESSION_PATTERN_COPIES_LABEL,
  SESSION_PATTERN_EMPTY_LABEL,
  SESSION_PATTERN_INSTALL_HINT,
  SESSION_PATTERN_INSTALL_LABEL,
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

  it("offers a named template under its own name, and sends the id verbatim", async () => {
    const named = pattern({
      id: "_template/interview",
      label: "interview",
      detail: "a named template — copied whole",
      copies: [{ relPath: "questions.md", isDir: false }],
    });
    const onChange = vi.fn();
    render(
      <SessionPatternPicker
        patterns={[pattern(), named]}
        value="_template"
        onChange={onChange}
        nowMs={NOW}
      />,
    );

    // The label is the folder's own name — keeper does not improve on what the
    // operator called it.
    const control = screen.getByRole("combobox", { name: SESSION_PATTERN_LABEL });
    fireEvent.keyDown(control, { key: "Enter" });
    fireEvent.click(await screen.findByRole("option", { name: /interview/ }));

    // `_template/interview` — the id is the directory it copies out of, and
    // nothing here composes a second spelling of it (AD-65).
    expect(onChange).toHaveBeenCalledWith("_template/interview");
  });

  it("offers keeper's own template to a zone that has none, even an empty one", () => {
    const onInstall = vi.fn();
    render(
      <SessionPatternPicker
        patterns={[]}
        value={null}
        onChange={() => {}}
        onInstallTemplate={onInstall}
        nowMs={NOW}
      />,
    );
    // The one case where a zone with nothing to pick between still renders: it
    // has a question to answer that the create row cannot ask for it.
    fireEvent.click(screen.getByRole("button", { name: SESSION_PATTERN_INSTALL_LABEL }));
    expect(onInstall).toHaveBeenCalledTimes(1);
    expect(screen.getByText(SESSION_PATTERN_INSTALL_HINT)).toBeInTheDocument();
  });

  it("still offers it beside a session-only list — a session is not a template", () => {
    render(
      <SessionPatternPicker
        patterns={[pattern({ id: "01J5DDDDDDDDDDDDDDDDDDDDDD", kind: "session", label: "friday" })]}
        value="01J5DDDDDDDDDDDDDDDDDDDDDD"
        onChange={() => {}}
        onInstallTemplate={() => {}}
        nowMs={NOW}
      />,
    );
    expect(screen.getByRole("button", { name: SESSION_PATTERN_INSTALL_LABEL })).toBeInTheDocument();
  });

  it("does not ask a zone that already answered — a named template counts", () => {
    render(
      <SessionPatternPicker
        patterns={[pattern({ id: "_template/interview", label: "interview" })]}
        value="_template/interview"
        onChange={() => {}}
        onInstallTemplate={() => {}}
        nowMs={NOW}
      />,
    );
    // FR-266's lesson, applied to the offer: `_template/interview` IS a
    // template, so the zone has one and keeper does not offer to write another.
    expect(
      screen.queryByRole("button", { name: SESSION_PATTERN_INSTALL_LABEL }),
    ).not.toBeInTheDocument();
  });

  it("cannot be pressed twice, and reports the refusal in Rust's own words", () => {
    render(
      <SessionPatternPicker
        patterns={[]}
        value={null}
        onChange={() => {}}
        onInstallTemplate={() => {}}
        installing
        installError="permission denied writing _template/AGENTS.md"
        nowMs={NOW}
      />,
    );
    expect(screen.getByRole("button", { name: SESSION_PATTERN_INSTALL_LABEL })).toBeDisabled();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "permission denied writing _template/AGENTS.md",
    );
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
