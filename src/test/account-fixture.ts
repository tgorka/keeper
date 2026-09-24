/**
 * A signed-in organisation account, as Rust would describe it (Epic 82), for
 * the suites that render something which depends on one. Each suite overrides
 * only the fields its case is about.
 */
import type { DriveOfferVm, MatrixOfferVm, OrgAccountVm, ProviderOfferVm } from "@/lib/ipc/client";

export function accountVm(over: Partial<OrgAccountVm> = {}): OrgAccountVm {
  return {
    configured: true,
    id: "acme",
    name: "Acme",
    issuerHost: "id.acme.dev",
    repoHost: "git.acme.dev",
    repoMode: "same",
    state: "ready",
    sentence: "Up to date.",
    identity: {
      login: "tgorka",
      displayName: "Tomasz Gorka",
      email: null,
      roles: ["keeper", "staff"],
    },
    device: {
      slug: "hesperia",
      name: "hesperia",
      class: "desktop",
      platform: "macos",
      thisDevice: true,
    },
    devices: [
      { slug: "hesperia", name: "hesperia", class: "desktop", platform: "macos", thisDevice: true },
      {
        slug: "iphone-3f2a",
        name: "iphone-3f2a",
        class: "mobile",
        platform: "ios",
        thisDevice: false,
      },
    ],
    lastSyncedMs: null,
    forgeConnected: false,
    faults: [],
    offers: { drives: [], providers: [], matrix: [] },
    restore: { sentence: null, pending: [], listeningOff: false },
    revision: 0,
    ...over,
  };
}

/** A drive another device syncs (Epic 84): a notes vault with a task ledger. */
export function driveOffer(over: Partial<DriveOfferVm> = {}): DriveOfferVm {
  return {
    // Identity is (normalized remote, branch, name), so the key names the branch
    // and the name below (Epic 85, AD-327/F2).
    key: "drive:https://git.acme.dev/tgorka/notes#trunk^Acme notes",
    name: "Acme notes",
    remoteUrl: "https://git.acme.dev/tgorka/notes.git",
    branch: "trunk",
    credential: "own",
    notes: "vault",
    recordings: null,
    sessions: null,
    tasks: "ledger",
    excludes: [".DS_Store"],
    lfsThresholdBytes: null,
    virtualPatterns: null,
    virtualOverBytes: null,
    releaseTtlMs: null,
    tags: [],
    commitSubjectTemplate: null,
    devices: ["iphone-3f2a", "ipad-91c0"],
    ...over,
  };
}

/** An endpoint another device uses (Epic 84), signing with the account. */
export function providerOffer(over: Partial<ProviderOfferVm> = {}): ProviderOfferVm {
  return {
    key: "provider:hermes:https://hermes.acme.dev",
    kind: "hermes",
    name: "Acme Hermes",
    baseUrl: "https://hermes.acme.dev",
    credential: "account",
    bots: ["Research", "Scheduler"],
    devices: ["iphone-3f2a"],
    ...over,
  };
}

/** A Matrix account another device is signed in to (Epic 84). */
export function matrixOffer(over: Partial<MatrixOfferVm> = {}): MatrixOfferVm {
  return {
    key: "matrix:@tgorka:acme.dev",
    userId: "@tgorka:acme.dev",
    homeserverUrl: "https://matrix.acme.dev/",
    kind: "password",
    devices: ["iphone-3f2a"],
    ...over,
  };
}
