import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  SIGN_IN_LINK_COPIED_LABEL,
  SIGN_IN_LINK_COPY_LABEL,
  SIGN_IN_LINK_LABEL,
  SIGN_IN_LINK_POLL_MS,
  SIGN_IN_LINK_PROMPT,
  SignInLink,
} from "@/components/auth/sign-in-link";
import type { SignInLinkVm } from "@/lib/ipc/client";

const PAGE = "https://electra.siren-alsephina.ts.net:8451/oauth/v2/authorize?client_id=k&state=s1";

const writeText = vi.fn(() => Promise.resolve());

beforeEach(() => {
  // jsdom lacks a clipboard by default.
  Object.assign(navigator, { clipboard: { writeText } });
  writeText.mockClear();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("SignInLink", () => {
  it("renders nothing until the waiting sign-in has a page", async () => {
    const fetchLink = vi.fn(() => Promise.resolve<SignInLinkVm | null>(null));
    const { container } = render(<SignInLink fetchLink={fetchLink} />);
    await waitFor(() => expect(fetchLink).toHaveBeenCalled());
    expect(container).toBeEmptyDOMElement();
  });

  it("offers the page to copy when the browser may not have opened", async () => {
    const fetchLink = vi.fn(() =>
      Promise.resolve<SignInLinkVm | null>({ url: PAGE, openFailed: null }),
    );
    render(<SignInLink fetchLink={fetchLink} />);

    expect(await screen.findByText(SIGN_IN_LINK_PROMPT)).toBeInTheDocument();
    expect(screen.getByLabelText(SIGN_IN_LINK_LABEL)).toHaveValue(PAGE);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: SIGN_IN_LINK_COPY_LABEL }));
    expect(writeText).toHaveBeenCalledWith(PAGE);
    expect(
      await screen.findByRole("button", { name: SIGN_IN_LINK_COPIED_LABEL }),
    ).toBeInTheDocument();
  });

  it("says keeper's reason when it could not open the page itself", async () => {
    const fetchLink = vi.fn(() =>
      Promise.resolve<SignInLinkVm | null>({
        url: PAGE,
        openFailed: "keeper could not open your browser (no default browser).",
      }),
    );
    render(<SignInLink fetchLink={fetchLink} />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "keeper could not open your browser (no default browser). Copy the sign-in link and open it in your browser.",
    );
    expect(screen.queryByText(SIGN_IN_LINK_PROMPT)).not.toBeInTheDocument();
  });

  it("follows the waiting sign-in: a new page is offered uncopied, an ended one goes away", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    let current: SignInLinkVm | null = { url: PAGE, openFailed: null };
    const fetchLink = vi.fn(() => Promise.resolve(current));
    render(<SignInLink fetchLink={fetchLink} />);
    fireEvent.click(await screen.findByRole("button", { name: SIGN_IN_LINK_COPY_LABEL }));
    await screen.findByRole("button", { name: SIGN_IN_LINK_COPIED_LABEL });

    // The repository leg's page follows the account's.
    current = { url: "https://git.acme.dev/login/oauth/authorize?state=s2", openFailed: null };
    await act(async () => {
      await vi.advanceTimersByTimeAsync(SIGN_IN_LINK_POLL_MS);
    });
    expect(screen.getByLabelText(SIGN_IN_LINK_LABEL)).toHaveValue(current.url);
    expect(screen.getByRole("button", { name: SIGN_IN_LINK_COPY_LABEL })).toBeInTheDocument();

    current = null;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(SIGN_IN_LINK_POLL_MS);
    });
    expect(screen.queryByLabelText(SIGN_IN_LINK_LABEL)).not.toBeInTheDocument();
  });

  it("stops asking once asking fails", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const fetchLink = vi.fn(() => Promise.reject(new Error("ipc down")));
    render(<SignInLink fetchLink={fetchLink} />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(SIGN_IN_LINK_POLL_MS * 3);
    });
    expect(fetchLink).toHaveBeenCalledTimes(1);
  });
});
