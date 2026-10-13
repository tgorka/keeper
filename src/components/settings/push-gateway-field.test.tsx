import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  pushGatewayGet: vi.fn(),
  pushGatewaySet: vi.fn(),
}));

import { PUSH_GATEWAY_LABEL, PushGatewayField } from "@/components/settings/push-gateway-field";
import { pushGatewayGet, pushGatewaySet } from "@/lib/ipc/client";

const mockGet = vi.mocked(pushGatewayGet);
const mockSet = vi.mocked(pushGatewaySet);

const FULL = "https://push.example.org/_matrix/push/v1/notify";
const HTTPS_ONLY =
  "keeper registers a push gateway only over https://, and this address starts with http://.";

beforeEach(() => {
  vi.clearAllMocks();
  mockGet.mockResolvedValue({ url: null, refusal: null });
});

function field(): HTMLInputElement {
  return screen.getByLabelText(PUSH_GATEWAY_LABEL) as HTMLInputElement;
}

async function type(value: string) {
  await waitFor(() => expect(field()).toBeEnabled());
  fireEvent.change(field(), { target: { value } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
}

describe("PushGatewayField (98.1, Q4)", () => {
  it("saves what was typed and shows back what Rust stored", async () => {
    mockSet.mockResolvedValue({ url: FULL, refusal: null });
    render(<PushGatewayField open />);

    await type("https://push.example.org");

    expect(mockSet).toHaveBeenCalledWith("https://push.example.org");
    // A bare host comes back with the push path Rust filled in.
    await waitFor(() => expect(field().value).toBe(FULL));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("shows Rust's refusal and keeps what was typed when nothing was written", async () => {
    mockGet.mockResolvedValue({ url: FULL, refusal: null });
    mockSet.mockRejectedValue({ code: "internal", message: HTTPS_ONLY, retriable: false });
    render(<PushGatewayField open />);
    await waitFor(() => expect(field().value).toBe(FULL));

    await type("http://push.example.org");

    expect(await screen.findByRole("alert")).toHaveTextContent(HTTPS_ONLY);
    expect(field().value).toBe("http://push.example.org");
  });

  it("shows why a stored address will not be used", async () => {
    mockGet.mockResolvedValue({ url: "http://push.example.org", refusal: HTTPS_ONLY });
    render(<PushGatewayField open />);

    expect(await screen.findByRole("status")).toHaveTextContent(HTTPS_ONLY);
    expect(field().value).toBe("http://push.example.org");
  });

  it("clears a stored value keeper will not register even when it cannot be shown", async () => {
    mockGet.mockResolvedValue({ url: null, refusal: HTTPS_ONLY });
    mockSet.mockResolvedValue({ url: null, refusal: null });
    render(<PushGatewayField open />);
    expect(await screen.findByRole("status")).toHaveTextContent(HTTPS_ONLY);

    await type("");

    expect(mockSet).toHaveBeenCalledWith("");
    await waitFor(() => expect(screen.queryByRole("status")).not.toBeInTheDocument());
  });

  it("clears the gateway with an empty field", async () => {
    mockGet.mockResolvedValue({ url: FULL, refusal: null });
    mockSet.mockResolvedValue({ url: null, refusal: null });
    render(<PushGatewayField open />);
    await waitFor(() => expect(field().value).toBe(FULL));

    await type("");

    expect(mockSet).toHaveBeenCalledWith("");
    await waitFor(() => expect(field().value).toBe(""));
  });
});
