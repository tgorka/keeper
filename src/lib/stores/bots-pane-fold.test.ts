import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  BOTS_PANE_FOLD_COOKIE,
  botsPaneFoldCookie,
  botsPaneFoldStore,
  hydrateBotsPaneFold,
  readBotsPaneFold,
  resetBotsPaneFoldForTest,
} from "@/lib/stores/bots-pane-fold";
import { COLUMN_FOLD_COOKIE, readColumnFold } from "@/lib/stores/column-fold";

/**
 * A jar with other people's cookies in it, and the pane's own somewhere inside.
 * One of the foreign entries is the COLUMN fold, whose `bots-list` key is the
 * neighbour this store refused to become a key of.
 */
function jar(value: string): string {
  return [
    "theme=dark",
    `${COLUMN_FOLD_COOKIE}=${encodeURIComponent("notes-rail:0|bots-list:1")}`,
    `${BOTS_PANE_FOLD_COOKIE}=${encodeURIComponent(value)}`,
  ].join("; ");
}

beforeEach(resetBotsPaneFoldForTest);

afterEach(() => {
  resetBotsPaneFoldForTest();
  // biome-ignore lint/suspicious/noDocumentCookie: arranging or clearing cookie state is this test's subject
  document.cookie = `${BOTS_PANE_FOLD_COOKIE}=; path=/; max-age=0`;
});

describe("readBotsPaneFold", () => {
  /** The default is folded, and that is the story (AD-184): flip it to
   *  `false` in `botsPaneUnfolded` and this fails alone. */
  it("starts the voice block folded when there is no cookie", () => {
    expect(readBotsPaneFold("theme=dark")).toEqual({ voice: true });
  });

  it("reads the band out of a jar full of other cookies", () => {
    expect(readBotsPaneFold(jar("voice:0"))).toEqual({ voice: false });
  });

  it("drops a malformed entry and an unknown key rather than refusing to render", () => {
    expect(readBotsPaneFold(jar("voice|pins:0"))).toEqual({ voice: true });
    expect(readBotsPaneFold(jar("voice:yes"))).toEqual({ voice: true });
  });

  it("round trips both states through its own writer", () => {
    for (const voice of [false, true]) {
      expect(readBotsPaneFold(botsPaneFoldCookie({ voice }))).toEqual({ voice });
    }
  });

  it("writes a year-long path-wide cookie under the keeper-prefixed name", () => {
    expect(botsPaneFoldCookie({ voice: false })).toBe(
      `${BOTS_PANE_FOLD_COOKIE}=${encodeURIComponent("voice:0")}; path=/; max-age=${60 * 60 * 24 * 365}`,
    );
  });
});

describe("the two folds do not read each other", () => {
  it("leaves the column fold alone when the pane's band is written", () => {
    botsPaneFoldStore.getState().toggleBand("voice");
    expect(readColumnFold(document.cookie)["bots-list"]).toBe(false);
    expect(readBotsPaneFold(document.cookie).voice).toBe(false);
  });
});

describe("the Bots pane fold store", () => {
  it("unfolds the band, writes it out, and folds it back", () => {
    botsPaneFoldStore.getState().toggleBand("voice");
    expect(botsPaneFoldStore.getState().bands.voice).toBe(false);
    expect(readBotsPaneFold(document.cookie)).toEqual({ voice: false });

    botsPaneFoldStore.getState().toggleBand("voice");
    expect(readBotsPaneFold(document.cookie).voice).toBe(true);
  });

  it("restores a remembered fold, once", () => {
    hydrateBotsPaneFold(jar("voice:0"));
    expect(botsPaneFoldStore.getState().bands).toEqual({ voice: false });

    // A second hydrate must not undo a fold the user has changed since the
    // first — the pane can mount twice and React double-invokes effects.
    botsPaneFoldStore.getState().toggleBand("voice");
    hydrateBotsPaneFold(jar("voice:0"));
    expect(botsPaneFoldStore.getState().bands.voice).toBe(true);
  });
});
