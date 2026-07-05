import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ReadReceipts } from "@/components/chat/read-receipts";

describe("ReadReceipts", () => {
  it("renders nothing when there are no readers", () => {
    const { container } = render(<ReadReceipts readers={[]} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("renders an initials chip per reader derived from the user-id localpart", () => {
    render(<ReadReceipts readers={["@bob:example.org", "@carol:example.org"]} />);
    // Initials come from the localpart (first two letters, uppercased).
    expect(screen.getByText("BO")).toBeInTheDocument();
    expect(screen.getByText("CA")).toBeInTheDocument();
  });

  it("shows an accessible read-by label with the count", () => {
    render(<ReadReceipts readers={["@bob:example.org"]} />);
    expect(screen.getByText("Read by 1 person")).toBeInTheDocument();
  });

  it("collapses more than three readers into a +K overflow badge", () => {
    render(
      <ReadReceipts
        readers={[
          "@a:example.org",
          "@b:example.org",
          "@c:example.org",
          "@d:example.org",
          "@e:example.org",
        ]}
      />,
    );
    // Three chips shown, the remaining two collapsed into "+2".
    expect(screen.getByText("+2")).toBeInTheDocument();
    expect(screen.getByText("Read by 5 people")).toBeInTheDocument();
  });

  it("gives the same reader a stable chip color across renders", () => {
    const { container: a } = render(<ReadReceipts readers={["@bob:example.org"]} />);
    const { container: b } = render(<ReadReceipts readers={["@bob:example.org"]} />);
    const chipA = a.querySelector("span[title='@bob:example.org']");
    const chipB = b.querySelector("span[title='@bob:example.org']");
    expect(chipA?.getAttribute("style")).toBe(chipB?.getAttribute("style"));
  });
});
