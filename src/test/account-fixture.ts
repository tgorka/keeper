/**
 * A signed-in organisation account, as Rust would describe it (Epic 82), for
 * the suites that render something which depends on one. Each suite overrides
 * only the fields its case is about.
 */
import type { OrgAccountVm } from "@/lib/ipc/client";

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
    revision: 0,
    ...over,
  };
}
