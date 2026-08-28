# Design research — keeper visual identity

**Status:** research input for a forthcoming `DESIGN.md`. Not itself a design decision record.
**Date:** 2026-08-11.
**Brief being served:** a new identity for keeper (Tauri + React, macOS, local-first; Chats / Notes / Files / Recordings / Sync / Settings) that is *impressive but usable*, greenish, and signed with a **robot head** rather than the current speech bubble, because bots and AI conversation are coming.
**Standard to meet:** `/workspace/tgsite/DESIGN.md` — YAML token frontmatter, prose sections, binding anti-AI guardrails, stated contrast floors, and rules written to be machine-enforceable by `scripts/check-design.mjs` (raw-colour ban, gradient ban, `backdrop-filter` ban, font allowlist, a two-site "red budget", and a mirror check that `index.css` carries exactly the frontmatter tokens).

## Method and provenance

Three classes of number appear below, and they are labelled:

- **(measured)** — pulled from shipped source at the URL given, or decoded from a shipped binary asset, on 2026-08-11. Not eyeballed from a screenshot.
- **(computed)** — arithmetic I or a research agent performed on a sourced hex: WCAG 2.1 relative luminance and contrast ratio, CIE L\*, CIELAB ΔE76, and dichromat simulation via the Viénot–Brettel–Mollon 1999 LMS projection.
- **`[INFERENCE]`** / **`[UNVERIFIED]`** — reasoning, or a claim whose primary source could not be opened. Treated as weaker evidence, never as fact.

**Cross-check performed.** The Q3 contrast table was computed independently twice — once by me, once by the research agent — from separate implementations of the sRGB linearisation. All 80 cells agreed to two decimal places. The WCAG 2.0 `0.03928` threshold and the WCAG 2.1 `0.04045` threshold were both run; **zero cells differ at 2 dp**, so the arithmetic is threshold-invariant. Quote `0.04045` (the current Recommendation) in `DESIGN.md`.

**keeper's current state**, cited throughout by `file:line`, was read directly from this worktree. The short version, so the rest of the document has something to argue against:

| Fact | Evidence | Consequence |
|---|---|---|
| Body and heading type are the same system stack | `src/index.css:8-9` — `--font-heading: var(--font-sans)`, `--font-sans: -apple-system, …` | 21 usages of `font-heading` across `src/` render identically to body. There is no typographic identity, only a system default. |
| `--font-mono` is never defined | absent from `src/index.css`; 26 `.tsx` files use `font-mono` | Mono falls through to Tailwind's stock stack. The one place keeper *is* a machine tool is unstyled. |
| The accent is already a jade-teal | `src/index.css:73` `--primary: #0f6e5c`, `:153` `--primary: #3ecfae` | OKLCH hue 175.5° / 174.0° (computed). Already off the status-green hue — see Q2C. |
| A second green carries status | `src/index.css:95,175` `--bridge-healthy: #16a34a` (both themes) | OKLCH hue 149.2° (computed). Used as `bg-bridge-healthy` dots in `bridge-card.tsx:68`, `sidebar-pane.tsx:88`, `lib/bridges.ts:72`. |
| A *third*, unchosen green exists | `settings/device-verification-dialog.tsx:115`, `settings/key-backup-dialog.tsx:257` — raw `text-emerald-600 dark:text-emerald-400` | Bypasses the token layer entirely. ΔE 23.0 from `--primary` in normal vision, **17.5 under deuteranopia** (computed) — near-collision. |
| Violet is in the palette | `src/index.css:92` `--incognito: #6d28d9`, `:172` `#a78bfa` | `#6d28d9` is Tailwind `violet-700` verbatim — the single highest-signal AI-UI tell (Q5 B1). |
| An un-overridden shadcn default survives in dark | `src/index.css:190` `--sidebar-primary: oklch(0.488 0.243 264.376)` | = `#1447e6` (computed), a blue-violet nobody picked. |
| keeper already owns its window chrome | `src-tauri/crates/keeper/tauri.conf.json:27-28` `titleBarStyle: "Overlay"`, `hiddenTitle: true`; drag band in `layout/app-shell.tsx:250` | Identity *can* live in the titlebar without new platform risk — the cost has already been paid. |
| Ten monochrome tray template icons already ship | `src-tauri/crates/keeper/icons/tray-{idle,sync,sync-up,sync-down,sync-updown,sync-refresh,sync-paused,sync-warning,recording,error}-template.png` | The menu-bar constraint in Q4C is live, not hypothetical. |
| Density is real | 376 occurrences of `text-xs` across `src/**/*.tsx` (measured) | keeper is genuinely a dense tool. Any identity that costs rows is the wrong identity. |

---

## Q1 — Dense professional desktop apps that look distinctive without hurting usability

Method note for this section: hexes marked **(measured)** were pulled from shipped stylesheets, theme files or config repos at the URLs given. Contrast ratios are computed, not audited.

### Q1a — Linear, Raycast, Warp, Ghostty, Zed, Obsidian

#### Linear

- **Identity mechanism:** a near-black canvas `--color-bg-primary: #08090a` with exactly one saturated colour in the system, indigo `--color-brand-bg: #5e6ad2`, over a four-step grey text ramp (`#f7f8f8` → `#d0d6e0` → `#8a8f98` → `#62666d`) — all **(measured)** from Linear's shipped stylesheet [`index.ONusDM1Q.css`](https://static.linear.app/web/_next/static/css/index.ONusDM1Q.css); `#08090a` is corroborated as `meta theme-color` on [linear.app/brand](https://linear.app/brand). Type is Inter + **Inter Display** ([Linear, "How we redesigned the Linear UI"](https://linear.app/now/how-we-redesigned-the-linear-ui)) on a compressed ramp — 15 / 13 / 12 / 11px — with non-standard variable weights 510 / 590 / 680.
- **How it is deployed:** grey is the whole surface; indigo is a scarce accent — one `--color-brand-bg` against 20+ neutral tokens. Elevation is four background steps whose neighbours differ by ~1.17:1: surfaces separate by hairline and near-invisible luminance step, never by shadow. The generator underneath is **LCH-based**, reducing 98 per-theme variables to three inputs — base colour, accent colour, and a **contrast scalar** (same source).
- **Usability cost:** the ramp bottoms out below AA. `--color-text-quaternary: #62666d` on `#08090a` is **3.45:1** (computed) — fails [SC 1.4.3](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html). `--color-border-primary: #23252a` on the same ground is **1.30:1**, far under [SC 1.4.11](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html)'s 3:1 — pane boundaries are functionally invisible to low-vision users. Linear ships high-contrast themes precisely because the default is not the accessible one. Being Electron, it also maintains three chrome variants: navigation had to be designed so back/forward/history/tabs were "easily removable" across macOS, Windows and browser.
- **Steal:** the three-input LCH generator with an explicit `contrast` knob. **Don't steal:** `#62666d`-class grey as the default metadata colour.

#### Raycast

- **Identity mechanism:** a **twelve-slot theme contract** and nothing else — `background`, `backgroundSecondary`, `text`, `selection`, `loader`, plus seven fixed accent roles `red, orange, yellow, green, blue, purple, magenta`, under `appearance: light|dark`. Verified by reading a shipped theme, [`themes/danhollick/greenish.json`](https://github.com/raycast/theme-explorer/blob/main/themes/danhollick/greenish.json). There is no titlebar token, no border token, no elevation token, no radius token.
- **How it is deployed:** the lever *is* the chrome, because there is barely any chrome. Scarcity is structural — the [Colors API](https://developers.raycast.com/api-reference/user-interface/colors) exposes a closed set, and any raw hex an extension passes is **auto-contrast-adjusted against the active theme** unless the author opts out with `adjustContrast: false`. The platform owns contrast, not the extension. That is the transferable idea.
- **Usability cost:** themes are Pro-only ([manual.raycast.com/themes](https://manual.raycast.com/themes)), so the identity surface is paywalled. The twelve slots have no role for borders or dividers, so nothing constrains the selection/background delta. Discovery is entirely `⌘K`-gated — the store guidelines mandate Title Case, icons and `…` on submenus ([Prepare an Extension for Store](https://developers.raycast.com/basics/prepare-an-extension-for-store)) because the Action Panel is the only place discovery happens. `[INFERENCE]` on that last cost: reasoned from the guideline's existence, not a cited complaint.
- **Steal:** the small closed token vocabulary plus platform-side contrast adjustment. **Don't steal:** omitting a hairline/divider token — keeper's dense columns need one as a first-class role.

#### Warp

- **Identity mechanism:** one brand accent `accent: "#00c2ff"`, held **identical across light and dark**, in [`warpdotdev/themes/warp_bundled`](https://github.com/warpdotdev/themes/tree/main/warp_bundled). Structurally the identity is the **Block** — "A Block groups commands and outputs into one atomic unit" ([docs.warp.dev](https://docs.warp.dev/terminal/blocks/)).
- **How it is deployed:** the YAML makes the separation most products blur: `accent` is "Color used for highlights in **Warp's UI**", while sixteen `terminal_colors` carry *content* status ([custom-themes docs](https://docs.warp.dev/terminal/appearance/custom-themes)). **One chrome accent, sixteen content colours, zero overlap.** Density is a user switch — Compact mode is off by default.
- **Usability cost:** the fixed accent is not luminance-portable. `#00c2ff` on `#000000` is **10.16:1**; the *same hex* on `#ffffff` in the shipped `warp_light.yaml` is **2.07:1** (computed) — below AA for text and below the 3:1 non-text floor. A single brand hex across both appearances is measurably broken in one of them. Separately, the block model breaks TUI composition — the first line of text is covered during SSH per Warp's own [TMUX discussion #501](https://github.com/warpdotdev/Warp/discussions/501), and users called the tmux incompatibility disqualifying on [HN](https://news.ycombinator.com/item?id=30923229).
- **Steal:** the explicit accent-vs-status separation. **Don't steal:** one accent hex across both themes.

#### Ghostty

- **Identity mechanism:** a **stated refusal to have UI identity**. macOS GUI in Swift/AppKit+SwiftUI, Linux in Zig/GTK4, both over a shared C-ABI `libghostty`; the docs say "we don't draw custom widgets and Ghostty will fit right into your desktop environment," with native tabs, splits, Quick Look, secure-input API and window state restoration ([ghostty.org/docs/about](https://ghostty.org/docs/about)). Identity therefore lives in (i) the **app icon** — `macos-icon` defaults to `official` with artist variants, and the docs state "hand-created by artists (no AI)" ([config reference](https://ghostty.org/docs/config/reference)) — and (ii) one chrome trick: `macos-titlebar-style` defaults to `transparent`, letting the terminal background bleed through the *native* bar. Verified in source: `@"macos-titlebar-style": MacTitlebarStyle = .transparent` in [`src/config/Config.zig`](https://github.com/ghostty-org/ghostty/blob/main/src/config/Config.zig). Default `window-padding-x` is 2px.
- **How it is deployed:** chrome only, minimally. The one custom-chrome path, `macos-titlebar-style = tabs`, is opt-in and Hashimoto describes it openly as "view hacking the native macOS titlebar" ([discussion #9553](https://github.com/ghostty-org/ghostty/discussions/9553)).
- **Usability cost — and this is the price list for redrawing chrome, published by the project against itself:** `window-decoration = none` **disables tabs entirely on macOS** ([discussion #7164](https://github.com/ghostty-org/ghostty/discussions/7164)); `macos-titlebar-style = hidden` means "the top titlebar area can no longer be used for dragging the window" ([config reference](https://ghostty.org/docs/config/reference)). The `tabs` style has a recurring bug tail: tabs hidden on state restore ([#3049](https://github.com/ghostty-org/ghostty/issues/3049)), the bar collapsing to a tiny box ([#13066](https://github.com/ghostty-org/ghostty/discussions/13066)), window-control collisions in fullscreen on macOS Tahoe ([#9597](https://github.com/ghostty-org/ghostty/issues/9597)).
- **Steal:** the position — keep the native titlebar, buy distinctiveness with the icon and a background tint that bleeds into it. **Don't steal:** the `tabs` approach.

#### Zed

- **Identity mechanism:** GPU-rendered UI (GPUI, custom Rust framework) plus a fully declared theme contract at [`zed.dev/schema/themes/v0.2.0.json`](https://zed.dev/schema/themes/v0.2.0.json), and a stated density: `ui_font_size: 16`, `buffer_font_size: 15`, `buffer_line_height: "comfortable"` **(measured** from [`assets/settings/default.json`](https://github.com/zed-industries/zed/blob/main/assets/settings/default.json)**)**. Typeface currency: Zed Sans/Mono were custom Iosevka builds, then moved to IBM Plex; `.ZedSans`/`.ZedMono` are now aliases and the bundled font dir contains `ibm-plex-sans` and `lilex` ([release 0.201.4](https://zed.dev/releases/stable/0.201.4)).
- **How it is deployed:** whole surface. The token names are the artefact worth copying — `title_bar.background`, `tab_bar.background`, `toolbar.background`, `panel.background`, `status_bar.background`, `scrollbar.thumb.*`, a nine-state `element.*` / `ghost_element.*` ramp, and eight `players[]` colours. From shipped [`one.json`](https://github.com/zed-industries/zed/blob/main/assets/themes/one/one.json): `title_bar.background: #3b414d` against `editor.background: #282c33` — a **1.37:1** step (computed). That token exists *because the OS bar does not*.
- **Usability cost:** GPUI has no accessibility tree. Zed's own tracker states "Zed is absolutely inaccessible for screen reader users on Windows… Zed is absolutely silent," tested with JAWS and NVDA ([#41138](https://github.com/zed-industries/zed/issues/41138)); the umbrella discussion concedes a11y "will be a long project. Likely lasting far beyond 1.0" ([#6576](https://github.com/zed-industries/zed/discussions/6576)). Chrome separation sits under the non-text floor (`border #464b57` on `#3b414d` = **1.17:1**), and `syntax.comment: #5d636f` on the editor background is **2.32:1** (both computed).
- **Steal:** the token naming discipline. **Don't steal:** the custom render stack — Tauri gives keeper a real DOM and therefore free platform a11y; discarding it is how you land in #41138.

#### Obsidian

- **Identity mechanism:** the CSS-variable surface itself — "over 400 new CSS variables" ([1.0 theme migration guide](https://obsidian.md/blog/1-0-theme-migration-guide/)), organised into Foundations / Components / Editor / Plugins / Window / Publish ([index](https://docs.obsidian.md/Reference/CSS+variables/CSS+variables)). A 12-step neutral ramp `--color-base-00 … 100`; a **user-owned accent** expressed as three HSL scalars, default `258 / 88% / 66%` = `#8a5cf5` (computed) ([Colors reference](https://docs.obsidian.md/Reference/CSS+variables/Foundations/Colors)); colour mixing moved to **OKLCH** in 1.13. Density is a published rule, not a vibe: **a 4px grid** exposed as `--size-4-1` = 4px, `--size-4-2` = 8px, `--size-4-4` = 16px, with UI type fixed at 12 / 13 / 15 / 20px ([Typography reference](https://docs.obsidian.md/Reference/CSS+variables/Foundations/Typography.md)).
- **How it is deployed:** everything, and deliberately not owned by the vendor — the brand accent is a *defaultable user variable*. Chrome likewise: three title-bar styles (Hidden, default since 1.0; Custom; Native) ([Window frame reference](https://docs.obsidian.md/Reference/CSS+variables/Window/Window+frame.md)).
- **Usability cost:** the vendor cannot guarantee its own surface. Theme authors are told the commonest maintenance failure is "broken selectors as a result of new versions of Obsidian" ([Theme guidelines](https://docs.obsidian.md/Themes/App+themes/Theme+guidelines)); 1.0 broke enough themes that incompatible ones were flagged `"legacy": true` and hidden from the gallery. The ecosystem's fix is a third-party plugin, [Style Settings](https://github.com/obsidian-community/obsidian-style-settings) — a settings UI the vendor didn't ship. And three title-bar styles × traffic-light side × popout windows × fullscreen is a combinatorial matrix the migration guide asks every author to reason about.
- **Steal:** the 4px grid published as `--size-4-N` tokens. **Don't steal:** unbounded theming across a 400-variable surface — keeper has six columns and one user, and every variable exposed is a combination someone has to keep working.

### Q1b — Things, Craft, Arc/Dia, Superhuman, Tana, Reflect

#### Things (Cultured Code)

- **Identity mechanism:** zero proprietary type or chrome — the identity is Apple's own typography plus space. Things renders inline syntax "in a unique blend of proportional and fixed-width fonts (SF Pro & SF Mono)" ([release notes](https://culturedcode.com/things/support/articles/2409117/)); "when we designed Things 3 we got rid of as much of the app's ornamentation as possible" ([blog](https://culturedcode.com/things/blog/page/9/)). Credentials are checkable: Apple Design Awards [2009](https://culturedcode.com/things/blog/2009/06/things-wins-apple-design-award-2009/) and [2017](https://culturedcode.com/things/blog/2017/06/back-from-wwdc/).
- **How it is deployed:** whole surface. Colour is rationed to small semantic dots and the Magic Plus button.
- **Usability cost — and Cultured Code conceded it.** Things 3.18 shipped adjustable text size so users could "shrink your text to fit a small iPhone, or jack up the size on a thirty-inch display", with a **density slider** in Settings → General on Mac because macOS has no Dynamic Type ([Things Big and Small](https://culturedcode.com/things/blog/2023/09/things-big-and-small/)). Shipping a user-controlled scale is the admission that one spacious default does not fit a power user on a large display.
- **Steal:** the density slider, not the spacing.

#### Craft

- **Identity mechanism:** typography as the whole brand, and all four faces are Apple's — System (San Francisco), Serif (New York), Mono (SF Mono), Round (SF Rounded); "they're all excellent because they exclusively come from Apple's own San Francisco font family" ([MacStories](https://www.macstories.net/reviews/craft-review-a-powerful-native-notes-and-collaboration-app/)). No web-font payload; distinctiveness comes from *which* system face goes where.
- **Correction to the received wisdom:** Craft has **no** Apple Design Award. It won **Mac App of the Year at the 2021 App Store Awards** ([Apple Newsroom](https://www.apple.com/newsroom/2021/12/app-store-awards-honor-the-best-apps-and-games-of-2021/)) and was an **ADA 2021 finalist**, Interaction ([Apple Developer](https://developer.apple.com/design/awards/2021/)).
- **Usability cost:** `[INFERENCE]` — no published animation cost exists to cite (native app, no inspectable stylesheet). The real cost is a build-first loop that trades design-system consistency for velocity: "rather than trying to judge if something will work on 'paper', we just code it up, use it for a couple of days or weeks, and then make a decision" ([Craft blog](https://www.craft.do/blog/how-to-elevate-your-design-daniel-korpai)).
- **Steal:** the four-face rule — one sans, one serif, one mono, one rounded, all from a single family. Personality with zero loading and zero cross-surface drift.

#### Arc / Dia (The Browser Company) — the cautionary case

- **Identity mechanism:** per-Space gradient theming as the *primary* brand surface — accent, saturation, grain, and "up to three complimentary colors… to create a gradient for the app's UI", bound per Space ([Arc Help Center](https://resources.arc.net/hc/en-us/articles/19228064149143-Spaces-Distinct-Browsing-Areas)). Stated intent was explicitly non-utilitarian: "something that felt more like a product from Nintendo or Disney than from a browser vendor" ([Letter to Arc members, 26 May 2025](https://browsercompany.substack.com/p/letter-to-arc-members-2025)).
- **Usability cost — quantified by the vendor.** Josh Miller named it the "novelty tax": "for most people, Arc was simply too different, with too many new things to learn, for too little reward," and "our metrics were more like a highly specialized professional tool (like a video editor) than a mass-market consumer product." The adoption numbers, same letter: **5.52% of DAUs used more than one Space regularly; 4.17% used Live Folders; 0.4% used Calendar Preview on Hover.** Scott Forstall's note to the team: Arc "felt like a saxophone — powerful but hard to learn"; make it a piano.
- **Currency, verified and dated:** Arc moved to maintenance on **26 May 2025** (same letter). Dia's stated principles are "simplicity over novelty… hide complexity behind familiar interfaces"; reviewers describe the result as "clean, light, and very approachable… much closer to Chrome or Safari" ([Efficient App](https://efficient.app/apps/dia)). Ownership changed too: Atlassian agreed to acquire The Browser Company for **$610M all-cash on 4 Sep 2025** ([CNBC](https://www.cnbc.com/2025/09/04/atlassian-the-browser-company-deal.html)).
- **Don't steal:** per-context gradient theming. Six greenish surfaces that each repaint the chrome is Arc's Spaces, and Arc's own telemetry says 94.5% of users never engaged with the multi-context idea.

#### Superhuman

- **Identity mechanism:** a published latency budget as the brand. "The 100ms rule states that every digital interaction should be faster than 100ms… Superhuman Mail treats the 100ms rule as the maximum, and actually aims for latency less than **50ms** whenever possible" ([Superhuman blog, primary](https://blog.superhuman.com/superhuman-is-built-for-speed/)). Vohra escalated it publicly to **<32 ms** ("1–2 Chrome frames"), quoted in the same post.
- **How it is deployed:** whole-app, and it *removes* visual identity rather than adding it — "Superhuman Mail uses minimal animations, so no time is wasted on loading them. Gestures are also made easier or avoided altogether in favor of keyboard shortcuts." Chrome is thin because chrome costs frames.
- **Usability cost — the invisible interface has a price, and they paid it in humans.** "At Superhuman, we initially mandated that all new customers attend a **30-minute human onboarding**." Making it optional dropped attendance to **15% from 100%**; early sessions ran 90 minutes. Their justification names the mechanism: "Learning keyboard shortcuts is a very unusual experience. Like a piano lesson, our customers benefitted from a real human…" ([First Round Review](https://review.firstround.com/superhuman-onboarding-playbook/)).
- **Currency, verified:** Grammarly announced the acquisition **1 Jul 2025** ([Grammarly](https://www.grammarly.com/blog/company/grammarly-to-acquire-superhuman/)); on **29 Oct 2025** Grammarly renamed *the company* to Superhuman ([Grammarly](https://www.grammarly.com/blog/company/announcing-company-rebrand-to-superhuman/)).
- **Steal:** the published budget. **Skip:** the concierge — surface shortcut hints inline instead.

#### Tana

- **Identity mechanism:** density as identity, and it is measurable. **(measured** from Tana's shipped stylesheet `app.tana.inc/static/index-B90PJN8R.css`, 2026-08-11**)**: `--listItemVerticalSpacing: .25rem` (**4px** per outline node), `--listItemHorizontalSpacing: .4rem`, `--levelIndent: 1.25rem` (**20px per nesting level**). Layer separation comes from a **20-step neutral ramp in 25-unit increments** (`--colorGray25: #fcfcfd` … `--colorGray500: #7d798f`) — near-identical greys *instead of borders*. Fonts: Inter/InterVariable/InterDisplay plus Soehne.
- **Usability cost:** a documented multi-week ramp — "Expect 2 weeks before feeling productive" ([aiproductivity.ai](https://aiproductivity.ai/tools/tana/)); "only after **10–15 hours** of learning supertags and typed queries" ([Pickuma](https://pickuma.com/for-dev/tana-personal-knowledge-management-review/)). The density is not the barrier — the user-authored typed-schema vocabulary is.
- **Steal:** the numbers (4px row padding, 20px indent per level, a 15–20 step grey ramp replacing borders). **Don't steal:** the user-authored type system.

#### Reflect

- **Identity mechanism:** near-monochrome plus one accent, one self-hosted sans. **(measured** from `reflect.app/_next/static/css/309c0da8ff6730a0.css`, 2026-08-11**)**: self-hosted Inter plus `ui-monospace` for code, and a tight scale of **12 / 14 / 16 / 20 / 24 / 32px**. By frequency the palette is `#111827` (25), `#e5e7eb` (22), `#374151` (22), `#d1d5db` (20), `#9ca3af` (19), `#4b5563` (15), `#6b7280` (11), `#f3f4f6` (10) — against exactly one prominent chromatic accent, `#3b82f6` (9). Note honestly: that neutral ramp *is* Tailwind's stock grey, so read it as "they chose the stock neutrals", not "they hand-picked these".
- **Usability cost:** the monochrome leaves no colour channel for status, so state rides on weight and opacity and on filter chips rather than colour coding ([changelog](https://reflect.app/changelog)) — `[INFERENCE]`. Also worth noting against any "minimal = fast" assumption: they "spent a lot of our time over the past year rewriting the entire frontend codebase to be powered by **SQLite**" to fix load times on large collections. Small type is cheap; a fast dense list is not.
- **Steal:** the ratio — roughly **8 neutrals to 1 accent**, one sans, six type sizes.

### Cross-cutting read

| | Identity lever | Deployment | The named cost |
|---|---|---|---|
| Linear | `#08090a` + one indigo `#5e6ad2`, LCH generator, 3 inputs | scarce accent, grey everything | `#62666d` text = 3.45:1; borders = 1.30:1 |
| Raycast | 12-slot theme contract, platform-side contrast adjust | *is* the chrome | paywalled; no border token; ⌘K-gated discovery |
| Warp | one accent `#00c2ff`; blocks as atomic units | accent = chrome, ANSI 16 = content | same hex in light = 2.07:1; tmux/TUI breakage |
| Ghostty | refuses custom chrome; icon + transparent native titlebar | chrome only, 2px padding | opt-in custom titlebar → lost drag, lost tabs, bug tail |
| Zed | GPUI + explicit chrome token vocabulary, 16/15px | whole surface | zero screen-reader support; 1.17:1 borders |
| Obsidian | 400+ CSS vars, 4px grid, user-owned OKLCH accent | whole surface, user-owned | theme rot across releases; 3-way titlebar matrix |
| Things | Apple system type + space, no ornament | whole surface | fewer rows; answered with a density slider |
| Craft | four faces, all from the SF family | whole document surface | design-system drift from a build-first loop `[INFERENCE]` |
| Arc | per-Space gradient chrome | whole chrome | 5.52% multi-Space DAU; app retired to maintenance |
| Superhuman | a published <50ms latency budget | whole app, subtractive | required 30-min human onboarding; 15% attendance when optional |
| Tana | 4px rows, 20px indent, 20-step grey ramp | whole surface | 10–15h learning curve (schema, not density) |
| Reflect | ~8 neutrals : 1 accent `#3b82f6`, 6 type sizes | scarce accent | no colour channel left for status |

**Two mechanisms recur in every one that stayed usable, and both are cheap for a Tauri app:**

1. **Separate the chrome accent from the status ramp.** Warp does it explicitly in YAML (`accent` = UI, `terminal_colors` = content); Zed does it via `element.*` vs `error/warning/success`.
2. **Make contrast a parameter, not a constant.** Linear's `contrast` scalar; Raycast's `adjustContrast`.

**The one that consistently costs the most is redrawing the OS title bar.** Ghostty documents the losses in its own config reference; Zed pays for it with a `title_bar.background` token *and* no a11y tree; Obsidian offloads it onto theme authors as a three-way matrix. keeper has already taken this on (`tauri.conf.json:27-28`) — which means the cost is sunk and the band should now be *used*, not that more chrome should be invented.

---

## Q2 — Green as a product colour

Dark-surface reference for this section is `#161616` (Carbon's `$background` for the g100 theme).

### Q2A — Named green families, with sourced hexes

"Survives at UI scale" = passes 3:1 non-text contrast (SC 1.4.11) as a 2px border **and** 4.5:1 as a 12px label, against `#161616` and against white.

| Family | Hexes + source | Hue | Signals | UI-scale verdict (computed) |
|---|---|---|---|---|
| **Terminal phosphor** | `#09CC50` — PCjs author: "I eventually settled on `#09CC50` for PCjs' default monochromatic green" ([pcjs.org, 2018-11-15](https://www.pcjs.org/blog/2018/11/15/)). `#00AA00` / `#55FF55` = CGA and IBM 5153 schemes in Microsoft's shipped [`defaults.json`](https://raw.githubusercontent.com/microsoft/terminal/main/src/cascadia/TerminalSettingsModel/defaults.json); `#008000` / `#00FF00` = "Vintage", same file. Physics caveat: green screens used **P1** phosphor, but the **IBM 5151 used P39** ([IBM 5151](https://en.wikipedia.org/wiki/IBM_5151)) — "P1 green" and "the green of a 5151" are different claims. | 120–142° | Machine, log, raw output. Reads as *content*, not brand. | Dark: all pass (`#09CC50` = 8.4:1). Light: **all fail** (`#09CC50` = 2.2:1). Dark-only family. |
| **"Matrix" green — the trap** | `#33FF33` and `#00FF41` are **folklore**. No primary source found — no studio style guide, no production document. Treat as unsourced. | 120° / 135° | Costume. | `#33FF33` on white = **1.36:1**; `#00FF41` = **1.37:1**. On dark they hit ~13:1, which *is* the trap: at 100% saturation they out-shout everything, so a 12px green label becomes the loudest thing on screen. Also, a monochromatic phosphor is generally outside sRGB — no hex is "the" phosphor colour. |
| **Sage / moss** | `#81b88b` = VS Code `gitDecoration.addedResourceForeground` dark; `#73C991` untracked; light-mode added `#587c0c` — [`extensions/git/package.json`](https://raw.githubusercontent.com/microsoft/vscode/main/extensions/git/package.json). Solarized green `#859900`. | 131°, S = 28% | Quiet, editorial, "already handled". The only green family that reads as *typography* rather than *signal*. | Dark 7.9:1 → passes both tests. Light 2.3:1 → fails. This is the family that actually ships at 13px in a dense IDE. |
| **Jade / emerald** | `#3DDC84` — Android robot, official: "The color value for print is `PMS 2412C` and the online hex color is `#3ddc84`" ([Android brand guidelines](https://developer.android.com/distribute/marketing-tools/brand-guidelines)). `#3ECF8E` — Supabase, measured from [`supabase-logo.svg`](https://raw.githubusercontent.com/supabase/supabase/master/apps/studio/public/supabase-logo.svg), confirmed as `--brand-default: 153.1deg 60.2% 52.7%` in the shipped [dark theme](https://raw.githubusercontent.com/supabase/supabase/master/packages/ui/build/css/themes/dark.css). `#079355` = Adobe Spectrum `green-800`. `#22A06B` = Atlassian `Green600`. | 147–155° | Manufactured, precise, slightly cold. Not "nature", not "success". | At the darker end passes both polarities (`#079355`: 4.6:1 dark / 4.0:1 light). **The only green family that survives light *and* dark at small scale.** |
| **British racing green** | `#004225`, HSV 154°/100%/26° — [Wikipedia](https://en.wikipedia.org/wiki/British_racing_green), which states the honest caveat: "There is no exact hue for BRG; the term is used to denote a spectrum of deep, rich greens." BS 381C is a paint-chip standard; no hex is canonical. | 154° | Heritage, engineered, expensive. | **1.56:1 against `#161616`** — invisible as a hairline or a label on a dark UI. Large flat field only (11.6:1 vs white, so good as a *surface* carrying white type). |
| **Oxide / patina / verdigris** | `#43B3AE` — Wikipedia [Verdigris](https://en.wikipedia.org/wiki/Verdigris) infobox; weak sourcing (the article carries a cleanup banner). The Statue of Liberty patina has **no single value**: "It is hard to give it a hex code because it's not flat" ([The Paris Review, 2020-11-24](https://www.theparisreview.org/blog/2020/11/24/verdigris-the-color-of-oxidation-statues-and-impermanence/)). Nearest shipped analogue: Carbon `teal60` `#007d79` / `teal50` `#009d9a`. | 177–179° | Age, oxidation, patient craft. Reads teal, not green — which is the point. | `#009d9a` = 5.4:1 dark / 3.3:1 light → passes both at 2px and 12px. |
| **Olive / drab** | FS 34087 `#3C3421`, FS 34088 `#3C341F`, FS 33070 `#544F3D` — circulated via vendor FED-STD-595 conversion charts. **`[UNVERIFIED]`**: the standard defines *paint chips*, and the PDF text could not be read directly. | 42° — not a green at all in sRGB terms | Utility, issued equipment. | `#3C3421` = **1.47:1 against `#161616`** — literally invisible on a dark app, and reads brown even as a field. Do not use. |

### Q2B — Shipped products that own a green, and how

| Product | Green (sourced) | Hue | How it is used |
|---|---|---|---|
| **Spotify** | **`#1ED760`** — **measured** from Spotify's own press-kit asset `Spotify_Primary_Logo_RGB_Green.png` ([media kit](https://newsroom.spotify.com/media-kit/logo-and-brand-assets/)): the flat region is exactly `#1ED760` across 528,471 opaque pixels. **The widely-repeated `#1DB954` is not the colour of the current shipped logo asset.** No first-party page states a hex in text. | 141° | Scarce accent on near-black — logo, play button, progress fill. Never a large field. |
| **Duolingo** | Feather Green **`#58CC02`** (PMS 361 C) and Mask Green `#89E219`, from the official [brand guidelines](https://design.duolingo.com/identity/color): "Feather Green is the core color of our brand… When in doubt, lean in to green!" Secondaries Cardinal `#FF4B4B`, Bee `#FFC800`, Fox `#FF9600`, Macaw `#1CB0F6`. Measured discrepancy: the shipped App Store icon is `#78C800`/`#8EE000`, off-book. | 94° | Whole surface + chrome. Green *is* the product; every other state got a non-green hue. |
| **Robinhood** | **`#CCFF00`** — measured from the App Store icon and confirmed in robinhood.com's inlined CSS (`background-color:#CCFF00`). | 72° (chartreuse) | Chrome + CTA, deliberately *not* the market-up green. |
| **Cash App** | **`#00E013`** — from the inline logo SVG on [cash.app](https://cash.app/) (`fill="#00e013"`), reused as `--button-background-color` and as full-bleed section backgrounds. | 125° | Whole surface. |
| **Starbucks** | `#006242` / `#00754A` in starbucks.com's served CSS. | 160° / 158° | Chrome and large field. `#006241` is **2.43:1 on dark** — a light-mode surface colour only. |
| **Node.js** | **`#5FA04E`** — the sole fill in the official [`jsIconGreen.svg`](https://raw.githubusercontent.com/nodejs/nodejs.org/main/apps/site/public/static/logos/jsIconGreen.svg). | 108°, S = 34% | Mark only. Desaturated leaf green, deliberately off both the status hue and the saturation band. |
| **Supabase** | **`#3ECF8E`**, = `--brand-default: 153.1deg 60.2% 52.7%`. | 153° | Scarce accent on near-black — and crucially the *only* green in the theme. See C2. |
| **NVIDIA** | **`#76B900`** in nvidia.com's served markup; their [brand page](https://www.nvidia.com/en-us/about-nvidia/legal-info/logo-brand-usage/) refers to "the NVIDIA green background". | 82° | Large flat field + mark. Never a status colour. |
| **Grammarly** | **`#027E6F`** in grammarly.com's served markup. (The `#34A853` on that page is Google's, from the sign-in button.) | 173° | Brand shifted fully into teal; status rides on underline colour + glyph. |
| **VS Code** | `#81b88b` added / `#73C991` untracked / `#E2C08D` modified / `#c74e39` deleted. Brand accent is blue. | 131° | **State colour only**, desaturated to 28% S, always paired with a letter badge (`A`/`M`/`D`/`S`) in [`decorationProvider.ts`](https://raw.githubusercontent.com/microsoft/vscode/main/extensions/git/src/decorationProvider.ts). |
| **Android** | `#3DDC84` / PMS 2412C, official. | 147° | Mark only — **and a hard constraint on keeper's brief.** The same page states: *"You may not file trademark applications for or claim trademark rights to the Android robot logo or any derivatives thereof."* A **green robot head** is Android's exact trademark territory. This is checkable law, not a design opinion. |

**Verified decoys — do not cite these as green-owning products:** Hetzner's brand is red ([brand page](https://www.hetzner.com/legal/hetzner-brand/)); the greens on deno.com are third-party framework logos (Node, Vue, Nuxt) and Deno's own mark is monochrome; Kagi, Fathom and 1Password show no green in their served markup (**1Password's brand is blue**); Bear / iA Writer / Instapaper could not be verified either way.

### Q2C — The trap: brand green vs status green

#### What the conventions actually are (from shipped token sources, not blog summaries)

| System | Green means | Source-level fact |
|---|---|---|
| **IBM Carbon** | success, exclusively | `support.success → {green.50}` = `#24a148`. Decisively, `interactive` and `link.primary` → `{blue.60}` = `#0f62fe` ([white.json](https://raw.githubusercontent.com/carbon-design-system/carbon/main/packages/themes/src/dtcg/white.json), [colors.ts](https://raw.githubusercontent.com/carbon-design-system/carbon/main/packages/colors/src/colors.ts)). **Carbon's brand hue is blue precisely so green can stay semantic.** |
| **GitHub Primer** | success **and** primary action **and** contribution density | `fgColor.success` = `#1a7f37`; `bgColor.success.emphasis` = `#1f883d` ([fgColor.json5](https://raw.githubusercontent.com/primer/primitives/main/src/tokens/functional/color/fgColor.json5), [bgColor.json5](https://raw.githubusercontent.com/primer/primitives/main/src/tokens/functional/color/bgColor.json5)). |
| **Adobe Spectrum** | `positive` | Two hard rules on the [colour system page](https://spectrum.adobe.com/page/color-system/): *"When using color with semantic meaning, you must also display text or an icon"*, and *"Spectrum only supports the use of colored text for the accent and negative semantics"* — i.e. **green text is forbidden**; positive green may only be a background, border or icon. |
| **Atlassian** | success is **Lime**; green is explicitly *meaningless* | Current [`@atlaskit/tokens`](https://unpkg.com/@atlaskit/tokens/dist/cjs/artifacts/tokens-raw/atlassian-light.js): `color.icon.success` = `#6A9A23` (Lime600). Meanwhile `color.background.accent.green.*` carries the description *"Use for green backgrounds when there is no meaning tied to the color."* |
| **Material 3** | **no success role exists** | M3 ships `error`/`onError`/`errorContainer` and no success/positive role at all ([Color roles](https://m3.material.io/styles/color/roles)). Material's answer to the collision is to not have a status green. |
| **Apple HIG** | positive, but culture-dependent | *"Avoid relying solely on color to differentiate between objects, indicate interactivity, or communicate essential information."* With a worked example: *"Green indicates a positive trend in the Stocks app in English… Red indicates a positive trend in the Stocks app in Chinese."* System green light `#34C759` ([HIG Color](https://developer.apple.com/design/human-interface-guidelines/color)). |

#### Who solved it, and by exactly what mechanism

1. **Atlassian — moved *status* off green into lime, and stripped green of meaning.** Datable from npm: `@atlaskit/tokens@7.0.0` (2025-10-15) still had `color.icon.success` = `#22A06B` Green600; `@atlaskit/tokens@8.0.0` (2025-11-07) has `#6A9A23` Lime600. Separation: accent-green `#22A06B` (155°) vs success-lime `#6A9A23` (84°) → **ΔE 36.7** normal, 37.1 deuteranopia, 33.3 protanopia (computed).
2. **Supabase — deleted the success token entirely.** The shipped dark theme defines exactly four semantic `*-default` tokens — `brand` (153°), `destructive` (10°), `warning` (39°), `secondary`. **There is no `--success` token in either theme.** Status is red/amber only; positive state is the brand green or nothing. The cheapest correct answer, and the most under-used.
3. **GitHub Primer — ships whole alternate colour-vision themes that replace green with blue.** `fgColor.success` carries `org.primer.overrides` entries `'light-protanopia-deuteranopia': '{base.color.blue.5}'` and `'dark-protanopia-deuteranopia': '{base.color.blue.3}'`; even the contribution graph swaps its green ramp for a blue one in [`contribution.json5`](https://raw.githubusercontent.com/primer/primitives/main/src/tokens/component/contribution.json5). There is an engineering comment in-source justifying a bespoke hex: `'light-high-contrast': '#04591f', /* … level 5 fails 7:1 vs bgColor-muted, level 6 is too dark to visually differentiate status colors. */`
4. **VS Code — desaturated the status green and added a non-chromatic channel.** Added files are `#81b88b` at **28% saturation** versus Carbon's success at 63%; every coloured decoration also carries a letter badge. Brand accent is blue; green never appears as chrome. This is why VS Code can put a green label at 13px next to an amber one and stay readable.
5. **Robinhood — moved the brand away from green so green stays a state colour.** Brand at 72° chartreuse vs the market-up convention at ~137–141°. `[INFERENCE]` on the in-app half: `#CCFF00` was verified in the icon and site CTA, but no Robinhood page stating the in-app gain/loss hexes could be retrieved.
6. **Carbon — the brand simply isn't green.** Zero contention by construction. The boring answer, and the reason enterprise systems rarely have this problem.

#### Who did not solve it

1. **GitHub — the most airtight failure, visible in the token graph.** In [`button.json5`](https://raw.githubusercontent.com/primer/primitives/main/src/tokens/component/button.json5) the primary button is literally `primary.bgColor.rest → {bgColor.success.emphasis}`. The most-used CTA in the product **is aliased to the success status token**. On one pull-request page, `#1f883d`-family green simultaneously means *this is the primary action*, *this succeeded*, and *this is contribution density*. Three unrelated meanings, one scale. Green has stopped being information on GitHub; it is just "GitHub-coloured".
2. **Spotify — the brand hue sits on the status hue to within one degree.** Measured brand `#1ED760` = **141.4°**; Carbon `green40` = 141.5°, Primer `bgColor.success.emphasis` = 137.1°. ΔE between Spotify's brand green and Carbon's `green40` is **19.4 normal, 12.5 under deuteranopia** — inside the range where two colours read as "the same green, slightly different". There is no hue headroom left. `[INFERENCE]` on the product consequence; the arithmetic collision is proven.
3. **Cash App — brand green as an entire surface, at 125°.** `#00E013` is the logo fill, the primary button *and* full-bleed section backgrounds in one document. At 10.1:1 on dark and 100% saturation it cannot be scaled down into an accent, and it leaves nowhere to put "transaction succeeded".
4. **Duolingo — deliberate collapse, which works only because the product has one positive meaning.** Green is the brand *and* "correct answer". They get away with it because every other state was assigned a non-green hue up front and there is exactly one thing green can mean. **keeper has at least four** (bridge healthy, sync clean, recording ok, device verified), so this strategy does not transfer.

#### Colour vision, with numbers

Prevalence: congenital red–green CVD "affects up to 1 in 12 males (8%) and 1 in 200 females (0.5%)", and "common colors of confusion include red/brown/green/yellow" ([Color blindness, Wikipedia](https://en.wikipedia.org/wiki/Color_blindness)).

The governing requirement is **WCAG 2.2 SC 1.4.1 Use of Color (Level A)**, verbatim from [W3C Understanding SC 1.4.1](https://www.w3.org/WAI/WCAG22/Understanding/use-of-color.html):

> Color is not used as the only visual means of conveying information, indicating an action, prompting a response, or distinguishing a visual element.

With the escape hatch that matters for a dense UI, also verbatim:

> If content is conveyed through the use of colors that differ not only in their hue, but that also have a significant difference in lightness, then this counts as an additional visual distinction, as long as the difference in relative luminance between the colors leads to a contrast ratio of 3:1 or greater.

Why green-vs-amber and green-vs-red pairs fail — ΔE76 on the sourced hexes, normal vs simulated dichromacy (computed):

| Pair | ΔE normal | ΔE deuteranopia | ΔE protanopia |
|---|---|---|---|
| VS Code added `#81b88b` vs modified `#E2C08D` | 36.1 | 20.2 | **9.6** |
| Primer success `#1f883d` vs danger `#cf222e` | 110.7 | **18.0** | 25.5 |
| Primer success `#1f883d` vs attention `#9a6700` | 63.7 | 25.6 | **15.6** |
| Atlassian success-lime `#6A9A23` vs danger `#C9372C` | 92.5 | **8.3** | 39.9 |

Read the last row carefully: **Atlassian's lime move fixed the brand collision and made the success-vs-error pair *worse* for deutans** — ΔE 8.3, effectively the same colour. That is the hidden cost of solving the collision by rotating toward yellow, and it is why their success tokens still require an icon.

#### Which mechanism is best, and why — teal-jade, not lime

Rotate the **brand** green *up* the hue wheel into teal-jade (165–177°) and keep true green (137–150°) as the only status green. Not lime, not desaturation alone, not "brand green as a large field only".

The reason is arithmetic. Holding S = 60% / L = 53% and measuring against a 137° status green `#24a148` (computed):

| Brand hue | Hex | ΔE normal | ΔE deuteranopia | ΔE protanopia |
|---|---|---|---|---|
| 84° (lime — Atlassian's direction) | `#96cf3f` | 34.3 | 34.0 | **29.8** |
| 120° | `#3fcf3f` | 28.8 | 25.7 | 27.5 |
| 145° | `#3fcf7b` | 17.4 | 16.9 | 17.0 |
| 153° (Supabase / Android) | `#3fcf8e` | 22.2 | 22.6 | 22.1 |
| 160° | `#3fcf9f` | 29.1 | 30.0 | 29.3 |
| **168°** | **`#3fcfb2`** | **38.1** | **38.7** | **38.2** |
| 175° | `#3fcfc3` | 46.9 | 47.3 | 46.5 |

Both directions reach ΔE ≈ 34–38 in normal vision, so on that axis lime and teal look equivalent. **They are not.** Lime separation *degrades* under protanopia (34.3 → 29.8) because it rides the red–green axis, which is the damaged one. Teal separation is *flat* (38.1 → 38.7 / 38.2) because it opens the blue–yellow S-cone axis, which protans and deutans retain. A teal-jade brand is as far from the status green as Atlassian's lime is from its accent green **and keeps that distance for 8% of men**.

---

## Q3 — Dark-first vs light-first, and the contrast numbers

### Q3A — The actual evidence on polarity

**The "positive polarity advantage" (dark text on light) is real, replicated, and mechanistically explained.**

| Claim | Evidence |
|---|---|
| Positive polarity wins on acuity *and* proofreading, young *and* old | Piepenbrock, Mayr, Mund & Buchner, *Ergonomics* 56(7):1116–24 (2013): "A positive polarity advantage was found for both age groups. The presentation in positive polarity is recommended for all ages." ([PubMed 23654206](https://pubmed.ncbi.nlm.nih.gov/23654206/)) |
| The mechanism is pupil size | Piepenbrock, Mayr & Buchner, *Ergonomics* 57(11):1670–7 (2014): "pupil sizes were smaller and proofreading performance was better with positive than with negative polarity displays." Smaller pupil → pinhole effect → greater depth of field → sharper retinal image. ([PubMed 25135324](https://pubmed.ncbi.nlm.nih.gov/25135324/)) |
| The advantage **grows as text gets smaller** — directly relevant to keeper's 376 `text-xs` sites | Piepenbrock, Mayr & Buchner, *Human Factors* (2014), "Positive Display Polarity Is Particularly Advantageous for Small Character Sizes" ([SAGE](https://journals.sagepub.com/doi/abs/10.1177/0018720813515509)); via [NN/g](https://www.nngroup.com/articles/dark-mode/): "the positive-polarity advantage increased linearly as the font size was decreased." |
| Users **cannot feel** the difference | Same study via NN/g: performance was better in light mode but "participants in the study did not report any difference in their perception of text readability." **Do not settle this by preference poll.** |

**The counter-case — genuine, but narrower than folklore claims:**

- **Cloudy ocular media.** Legge, Rubin, Pelli & Schleske, *Vision Research* 25(2):253–65 (1985) ([PubMed 4013092](https://pubmed.ncbi.nlm.nih.gov/4013092/)); per NN/g's reading of the full paper, "each of the 7 participants with cloudy ocular media had better reading rates with dark modes."
- **Ambient light interacts.** Dobres et al. (MIT AgeLab), *Applied Ergonomics* 2017: "during daytime, there was no significant effect of contrast polarity, but during nighttime, light mode led to better performance than dark mode" ([PDF](https://jdobr.es/pdf/Dobres-etal-2017-Ambient.pdf)).
- **The strongest pro-dark finding.** Aleman, Wang & Schaeffel, *Scientific Reports* 8:10840 (2018): after one hour, black-on-white thinned the choroid by −16.13 ± 4.54 µm (p = 8.3 × 10⁻⁵) while white-on-black thickened it by +9.96 ± 6.51 µm (p = 0.0067) ([nature.com](https://www.nature.com/articles/s41598-018-28904-x)). n = 7, one hour, no epidemiological follow-up — the authors say so.
- **The astigmatism/halation prevalence claim is `[UNVERIFIED]`.** The ubiquitous "50% of people have astigmatism, so dark mode blurs for them" traces to unreferenced secondary sources. The optical mechanism is sound (Q3D); the population statistic is not. **Do not put a prevalence number in `DESIGN.md`.**

**Apple HIG › Dark Mode** ([source](https://developer.apple.com/design/human-interface-guidelines/dark-mode)):

- *"**Avoid offering an app-specific appearance setting.** … they may think your app is broken because it doesn't respond to their systemwide appearance choice."* — Apple is **against** the manual override most apps ship. Shipping one is a deliberate deviation, defensible for a pro tool, but it is a decision.
- *"The color palette in Dark Mode includes dimmer background colors and brighter foreground colors… these colors aren't necessarily inversions of their light counterparts."*
- Contrast floor: *"At a minimum, make sure the contrast ratio between colors is no lower than 4.5:1. For custom foreground and background colors, **strive for a contrast ratio of 7:1**, especially in small text."*
- Glare: *"**Soften the color of white backgrounds** … to prevent the background from glowing in the surrounding Dark Mode context."*
- Testing: *"in Dark Mode with **Increase Contrast and Reduce Transparency** turned on (both separately and together)."*
- Note: the HIG page does **not** contain a "never use pure black" sentence. That rule is Material's.

**Material 2 dark theme** ([2020-01-14 Wayback capture](https://web.archive.org/web/20200114125626/https://material.io/design/color/dark-theme.html); `#121212` also shipped as `design_dark_default_color_background` in [material-components-android](https://github.com/material-components/material-components-android/blob/master/lib/java/com/google/android/material/color/res/values/colors.xml)):

- *"A dark theme uses **dark gray, rather than black** … The recommended dark theme surface color is **#121212**."*
- *"A dark theme should **avoid using saturated colors** … **Saturated colors also produce optical vibrations against a dark background, which can induce eye strain.**"*
- *"use **lighter tones (200-50)** in dark theme, rather than your default color theme (saturated tones ranging from 900-500)."*

### Q3B — What shipping BOTH costs, concretely

1. **Tokens don't double — they multiply.** In shipped [`primer/primitives`](https://github.com/primer/primitives/blob/main/src/tokens/functional/color/fgColor.json5), the single token `fgColor.success` carries a base value plus **seven** overrides: `dark`, `dark-high-contrast`, `dark-dimmed-high-contrast`, `light-high-contrast`, `dark-protanopia-deuteranopia`, `dark-protanopia-deuteranopia-high-contrast`, `light-protanopia-deuteranopia`. One green = eight values.
2. **Every accent needs two values — mechanical, not stylistic.** Primer's own numbers (ratios computed): `fgColor.success` light `#1a7f37` scores **5.08 on white ✅ / 4.04 on `#010409` ❌**; dark `#3fb950` scores **2.54 on white ❌ / 8.08 on `#010409` ✅**. Neither hex works in the other theme.
3. **Shadows stop being elevation.** [material-components-android `Dark.md`](https://github.com/material-components/material-components-android/blob/master/docs/theming/Dark.md): *"Shadows are less effective in an app using a dark theme… In order to compensate for this, Material surfaces become lighter and more colorful at higher elevations."* M2 also forbids the lazy substitute: *"Don't use light glows in place of dark shadows to express elevation."* You need **two elevation strategies**: box-shadow in light, surface-lightness steps and hairlines in dark.
4. **Assets need two variants.** Apple HIG: *"Design separate interface icons for the light and dark appearances if necessary."* For keeper that means the mark, the Files/Notes preview screenshots, and the **markdown/code syntax theme**.
5. **The test matrix multiplies past 2×.** Apple requires testing with Increase Contrast and Reduce Transparency "both separately and together" → 2 appearances × 4 flag combinations = 8 render states per surface, before keeper's six columns × empty/loading/error.
6. **The shipped engineering write-up.** [Building dark mode on Stack Overflow](https://stackoverflow.blog/2020/03/31/building-dark-mode-on-stack-overflow/): exploratory PR July 2019 → PoC October 2019 → *"After at least 60 follow-up pull requests, the dark mode beta went live on March 30, 2020."* Naive inversion "made everything have unusable contrast". Build-time colour maths had to die: *"`darken(var(--red-500), 5%)` breaks the compiler"* — every derived hover/border state had to be re-authored as an explicit token. Honest summary: **dark mode was the forcing function for a design-system migration. Budget it that way or not at all.**

### Q3C — The numbers

**Definitions, quoted.** *Contrast ratio*, [WCAG 2.1 glossary](https://www.w3.org/TR/WCAG21/#dfn-contrast-ratio): "(L1 + 0.05) / (L2 + 0.05), where L1 is the relative luminance of the lighter of the colors". *Relative luminance*, [same](https://www.w3.org/TR/WCAG21/#dfn-relative-luminance): "**L = 0.2126 × R + 0.7152 × G + 0.0722 × B** where … if R<sub>sRGB</sub> **≤ 0.04045** then R = R<sub>sRGB</sub>/12.92 else R = ((R<sub>sRGB</sub>+0.055)/1.055) ^ 2.4".

*SC 1.4.3 (AA)*: 4.5:1 for text, 3:1 for large text. *Large scale*, [glossary](https://www.w3.org/TR/WCAG21/#dfn-large-scale): "at least **18 point or 14 point bold**" — and since CSS fixes 1in = 96px and 1pt = 1/72in ([css-values-3](https://www.w3.org/TR/css-values-3/#absolute-lengths)), **18pt = 24px and 14pt bold = 18.67px bold**. *SC 1.4.11 (AA)*: 3:1 for UI components and graphical objects, with the operational note from [Understanding 1.4.11](https://www.w3.org/WAI/WCAG21/Understanding/non-text-contrast.html) that computed values **should not be rounded** — "2.999:1 would not meet the 3:1 threshold" — and the warning that thin hairlines anti-alias fainter than specified, so authors should "use a combination of colors that **exceeds** the normative requirements". That last note bears directly on keeper's 1px column separators.

**Validation of the implementation:** `#ffffff` vs `#000000` = **21.00** (spec maximum); `#767676` on white = **4.54**, matching the grey W3C publishes as the AA-passing border in Understanding 1.4.11 Figure 2.

**The table** (computed, 2 dp). Dark-pass requires the threshold on *both* near-blacks; light-pass requires it on *both* white and warm paper.

| green | L\* | `#0e1210` | `#101014` | `#ffffff` | `#f6f2e9` | dark 4.5 | dark 3.0 | light 4.5 | light 3.0 |
|---|---|---|---|---|---|---|---|---|---|
| `#00ff41` | 87.9 | 13.82 | 13.90 | 1.37 | 1.22 | ✅ | ✅ | ❌ | ❌ |
| `#33ff33` | 88.2 | 13.92 | 14.00 | 1.36 | 1.21 | ✅ | ✅ | ❌ | ❌ |
| `#3ecfae` | 75.3 | 9.65 | 9.71 | 1.96 | 1.75 | ✅ | ✅ | ❌ | ❌ |
| `#4ade80` | 79.2 | 10.83 | 10.89 | 1.74 | 1.56 | ✅ | ✅ | ❌ | ❌ |
| `#22c55e` | 70.2 | 8.28 | 8.33 | 2.28 | 2.04 | ✅ | ✅ | ❌ | ❌ |
| `#16a34a` | 58.8 | 5.73 | 5.76 | 3.30 | 2.95 | ✅ | ✅ | ❌ | ❌ |
| `#15803d` | 46.9 | 3.76 | 3.78 | 5.02 | 4.49 | ❌ | ✅ | ❌ | ✅ |
| `#10b981` | 66.8 | 7.44 | 7.48 | 2.54 | 2.27 | ✅ | ✅ | ❌ | ❌ |
| `#059669` | 54.9 | 5.01 | 5.04 | 3.77 | 3.37 | ✅ | ✅ | ❌ | ✅ |
| `#047857` | 44.4 | 3.44 | 3.46 | 5.48 | 4.91 | ❌ | ✅ | ✅ | ✅ |
| `#0f6e5c` | 41.3 | 3.06 | 3.08 | 6.17 | 5.52 | ❌ | ✅ | ✅ | ✅ |
| `#2dd4bf` | 76.9 | 10.14 | 10.20 | 1.86 | 1.67 | ✅ | ✅ | ❌ | ❌ |
| `#5eead4` | 85.0 | 12.76 | 12.83 | 1.48 | 1.32 | ✅ | ✅ | ❌ | ❌ |
| `#84cc16` | 74.9 | 9.55 | 9.61 | 1.98 | 1.77 | ✅ | ✅ | ❌ | ❌ |
| `#a3e635` | 84.3 | 12.52 | 12.59 | 1.51 | 1.35 | ✅ | ✅ | ❌ | ❌ |
| `#6ee7b7` | 83.9 | 12.38 | 12.45 | 1.52 | 1.36 | ✅ | ✅ | ❌ | ❌ |
| `#8aa065` | 62.9 | 6.56 | 6.60 | 2.88 | 2.58 | ✅ | ✅ | ❌ | ❌ |
| `#7ba05b` | 61.7 | 6.31 | 6.35 | 2.99 | 2.68 | ✅ | ✅ | ❌ | ❌ |
| `#4d7c0f` | 47.0 | 3.78 | 3.80 | 4.99 | 4.47 | ❌ | ✅ | ❌ | ✅ |
| `#166534` | 37.3 | 2.65 | 2.66 | 7.13 | 6.38 | ❌ | ❌ | ✅ | ✅ |

Provenance: `#4ade80 #22c55e #16a34a #15803d #166534` = Tailwind green-400…800; `#6ee7b7 #10b981 #059669 #047857` = emerald-300/500/600/700; `#5eead4 #2dd4bf` = teal-300/400; `#a3e635 #84cc16 #4d7c0f` = lime-400/500/700 — all verified against [tailwindcss v3.4.17 `src/public/colors.js`](https://github.com/tailwindlabs/tailwindcss/blob/v3.4.17/src/public/colors.js).

**The mechanical conclusion — it is not "essentially never", it is provably never.** Because contrast depends only on relative luminance, the requirement collapses to a luminance band:

| Requirement | Constraint on the accent |
|---|---|
| 4.5:1 on `#ffffff` | Y ≤ 0.1833 → **L\* ≤ 49.9** |
| 4.5:1 on `#f6f2e9` | Y ≤ 0.1588 → **L\* ≤ 46.8** |
| 4.5:1 on `#0e1210` | Y ≥ 0.2004 → **L\* ≥ 51.9** |
| 4.5:1 on `#101014` | Y ≥ 0.1989 → **L\* ≥ 51.7** |

The intersection is **empty**. No colour of *any* hue clears 4.5:1 on both a near-black and a white surface. Zero of the 20 candidates do. **A dual-theme product needs two accent hexes, full stop.**

The 3:1 (large-text / UI-component) band *does* have a solution: **L\* 40.7 – 58.3**. Five candidates live there and pass 3:1 on all four surfaces — `#0f6e5c` (41.3), `#047857` (44.4), `#15803d` (46.9), `#4d7c0f` (47.0), `#059669` (54.9). Useful for a shared border/focus-ring green; useless for body-weight accent text.

**Refinement of the usual L\* bands:**
- **Light theme: L\* ≈ 41–45, not 40–48.** `#15803d` (L\* 46.9) passes white at 5.02 but **fails warm paper at 4.49** — a warm `#f6f2e9` costs ~10.6% of ratio versus pure white, and per SC 1.4.11's note 4.49 does not round up. If keeper ships a warm light surface, the ceiling is **L\* ≤ 46.8**.
- **Dark theme: L\* ≈ 55–70, not 75–85.** The floor is L\* 51.9. An L\* 80 accent buys 9.5–13:1 against a 4.5 requirement — 2–3× more contrast than needed, purchased entirely in glare and halation.

**The bright-green hazard, quantified.** `#00ff41` (S = 100%, L\* 87.9) is **13.82:1** on `#0e1210` — **3.07× the AA floor** — and **1.37:1 on white**, 3.3× short of AA and still 2.2× short of the 3:1 UI threshold; on warm paper, 1.22:1, all but invisible. For calibration, `#e6e8e6` body text on `#0e1210` is 15.32:1. **`#00ff41` at 13.82 is body-text-brightness green** — a column of it is a light source, not an accent.

### Q3D — Halation and the saturated-accent-on-dark hazard

1. **Pupil aperture.** Dark mode dilates the pupil, removing the pinhole effect that masks the eye's optical defects — the positive-polarity mechanism read backwards ([PubMed 25135324](https://pubmed.ncbi.nlm.nih.gov/25135324/)). Any uncorrected refractive error is *more* visible in dark mode.
2. **Intraocular scatter.** Legge et al. attributed low-vision preference for dark mode to "abnormal light scatter due to cloudy ocular media". Corollary: a bright saturated glyph on a dark field is a small high-luminance source in a dark surround — precisely the configuration that makes scatter visible as a halo.
3. **Longitudinal chromatic aberration — and why green is the *safest* saturated hue.** Chromatic difference of focus spans roughly 2–2.5 D across 400–700 nm ([Thibos et al., *Applied Optics* 31:3594, 1992](https://opg.optica.org/ao/abstract.cfm?uri=ao-31-19-3594)); the wavelength actually in focus sits in the green — Cooper & Pease measured a mean **wavelength in focus of 518 nm at 3 m** ([PubMed 3364521](https://pubmed.ncbi.nlm.nih.gov/3364521/)). Saturated blue and deep red sit furthest from best focus and blur worst. **This is a genuine, citable argument for keeper's green identity** — but it removes only the *chromatic* component of blur, not the luminance-driven halation that `#00ff41` at 13.82:1 delivers.
4. **Optical vibration / simultaneous contrast.** Material: "Saturated colors also produce optical vibrations against a dark background, which can induce eye strain." Apple states the perceptual asymmetry that causes designers to under-correct: *"In bright surroundings, colors look darker and more muted. **In dark environments, colors appear bright and saturated.**"* ([HIG Color](https://developer.apple.com/design/human-interface-guidelines/color)).

**Four mitigations, each with its source and number:** (1) never pure `#000000` — Material's `#121212`; (2) never pure `#ffffff` body text — `#e6e8e6` on `#0e1210` is 15.32:1, still 3.4× AA with materially less bloom; (3) reduce saturation and use a lighter *tone*, not a brighter *neon* — `#059669` gives 5.01:1 (11% above AA) for the same job `#00ff41` does at 13.82:1 (207% above AA); (4) **restrict area** — M2: "Reserve bright colors for smaller surfaces… they can emit too much brightness."

---

## Q4 — Robot-head marks

Method note: 16×16 rasters below were produced by flattening the real vendor SVG and scanline-rasterising at 4× supersampling, **alpha only** (colour discarded, exactly as a macOS template image would). "Mush %" = share of inked pixels landing on partial alpha, i.e. pixels that cannot render crisply.

### Q4A — How AI/agent products actually draw their mark

**The six tropes, with proof they are tropes:**

| Trope | Checkable artifact |
|---|---|
| **The sparkle / four-point star** | It is a *standard library icon*: Google ships it as Material Symbols [`auto_awesome`](https://fonts.gstatic.com/s/i/short-term/release/materialsymbolsoutlined/auto_awesome/default/24px.svg); Apple ships SF Symbols `sparkles`. When "AI" has a named glyph in two platform icon fonts, using it is not a design decision. Measured at 16px: **83% mush, 14 solid pixels.** |
| **The chat bubble** | keeper's current mark — and the thing every bridged messenger already uses. It says "messaging", not "agent". |
| **Gradient blob / orb** | Dies by definition under `NSImage.isTemplate`. A gradient has one alpha value: opaque. |
| **Anthropomorphic smiley robot** | Collapses to a featureless disc — see Hugging Face, measured. |
| **Neural-net node graph** | Nodes are 1–2px and edges <1px at 16. Fails the GNOME 2px rule outright ([GNOME HIG](https://developer.gnome.org/hig/guidelines/ui-icons.html)). |
| **Brain** | Requires convolutions — exactly the high-frequency detail Apple tells you to delete: *"The 16x16 px @1x icon has no EKG line and no grid lines"* ([HIG › Icons](https://developer.apple.com/design/human-interface-guidelines/icons)). |

**Anthropic / Claude.** No brand page exists at `anthropic.com/brand` (404). The authority is the [`brand-guidelines` skill in `anthropics/skills`](https://github.com/anthropics/skills/blob/main/skills/brand-guidelines/SKILL.md): Dark `#141413`, Light `#faf9f5`, accents Orange **`#d97757`**, Blue `#6a9bcc`, Green `#788c5d`. Confirmed from shipped source — [`claude.com/favicon.svg`](https://claude.com/favicon.svg) carries literally `fill="#D97757"`. ⚠️ The skill lists Poppins/Lora as fonts; those are artifact-generation fallbacks. The real typography is **Styrene** (Commercial Type) and **Tiempos** (Klim), per the studio that built the identity, [Geist](https://geist.co/work/anthropic). **Construction, measured:** a single closed path, one subpath, **158 vertices, 145 straight-line commands and exactly one curve** — it is a *polygon*; the "hand-drawn" quality is faceting, not curvature. Twelve arms at deliberately jittered 18°–38° spacing where a regular star would be a uniform 30°, with a **waist radius only 0.19 of the tip radius**. That waist ratio is what kills it: at 16px, **28 solid pixels, 128 antialiased — 82% mush.**

**Hugging Face.** [`huggingface_logo-noborder.svg`](https://huggingface.co/front/assets/huggingface_logo-noborder.svg) is a direct trace of the 🤗 emoji: yellow disc `#FFD21E`, orange ring `#FF9D0B`, dark eye-blobs `#3A3B45`, red mouth `#FF323D`. Its entire identity lives in *colour contrast between adjacent filled regions* — there is no alpha boundary between face and eyes. At 16px alpha-only it is only 23% mush, **but that number lies: the shape survives and the mark does not.** Every feature merges into one disc.

**Ollama** — line-art llama head ([official mark, LobeHub static mirror](https://registry.npmmirror.com/@lobehub/icons-static-svg/latest/files/icons/ollama.svg)). Directly relevant since keeper is local-first. **9 solid pixels, 70 antialiased, 88.6% mush — the worst in the survey.** The lesson is not "no mascots"; it is that a mascot needs a second, separately drawn 16px reduction, because it cannot be derived by scaling.

**Mistral** — [`mistral.ai/favicon.svg`](https://mistral.ai/favicon.svg) is **10 `<rect>` elements, no paths at all**, every edge on a uniform 23-unit module of a 183 viewBox; the M is negative space between five colour bands (`#FFAF01 #FF8204 #FA500F #E10500 #C4001D`). At 16px it is **the crispest mark tested — 13% mush**. **And that is the sharpest trap in the survey: geometry survived, identity did not.** In mono it is one flat slab. Grid discipline is necessary but not sufficient.

**Perplexity and Cursor — the pattern that actually survives.** Both are **a solid outer container with the glyph cut out as negative space**: Perplexity's black rounded square with an orthogonal-bar knockout ([favicon](https://www.perplexity.ai/favicon.svg)) at 45% mush, Cursor's squircle with a hollow cursor-arrow void ([favicon](https://cursor.com/favicon.svg)) at 42% — **both still legible.** This is the single most transferable finding: **outer contour = one big filled shape; identity = the holes.** That is precisely what an alpha-only template image can express.

**Linear — the developer-tool tell worth copying.** [`linear.app/static/favicon.svg`](https://linear.app/static/favicon.svg) is authored at **`viewBox="0 0 16 16"` with `rx="2"`**. Linear ships its mark in a *sixteen-unit coordinate space* — a company deciding the 16px case is canonical and everything else is an upscale.

**Zed** publishes a real [brand page](https://zed.dev/brand) with three permitted colours (brand blue `#1348DC`, white, black) and the instruction "Ensure that you don't render them in any other colors." Its recursive folded Z measures **72% mush** at 16px and the spiral visibly breaks — a beautiful mark the vendor evidently accepts will be illegible below ~32px. **Warp** refreshed its wordmark but [kept the glyph](https://www.warp.dev/blog/world-of-warp) — glyph continuity over novelty, the right instinct for a tool already in someone's dock. **LM Studio / Replicate:** no clean vector or brand page retrievable; geometry not described rather than guessed.

**Pattern across the developer tools: none of them use a face.** Zed, Linear, Warp, Cursor and Raycast all sign themselves with an abstract orthogonal or geometric glyph in one or two flat colours, and at least two publish an explicit monochrome-permitted rule. **A developer tool signs itself like a component, not like a character.**

### Q4B — Older computing marks, stated as rules you can follow

**1. Bell System / Saul Bass, 1969.** Bass removed the text from inside the ring because "the text in the ring was so tiny it was barely legible at smaller sizes" ([1000logos](https://1000logos.net/bell-system-logo/)); rings and typeface were made *bolder*, not finer. The symbol had to go on over a million phone booths, making it the largest corporate rebranding programme in US history ([Logo Histories](https://www.logohistories.com/p/logo-design-saul-bass-bell)), and reached a **93% US recognition rate** ([Bell System Memorial](https://memorial.bellsystem.com/bell_logos.html)).
> **Rule: when a mark must survive a reduction, delete the smallest element entirely rather than shrinking it, and thicken what remains.** Not "simplify" — *subtract and embolden*. (A formal distance-testing protocol is `[UNVERIFIED]`; the sources support simplification driven by small-size legibility, not a measured distance study.)

**2. Susan Kare, Macintosh, 1982–83 — the single most relevant reference here.** Kare bought "the smallest graph paper" she could find and ruled out a **32×32 grid — 1,024 squares, one square = one pixel** — filling them with pencil before touching a screen ([AIGA](https://www.aiga.org/membership-community/aiga-awards/2018-aiga-medalist-susan-kare); [Smithsonian/Lemelson](https://invention.si.edu/invention-stories/susan-kare-iconic-designer)). She drew on mosaic, needlepoint and pointillism — craft traditions where the unit is discrete and non-negotiable. The artifact is citable: **MoMA object 112.2015**, "Apple Macintosh OS icon sketchbook, 1982" ([MoMA](https://www.moma.org/collection/works/188382)).
> **Rule: the pixel grid is the design surface, not an output format.** Draw the 16px version *first* by filling cells, and derive the 1024px master by integer upscale — never the reverse. A mark authored at 16 and scaled ×64 is exactly crisp at every power of two; a mark authored at 1024 and downsampled to 16 is mush. Every measurement in Q4A is that lesson.

**3. Xerox PARC → Star 8010, 1981.** The Star's symbol set was drawn by Norm Cox, who is explicit that constraint drove form: the widget "was designed out of pure necessity… constrained by the limits of the display at the time (black and white, 72 dpi)" ([Cox interview](https://medium.com/readme-mic/a-conversation-with-norm-cox-creator-of-the-hamburger-menu-c913daea5f9e)). The document icon's folded corner was lifted from a physical embossed label on an office copier.
> **Rule: pick one physical affordance and abstract exactly one feature of it.** The folded corner is a *single* deviation from a rectangle and carries the whole meaning. One idea per glyph.

**4. Braun / Dieter Rams.** The ten principles ([Vitsœ](https://www.vitsoe.com/gb/about/good-design)). Three bind here: **#5 unobtrusive** — "Products fulfilling a purpose are like tools… their design should be neutral and restrained"; **#8 thorough down to the last detail** — "Nothing must be arbitrary or left to chance"; **#10 as little design as possible.**
> **Rule for a tool someone stares at all day: the mark must be able to disappear.** #5 is binding — a mark that demands attention in a sidebar every day becomes an irritant. #8 forbids the half-pixel: every coordinate is a decision.

**5. Teenage Engineering OP-1.** Rooted in the Ulm School's pictography; Sweden's Design S committee: "through a clever colour scheme and fantastic graphics [it] is intuitive, easily accessible and incredibly inviting" ([Wikipedia](https://en.wikipedia.org/wiki/Teenage_Engineering)). All icons, numerals and display UI were drawn by hand as a **closed set**, not sampled from a library.
> **Rule: design the mark and the UI glyph set in the same pass, in the same geometry.** keeper has ten tray states already; the head and the states must be one family or the head will look pasted on.

**6. Sinclair ZX Spectrum / Rick Dickinson, 1982.** The rainbow flash is four stripes at **24° from vertical** ([Sinclair Wiki](https://sinclair.wiki.zxnet.co.uk/wiki/ZX_Spectrum+)); the machine is in the V&A permanent collection ([O1300970](https://collections.vam.ac.uk/item/O1300970/zx-spectrum-personal-computer-dickinson-rick/)).
> **Rule: a fixed angle, repeated, is an identity you can apply to things that are not the logo.** The flash is not the mark — it is a reusable geometric constant.

### Q4C — The practical constraint

#### 16px — what actually survives

Three independent current primary specs converge:

| Source | Spec |
|---|---|
| [GNOME HIG › UI Icons](https://developer.gnome.org/hig/guidelines/ui-icons.html) | "Symbolics are drawn as **16×16px SVGs**"; "**2px strokes** … with 1px strokes avoided where possible"; "**Align all shapes to the pixel grid**"; "Defined in monochrome, then programmatically recolored"; identify "**a single property** to communicate" |
| [IBM Design Language › UI icons](https://www.ibm.com/design/language/iconography/ui-icons/design/) | 16×16 artboard with 1px padding → **14×14 live area**; **2px stroke**; whole pixels only, **no decimals**; strokes expanded to outlines; delivered at `#000000` with no styling |
| [Apple HIG › Icons](https://developer.apple.com/design/human-interface-guidelines/icons) | Progressive detail removal is mandatory: "The 32x32 px icon has fewer grid lines and a thicker EKG line. The 16x16 px @2x icon retains the EKG line but has no grid lines. The 16x16 px @1x icon has no EKG line and no grid lines." |

**Operative budget at 16×16:** 14×14 live area; minimum feature 2px in *both* axes; whole-pixel coordinates; 2–3 shapes; one idea. [SC 1.4.11](https://www.w3.org/TR/WCAG22/) additionally binds the tinted/mono variants at 3:1 against adjacent colours.

#### macOS dock icon — this changed, and it changed a lot

Verified current: the [HIG App icons page](https://developer.apple.com/design/human-interface-guidelines/app-icons) carries a changelog entry **"June 8, 2026 — Refined guidance for Liquid Glass"** (prior entry June 9, 2025, layered icons). The published spec table reads: *iOS, iPadOS, macOS · Layout shape: Square · Icon shape after system masking: Rounded rectangle · Layout size: 1024×1024 px · Style: **Layered** · Appearances: Default, dark, clear light, clear dark, tinted light, tinted dark.*

Three material changes:

1. **macOS app icons are now square, like iOS.** The old free-form silhouette with a baked drop shadow is gone. You supply a square, full-bleed, **unmasked** layer set; the system applies the mask. Apple: *"Providing layers with pre-defined masking negatively impacts specular highlight effects and makes edges look jagged."* **Do not pre-round your corners.**
2. **An app icon is now a layered document, not an image.** Authored in [Icon Composer](https://developer.apple.com/documentation/xcode/creating-your-app-icon-using-icon-composer), layers exported as SVG, **text converted to outlines**, maximum four groups. Crucially: *"Remove blurs and shadows, and specular, opacity, and translucency settings. Remove background colors and gradients."* The system owns all of that now.
3. **Six appearance variants, auto-generated if you don't supply them.** [Adopting Liquid Glass](https://developer.apple.com/documentation/technologyoverviews/adopting-liquid-glass) instructs: *"Consider a simplified design comprised of solid, filled, overlapping semi-transparent shapes"*; the HIG adds *"Prefer clearly defined edges in foreground layers"* and *"avoid extremely thin line weights and sharp corners."*

**What this means for a robot head:** the mark must survive being **auto-masked** (no content in the corners), **auto-tinted** (a single hue across the whole icon — all internal colour relationships collapse), and **rendered as clear glass** over arbitrary wallpaper. *A mark whose meaning depends on the colour difference between two adjacent filled areas has already failed the tinted variant* — before you even reach the menu bar. Apple's instruction to "keep your icon's core visual features the same in the default, dark, clear, and tinted appearances" makes this non-optional.

#### macOS menu-bar template icon — alpha is the whole design

From [`NSImage.isTemplate`](https://developer.apple.com/documentation/appkit/nsimage/istemplate): *"Images you mark as template images should consist of **only black and clear colors**. You can use the alpha channel … to adjust the opacity of black content."* And [HIG › The menu bar](https://developer.apple.com/design/human-interface-guidelines/the-menu-bar): *"Both interface icons and symbols use black and clear colors to define their shapes; the system can apply other colors to the black areas … so it looks good on both dark and light menu bars, **and when your menu bar extra is selected**."* — *"The menu bar's height is 24 pt."*

**Stated bluntly: no colour, no fills that depend on colour. Alpha is the entire design. Your only expressive variables are silhouette and hole.**

**This is live for keeper, and the shipped assets were measured.** All ten `tray-*-template.png` files in `src-tauri/crates/keeper/icons/` are **44×44 RGBA with a 38×38 content bbox at offset (3,3)**, and **every non-transparent pixel is exactly RGB(0,0,0)** — the template contract is correctly honoured today. **But they are not drawn on the pixel grid.** Fraction of inked pixels on *partial* alpha: `tray-idle` **75%**, `tray-sync-warning` 75%, `tray-sync` 74%, `tray-sync-{down,up,updown,refresh}` 74%, `tray-sync-paused` 72%, `tray-recording` 72%, `tray-error` 47%. `tray-idle` has just **166 fully-opaque pixels against 506 antialiased ones**. That three-quarters-partial-alpha signature is what you get from scaling vector strokes down to 44px rather than authoring on grid. At the 24pt menu bar these render soft.

Also binding, and stated better by GNOME than by Apple: *"When a metaphor relies on negative space, make sure it will work with the colors inverted."* The menu bar inverts on highlight.

#### The measured survives / dies list

All at 16×16, alpha only, same rasteriser:

| Mark | Solid px | Antialiased | Mush | Verdict |
|---|---:|---:|---:|---|
| Ollama llama | 9 | 70 | **89%** | **DIES.** Line-art mascot. Nothing left. |
| Material `auto_awesome` sparkle | 14 | 68 | **83%** | **DIES.** Concave cusps are sub-pixel. |
| Claude asterisk | 28 | 128 | **82%** | **DIES.** 12 arms, waist/tip 0.19. |
| Zed recursive Z | 50 | 128 | **72%** | **DIES.** Spiral folds merge. |
| Linear disc + chords | 44 | 82 | 65% | **Marginal.** Disc holds, chords blur. |
| Perplexity container + void | 140 | 114 | 45% | **SURVIVES.** |
| Cursor squircle + void | 122 | 87 | 42% | **SURVIVES.** |
| Hugging Face 🤗 | 154 | 47 | 23% | **DIES** — shape survives, *mark* does not. |
| Mistral 5-band M | 80 | 12 | 13% | **Geometry survives, identity dies.** Colour was the idea. |
| Directions A–D below | 122–148 | **0** | **0%** | **SURVIVE** by construction. |

**What dies and why:** the gradient orb dies at the definition (a gradient has one alpha value). The detailed robot face dies on feature count — eyes + mouth + antenna + panel lines is 6+ shapes in a 14×14 live area — and even the *undetailed* smiley died because its features were colour, not alpha. Sparkle, neural graph and brain die on stroke weight. The chat bubble survives 16px trivially and is disqualified on **meaning**, not geometry. **And anything whose identity is a colour relationship dies twice over** — Mistral and Hugging Face both render perfectly and communicate nothing.

**The geometry contract for what survives:** (1) one closed silhouette that reads from its outer contour alone; (2) 2–3 connected ink components maximum; (3) **exactly one negative-space idea** — the holes carry the identity, because holes are alpha and alpha is all you have; (4) nothing thinner than 2px in either axis, ink *or* gap; (5) whole-pixel coordinates, authored at 16 and upscaled by integers; (6) legible inverted.

#### Four drawable directions

All 16×16, 1px padding, whole-pixel, **zero antialiasing by construction**. `##` = one pixel.

**Direction A — "Visor Keep."** *Bass rule: delete the smallest element, embolden what remains.* Head silhouette x1–x14, y3–y13, chamfered 1px at all four corners; a 2×2 antenna stud at x7–x8, y1–y2. One negative idea: an **8×3 visor slot at x4–x11, y7–y9**. Side walls 3px, brow 4px, jaw 4px. — 122px ink · 1 component · min run 2px · 1 hole.

```
       ##
   ##########
  ############
 ##############
 ###        ###
 ###        ###
 ##############
  ############
   ##########
```

**Direction B — "Kare Eyes."** *Kare rule: fill cells; nothing smaller than the unit.* Solid head x1–x14, y3–y11, chamfered; 4×1 antenna at x6–x9, y2; 4×2 neck at x6–x9, y12–y13. Negative: **two 2×2 eyes** at (x3–x4, y6–y7) and (x11–x12, y6–y7). — 126px ink · 1 component · min run 2px · 2 holes. *The most conservative option: unmistakably a robot head, one negative idea.*

```
      ####
  ############
 ##############
 ##..######..##
 ##..######..##
 ##############
  ############
      ####
```

**Direction C — "Bridged Visor."** *Mistral/Sinclair rule: build only from grid modules; identity from rhythm.* Two 2×2 antenna studs at x4–x5 and x10–x11, y1–y2. Head x1–x14, y4–y9, stepping in to y12. One negative band across y6–y7 from x3 to x12, **bridged by a 2px ink column at x7–x8** — two 4×2 eyes that read as one visor. — 122px ink · 1 component · min run 2px · 2 holes. Inverts to a clean negative head.

```
    ##    ##
  ############
 ##############
 ##....##....##
 ##....##....##
 ##############
  ############
   ##########
```

**Direction D — "Cyclops."** *Rams #10 plus TE's single-aperture discipline.* Capsule head x1–x14, y2–y13, chamfered top and bottom. **One** negative element: a **4×4 square eye at x6–x9, y6–y9**, dead centre. Nothing else. Doubles as a lens/shutter (Recordings) and as a keyhole ("keep"). — 148px ink · 1 component · **min run 4px** (the most robust of the four) · 1 hole.

```
  ############
 ##############
 ##############
 #####....#####
 #####....#####
 #####....#####
 ##############
  ############
```

**Production note tying the three constraints together.** Author the winning direction as a **16×16 cell fill** (Kare's method), then upscale by integers: ×2 → 32, ×4 → 64 … ×64 → the 1024×1024 Icon Composer master. Every intermediate size is exact, with **zero antialiasing** — precisely what keeper's current tray icons, at 72–75% partial alpha, do not have. The same 16×16 source exported black-on-clear becomes the `tray-*-template.png` set; the same silhouette as a square, full-bleed, *unmasked* SVG layer with no shadows or gradients becomes the Icon Composer foreground. **One geometry, three destinations** — the only way the six appearance variants, the squircle mask and the inverting menu bar can all be satisfied at once. **The green then lives entirely in the Icon Composer background layer and the app UI, never in the mark's internal structure — because in two of the three destinations, colour does not exist.**

---

## Q5 — The 2026 AI-UI tells

**Framing.** NN/g's *State of UX 2026*: "UI is cheaper to produce, due to standardization… **If you're just slapping together components from a design system, you're already replaceable by AI**," and "The backlash against AI slop (in all its many forms) will increase" ([NN/g, 16 Jan 2026](https://www.nngroup.com/articles/state-of-ux-2026/)). Anthropic ships an official acknowledgement in its own cookbook: "You tend to converge toward generic, 'on distribution' outputs. In frontend design, this creates what users call the 'AI slop' aesthetic" ([claude-cookbooks](https://github.com/anthropics/claude-cookbooks/blob/main/coding/prompting_for_frontend_aesthetics.ipynb)).

### Q5A — The tells, mechanically detectable

#### A1 · The specific gradients

**The proximate cause is documented and admitted.** Adam Wathan, Tailwind's creator, 7 Aug 2025: *"I'd like to formally apologize for making every button in Tailwind UI `bg-indigo-500` five years ago, leading to every AI generated UI on earth also being indigo."* ([x.com/adamwathan/status/1953510802159219096](https://x.com/adamwathan/status/1953510802159219096)). The mechanism is statistical: "you're not getting design. You're getting the median of every Tailwind CSS tutorial scraped from GitHub between 2019 and 2024. And that median is purple" ([prg.sh](https://prg.sh/ramblings/Why-Your-AI-Keeps-Building-the-Same-Purple-Gradient-Website)).

Literal hexes, from [tailwindcss v3.4.17 `src/public/colors.js`](https://github.com/tailwindlabs/tailwindcss/blob/v3.4.17/src/public/colors.js): `indigo-500 #6366f1` (the original sin), `violet-500/600/700 #8b5cf6 / #7c3aed / #6d28d9`, `purple-500 #a855f7`, `fuchsia-500 #d946ef`, `pink-500 #ec4899`.

Linter rules: reject `from-purple-500`, `to-pink-500`, `from-indigo-500`, `via-purple-500`, `from-violet-*`, `to-fuchsia-*`; regex `bg-gradient-to-\w+.*\b(from|via|to)-(indigo|violet|purple|fuchsia|pink)-\d{3}`; raw-hex regex `#(6366f1|8b5cf6|7c3aed|6d28d9|a855f7|d946ef|ec4899)`; and the **animated aurora/blob background** — `filter: blur(…)` ≥ 40px on an absolutely-positioned decorative div, or ≥ 2 stacked `radial-gradient()`s in one `background`.

#### A2 · The specific card patterns

- **Bento grid** — `rounded-2xl border border-gray-200 shadow-sm p-6` in a `grid grid-cols-4` with `col-span-2 row-span-2` hero cells. The invariant is stated numerically: "The radius (20 to 28px) plus a single consistent gap is what reads as bento" ([Superdesign](https://www.superdesign.dev/styles/bento-grid)).
- **The three-column icon-card row** — detect a `grid-cols-3` whose three children each contain, in order, a `rounded-lg`/`rounded-xl` `bg-*-100` container wrapping a 24px icon, an `h3` with `font-semibold`, and a `<p class="text-sm text-gray-500">` of ≤2 lines. **Tailwind's own docs page ships this exact molecule as its dark-mode example** — `<span class="inline-flex items-center justify-center rounded-md bg-indigo-500 p-2 shadow-lg">` → `<h3 class="mt-5 text-base font-medium tracking-tight">` → `<p class="text-gray-500 mt-2 text-sm">` ([tailwindcss.com/docs/colors](https://tailwindcss.com/docs/colors)). That is the training datum, verbatim, in the canonical doc.
- **The glass card** — `bg-white/10 backdrop-blur-lg border border-white/20 rounded-2xl`. Detect co-occurrence of `backdrop-blur-*` with `bg-white/\d+` **on a content container** (scrims exempt — see B3).

#### A3 · The specific typefaces

Anthropic's own isolated typography prompt is the cleanest ban list in existence, verbatim from the cookbook:

> **Never use:** Inter, Roboto, Open Sans, Lato, default system fonts

and, critically, it flags its own escape hatch as already burned:

> "You still tend to converge on common choices (**Space Grotesk**, for example) across generations. Avoid this"

**The Inter irony:** Inter is a genuinely excellent screen face, "designed specifically for computer screens… so the face would hold up at 13px UI labels and 72px hero text alike" ([Font Compressor](https://fontcompressor.com/blog/inter-font-guide)). That is exactly the problem — "the model isn't choosing Inter because it's right for your product. It's choosing Inter because Inter is the statistical center of every modern UI in its training data" ([aiskill.market](https://aiskill.market/blog/banning-inter-the-font-tell)).

**Second-wave tells (2025–26)**, detectable by literal family name: `Geist`/`Geist Mono` (Vercel's own, [vercel.com/font](https://vercel.com/font), self-described as "drawing inspiration from the renowned Swiss design movement"), `Space Grotesk`, `Cal Sans`, `Satoshi`, `Clash Display`, `Cabinet Grotesk`. The last three are literally the cookbook's own "Startup:" suggestion list — **following the anti-slop prompt now produces the next generation of slop.**

**The hero string:** `text-6xl font-bold tracking-tight` on an `<h1>`, usually `text-center`. Detect: `text-[5-9]xl\b(?=[^"]*\bfont-bold\b)(?=[^"]*\btracking-tight\b)`.

#### A4 · The specific motion

Framer-Motion fade-up-on-scroll with stagger — literal `initial={{ opacity: 0, y: 20 }}` / `whileInView={{ opacity: 1, y: 0 }}` with `transition: { staggerChildren: 0.1 }`. The 100ms stagger is the settled default ([CoderCops](https://www.codercops.com/blog/web-animation-gsap-framer-motion-css-2026)). Siblings: `animate-pulse` as universal loading; the shimmer skeleton (`translate-x-[-100%] → 100%` over `bg-gradient-to-r from-transparent via-white/20 to-transparent`); the number counter; typewriter text; `whileHover={{ scale: 1.02 }}` / `hover:scale-105` on every card.

**For keeper specifically these are landing-page tells, and the correct rule in a dense desktop app is stronger: motion must be caused by state, not by scroll position.** `whileInView` has no legitimate use in a column of chat rows.

#### A5 · The specific iconography

- **Lucide/Heroicons at stock weight.** Read from keeper's installed `lucide-react@1.23.0`, `dist/esm/defaultAttributes.mjs`: `width: 24, height: 24, viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", strokeWidth: 2, strokeLinecap: "round", strokeLinejoin: "round"`. **That 2px round-cap 24-grid stroke *is* the visual signature.**
- **The sparkle glyph on any AI affordance.** NN/g: "In the absence of a standardized way to visually capture the idea of 'mysterious AI-driven process,' it has become the de facto symbol for everything AI" ([NN/g, 19 Sep 2025](https://www.nngroup.com/articles/ai-sparkles-icon-problem/)); Google Design reports **more than 100 of its system icons include an AI Sparkle** and that "the AI Sparkle icon doesn't always convey granular meaning" ([Google Design](https://design.google/library/ai-sparkle-icon-research-pozos-schmidt)). The 2026 verdict that binds: **"When everything gets an AI sparkle, it becomes noise, not novelty."** Detect: `Sparkles`, `Sparkle`, `WandSparkles`, `Wand2`, `Stars`; `✨`/`❇` (U+2728, U+2747); any four-pointed-star SVG adjacent to a control with an AI verb.

#### A6 · The specific copy tells

- **`not just X — it's Y`** is the strongest structural marker: "Humans occasionally write sentences in this form. **LLMs write one per paragraph.** … It's the sound of a model reaching for emphasis it hasn't earned" ([Duey AI](https://www.duey.ai/post/em-dash-ai-writing)). Detect: `\b(not|isn't|it's not) just\b.{0,80}(—|--|,)\s*(it's|but)\b`.
- **Em-dash cadence.** Measured: "em dash usage in scientific abstracts more than doubled between 2021 and 2025, almost exactly tracking the rise of ChatGPT" ([Level Up Coding](https://levelup.gitconnected.com/why-ai-lovesem-dashes-and-why-almost-every-explanation-is-wrong-7eb0577919aa)) — but the same field warns surface tells are weak evidence in isolation. **The enforceable version that sidesteps the detection debate: cap em dashes per UI string at zero. Product microcopy should be short enough not to need one.**
- **Marketing register** — `Elevate your…`, `Seamlessly…`, `Supercharge`, `Unleash`, `Next-Gen`, `in seconds`, `effortlessly`; plus the verb set "delves, underscores, showcasing, pivotal, intricate, meticulously, realm, aligns, garnered" ([field guide](https://matthewvollmer.substack.com/p/i-asked-the-machine-to-tell-on-itself)). Already banned in shipped design systems ([ogblocks](https://ogblocks.dev/blog/framer-motion-text-animation)).
- Tricolon headings ("Fast. Private. Yours.") — `[INFERENCE]`, no hard corpus cite found.

#### A7 · Layout tells

`max-w-7xl mx-auto` as the only container; `text-center` on every section header; `min-h-screen` hero; identical `py-20`/`py-24` on every section; dark mode as `dark:bg-gray-900` with no re-thought contrast or elevation.

**Density is the real defence, and keeper already has it.** Verified by grep across `src/**/*.tsx`: **zero** occurrences of `max-w-7xl`, `rounded-2xl`, `rounded-3xl`, `text-5xl`, `text-6xl` or `tracking-tight`. A genuine, measured clean bill on the layout tells.

#### A8 · shadcn/ui defaults — the actionable part for keeper

**What un-customised shadcn looks like, from source.** The default scaffold ([ui.shadcn.com/docs/theming](https://ui.shadcn.com/docs/theming)) is `--radius: 0.625rem` (**exactly 10px**) and a `neutral` base whose OKLCH greys are chromaless (`0 0` on every channel), landing exactly on Tailwind's `neutral` ramp (converted):

| shadcn token | OKLCH | hex | = Tailwind |
|---|---|---|---|
| `--foreground` | `0.145 0 0` | `#0a0a0a` | `neutral-950` |
| `--primary` (light) | `0.205 0 0` | `#171717` | `neutral-900` |
| `--muted-foreground` | `0.556 0 0` | `#737373` | `neutral-500` |
| `--ring` | `0.708 0 0` | `#a1a1a1` | `neutral-400` |
| `--border` / `--input` | `0.922 0 0` | `#e5e5e5` | `neutral-200` |
| `--muted`/`--accent`/`--secondary` | `0.97 0 0` | `#f5f5f5` | `neutral-100` |
| `--sidebar` | `0.985 0 0` | `#fafafa` | `neutral-50` |

**So "un-customised shadcn" is mechanically checkable as `--radius: 0.625rem` (or `10px`) plus any `oklch(… 0 0)` grey matching that table.** One grep, highest signal in this document.

Component giveaways, verbatim from the registry: `Card` = `"flex flex-col gap-6 rounded-xl border bg-card py-6 text-card-foreground shadow-sm"` with the `CardHeader`/`CardTitle`/`CardDescription` triple ([card.tsx](https://github.com/shadcn-ui/ui/blob/main/apps/v4/registry/new-york-v4/ui/card.tsx)); `Button` default `"h-9 px-4 py-2"` with `focus-visible:ring-[3px]` ([button.tsx](https://github.com/shadcn-ui/ui/blob/main/apps/v4/registry/new-york-v4/ui/button.tsx)) — **the 3px focus ring is the single most distinctive un-customised shadcn artifact; nothing else in the ecosystem uses that value.** And **two purples hide in the default dark theme**: `--chart-4: oklch(0.627 0.265 303.9)` = `#ad46ff` and `--sidebar-primary: oklch(0.488 0.243 264.376)` = `#1447e6`. Anyone who bans purple but ships stock shadcn dark still has purple in the file.

**keeper's audit** (`components.json` → `style: "radix-vega"`, `baseColor: "neutral"`, `iconLibrary: "lucide"`):

| Finding | Evidence | Verdict |
|---|---|---|
| `--font-sans: -apple-system, …` | `index.css:9` | ✅ **Not** the Inter tell. A deliberate platform-native choice. |
| `--primary: #0f6e5c` / `#3ecfae` | `index.css:73`, `:153` | ✅ Custom greens; neither matches any Tailwind default. Already off-distribution. |
| `--incognito: #6d28d9` | `index.css:92` | ❌ **Live violation.** Tailwind `violet-700` verbatim. |
| `--sidebar-primary: oklch(0.488 0.243 264.376)` | `index.css:190` | ❌ **Un-overridden shadcn default** = `#1447e6`. Nobody picked it. |
| `--radius: 10px` | `index.css:109` | ⚠️ Numerically identical to shadcn's `0.625rem`. `--radius-sm/md` *were* hand-set to 5px/7px (`index.css:57-58`), so intent exists — but the base is stock. |
| Semantic hexes are raw Tailwind — `#b45309` amber-700, `#dc2626` red-600, `#16a34a` green-600, `#d97706` amber-600, `#fde68a` amber-200 | `index.css:90-98` | ⚠️ Palette-by-default. Not the purple tell, but not chosen either. |
| Greys `oklch(0.145/0.205/0.556/0.708/0.922/0.97/0.985 0 0)` | `:root` + `.dark` | ⚠️ Verbatim shadcn neutral. |
| `backdrop-blur` usage | `App.tsx:199`, `ui/dialog.tsx:33`, `ui/sheet.tsx:31` | ✅ All three are **modal scrims**, not glass cards. Legitimate. |
| Layout tells | grep across `src/**/*.tsx` | ✅ **Zero occurrences.** |

**Ecosystem note that matters for the new identity:** shadcn's base-colour set is no longer just cool greys — it now offers **Neutral, Stone, Zinc, Mauve, Olive, Mist, Taupe**, and Tailwind v4.3 ships `taupe`, `mauve`, `mist`, `olive` in the default palette ([theming docs](https://ui.shadcn.com/docs/theming); [tailwindcss.com/docs/colors](https://tailwindcss.com/docs/colors)). Tinted greys are now the *default*, which means a chromaless `oklch(… 0 0)` grey is simultaneously yesterday's shadcn default **and** about to read as un-updated. For a green-forward identity, deriving the greys from the brand hue is both off-default and cheap.

### Q5B — Currency audit of the five existing bans

**B1 · No purple/violet — KEEP, still the highest-signal tell, but sharpen it.** The causal chain is live: Wathan's apology is from Aug 2025, and Anthropic's *current* cookbook still lists "Clichéd color schemes (particularly purple gradients on white backgrounds)". Models trained after 2025 are training on more indigo, not less.

**But "purple is bad" is false.** Linear — the reference-class dense keyboard-first app — ships `#5E6AD2`, a lavender-indigo. Measured: `curl https://linear.app/` returns exactly **one** occurrence of `#5E6AD2` in 1.27 MB of markup. One chromatic accent, used once. That is the opposite of slop. **The correct 2026 formulation is not "no purple" but "no *default* purple"** — ban the ramp values, not the hue family.

**And a warning that lands directly on this brief: green is on track to be 2027's purple.** WGSN named **"Transformative Teal"** Colour of the Year 2026 — "a fluid fusion between dependable dark blue and aquatic green" ([WGSN](https://www.wgsn.com/en/blog/colour-year-2026-transformative-teal)). Anthropic's own cookbook example theme is **Solarpunk: "Warm, optimistic color palettes (greens, golds, earth tones)"** — green is already the model's *first suggested escape* from purple. keeper going greenish is fine, but **the ban list must add a scarcity rule, not just swap the hue. A greenish UI with green everywhere is the same failure at a different wavelength.**

**B2 · No gradient meshes — KEEP, unchanged; this one has not rotated out at all.** The evidence points the wrong way for anyone hoping it aged out: aurora is "a staple of 2026 web design, seen on top-tier SaaS landing pages **and AI dashboards**" ([SyntaxSnap](https://syntaxsnap.com/tools/aurora-gradient)). It is *more* associated with AI products now than in 2023. Strengthen it to cover the animated blob and the stacked-`radial-gradient()` fake mesh. Separately: keeper is a local-first desktop app, and a GPU-compositing animated background running behind six live columns **and an active screen recording** is a battery and thermal cost with zero informational payload. The ban is justified on engineering grounds alone.

**B3 · No glass cards — REFRAME. The meaning of this ban changed in June 2025.** Apple shipped *actual, system-level* glassmorphism: Liquid Glass, "a new material… translucent and behaves like glass in the real world," across iOS/iPadOS/**macOS Tahoe**/watchOS/tvOS 26 ([Apple Newsroom, 9 Jun 2025](https://www.apple.com/newsroom/2025/06/apple-introduces-a-delightful-and-elegant-new-software-design/)), and asks third parties to adopt it ([Liquid Glass overview](https://developer.apple.com/documentation/technologyoverviews/liquid-glass)). **keeper is a macOS app, so on its own platform translucent material is now the vendor's convention for sidebars and toolbars — not a slop tell.** A blanket ban is therefore a deliberate refusal of a platform convention, and the doc should say so out loud rather than pretend it is neutral.

**Refuse it anyway — and here is what makes the refusal defensible rather than contrarian: Apple has been walking it back under legibility pressure.** Developers filed that "the Liquid Glass UI introduces excessive transparency and blur across the system. This significantly reduces text readability, lowers contrast, and causes **visual fatigue during prolonged use**" ([Apple Developer Forums 811219](https://developer.apple.com/forums/thread/811219)); beta 2 toned the effect down ([GSMArena](https://m.gsmarena.com/ios_26_beta_2_tones_down_the_liquid_glass_effect-amp-68379.php)); by iOS 26.1 beta 4 Apple added a **"Tinted"** appearance, and the placement is the tell — it "lives inside appearance controls — not buried under accessibility — implying Apple sees this as a mainstream preference" ([Gulf News](https://gulfnews.com/technology/companies/apple-yields-tinted-control-in-ios-261-beta-4-tones-down-liquid-glass-after-backlash-1.500315176)).

> **Split the ban into three.** (1) *Content* glass — `bg-white/10 backdrop-blur-lg border-white/20` on a card holding text — banned permanently; "visual fatigue during prolonged use" is disqualifying for an app someone lives in all day. (2) *Chrome* glass — a translucent titlebar/sidebar via `NSVisualEffectView` — a platform-convention question whose answer should be a stated "no, because six dense columns of text need a stable opaque substrate," citing Apple's own retreat as precedent. (3) Modal **scrims** exempt outright, which is what keeper already does.

**B4 · No hero-plus-three-cards — KEEP, but restate as a *card-grid* ban.** The 3-up row was absorbed into the bento grid, which is now the default for "6 to 9 parallel facts". Banning the 3-up while leaving bento open is a loophole — a 4-2-3 bento is the same instinct with fancier `col-span`s. Reject any grid of ≥3 uniform `rounded-*` `border` `shadow-sm` cards used to enumerate features. For keeper's product surfaces this is mostly moot (zero `rounded-2xl`, zero `max-w-7xl`); **where it bites is Settings, empty states, and the eventual website** — and Settings is exactly where a shadcn `Card`/`CardHeader`/`CardTitle`/`CardDescription` stack appears without anyone deciding to put it there.

**B5 · No emoji bullets — KEEP, but scope it and extend it.** Two things changed. (1) **The successor tell is the sparkle, and it is worse** — banning `🚀` while shipping `<Sparkles />` on the bot button is fighting the last war. (2) **The ban must be scoped, not global:** keeper legitimately ships an emoji shortcode table (`src/lib/emoji/table.ts`) and user-selectable space icons (`src/components/notes/space-icons.ts`). Emoji *as user content and user-chosen labels* is a feature; emoji *as chrome authored by keeper* is the ban. The current one-line prohibition doesn't make that distinction and would read as a contradiction to the next maintainer.

**B-extra · Are brutalism and Swiss-minimal now AI defaults too?** **Swiss minimal: effectively yes.** Vercel's Geist — the default face of the Next.js/v0/shadcn stack — describes itself as drawing on "the renowned Swiss design movement". When the default toolchain's own typeface is Swiss by charter, "clean grid, neutral grotesque, lots of whitespace" is the distribution mean wearing a nicer suit. **Neo-brutalism: not yet, but it is being commodified in real time** — the anti-AI texture movement now has a trend name, "tactile brutalism" ([Fireart](https://fireart.studio/blog/the-best-web-design-trends/)), which is how trends die. **Don't ban either, don't adopt either.** Neo-brutalism in particular is actively wrong for an all-day dense app: hard offset shadows and high-chroma flats burn contrast budget on decoration that eight hours of reading will make hostile. Take the one mechanism that survives — **honest, un-softened surfaces: hairlines instead of shadows, no faux depth** — and leave the aesthetic.

### Q5C — The counter-move: what actually reads as hand-made

**C1 · A committed, non-default typeface — ideally one you had to do work to get.** iA Writer is the reference case: three custom variable faces (Mono, Duo, Quattro) built on IBM Plex and then deliberately de-branded — "we have eliminated the IBM brand-typical features… Using square dots instead of round ones, adjusting the swirls and curves on a, j, f, l, t, y, Q" — and open-sourced at [github.com/iaolo/iA-Fonts](https://github.com/iaolo/iA-Fonts) ([iA](https://ia.net/topics/a-typographic-christmas)). The mechanism is that *the text-setting decision is the product decision*, made at the glyph level. Anthropic's pairing rule is the useful one: "High contrast = interesting. Display + monospace, serif + geometric sans… Use extremes: 100/200 weight vs 800/900, not 400 vs 600."

**C2 · A genuinely restricted palette with a scarcity rule — measured, not asserted.** Linear is the proof: one chromatic accent appearing exactly once in the shipped markup. Anthropic states the same from the generation side: "**Dominant colors with sharp accents outperform timid, evenly-distributed palettes.**"

**C3 · Hierarchy from surface lift and hairlines, not shadow.** Linear "trusts surface lift and hairline borders to carry every bit of hierarchy" and — the important part for a dark-first dense app — "This is not a dark theme applied to a light design… information density is managed through subtle gradations of white opacity rather than color variation" ([Open Design](https://opendesigner.io/design-systems/linear-app)). That is the direct antidote to the "dark mode as inverted grey" tell. **keeper already does this correctly** — `--border: oklch(1 0 0 / 10%)`, `--input: … / 15%` in `.dark` (`index.css:162-163`).

**C4 · Non-default spacing rhythm and real density.** The generic rhythm is `p-6`/`gap-6`/`py-20`; a dense app's rhythm is set by the row. keeper already has non-arbitrary numbers — `--phone-header: 52px`, `--radius-sm: 5px`, `--radius-md: 7px` (hand-set, *not* shadcn's `calc(var(--radius) * 0.6)` derivation). Those 5/7px radii are exactly the kind of decision a default cannot produce. The gap is `--radius: 10px` still sitting at stock.

**C5 · Custom iconography, or at minimum a non-default stroke.** The cheapest real win: Lucide's `strokeWidth: 2` on a 24-grid is the signature. Setting a house stroke (1.5px) and a house size grid (16px for row-level affordances in a dense column, not 24px) de-signatures the entire icon set in one place — `lucide-react@1.23.0` ships `LucideProvider` accepting `size`, `strokeWidth`, `absoluteStrokeWidth` specifically for this. Beyond that, the glyphs that carry meaning unique to keeper — bridge health, held, incognito, recording, sync state — should be **drawn, not borrowed**. The robot-head mark is the correct instinct applied at brand level; apply it to the six or seven glyphs that are keeper's own vocabulary.

**C6 · Texture/grain — real mechanism, real number, heavy restraint.** Specified precisely enough to implement: an SVG `feTurbulence` filter at **15–30% opacity** ([Gezar](https://gezar.dk/en/blog/web-design-trends-2026)). **But temper it for keeper:** grain behind six columns of running text is a legibility tax, and it is now a named trend on the same clock as everything else. If used at all: empty states, onboarding, the tray/menubar surface, the marketing site — **never behind a text column.** `[INFERENCE]` on that restriction; the sources address hero surfaces, not multi-column text density.

**C7 · Asymmetry — with an honest caveat.** `[INFERENCE]`: no shipped desktop-app case study isolates asymmetry as *the* mechanism, so treat this as weaker than C1–C5. The concrete version for keeper needs no citation because it is structural, not decorative: **six columns of deliberately unequal width, sized to their content's real information density** — a chat list, a file tree and a note body do not want the same measure — with the split points as named tokens. Uniform columns are the tell; content-derived columns are the answer.

**C8 · Microcopy in a specific human voice.** NN/g's 2026 forecast: "as AI fatigue increases, **authentic, human details will set experiences apart**." Concretely for keeper: name states in the app's own domain language — "held", "incognito", "bridge degraded" are all already tokens in `index.css`, so the vocabulary exists and just needs to reach the surface. State what happened and what happens next. No adjectives, no em dashes, no "seamlessly". A local-first tool's voice is factual and slightly terse: it is talking to someone who knows how git works.

---

## What this means for keeper

Twelve decisions, in the order a `DESIGN.md` would need them. Each names the evidence it rests on. Where keeper is already right, the recommendation is to *write it down and enforce it*, not to redesign it.

### 1. keeper already solved the green collision by accident. Make it deliberate and defend it.

The single most important finding in this research is that keeper's existing palette **already implements the mechanism Q2C recommends**. Measured from `src/index.css`:

| Token | Hex | OKLCH hue | Role |
|---|---|---|---|
| `--primary` (light) `:73` | `#0f6e5c` | **175.5°** | brand |
| `--primary` (dark) `:153` | `#3ecfae` | **174.0°** | brand |
| `--bridge-healthy` `:95,175` | `#16a34a` | **149.2°** | status |

Computed separation, brand vs status: **ΔE 44.8 normal / 37.8 deuteranopia / 41.3 protanopia** (light) and **36.0 / 36.1 / 35.8** (dark). Compare Atlassian's celebrated lime move at ΔE 36.7 / 37.1 / 33.3, and Spotify's failure at 19.4 / 12.5. **keeper's separation is better than Atlassian's and holds flat under both dichromacies**, because a teal-jade brand opens the S-cone axis that protans and deutans retain, where lime rides the damaged red–green axis.

**Action:** write the rule into `DESIGN.md` as a hue budget, not a colour list — *brand occupies 170–178°; status green occupies 145–152°; nothing may be authored between them* — and make it machine-checkable the way tgsite's `check-design.mjs` polices the red budget. This is the rule that stops the next contributor "improving" `--primary` toward `#22c55e` and silently destroying the separation.

### 2. Delete the third green. It is the only real collision keeper has.

`settings/device-verification-dialog.tsx:115` and `settings/key-backup-dialog.tsx:257` both use raw `text-emerald-600 dark:text-emerald-400`, bypassing the token layer. Computed: `#059669` vs `--primary` `#0f6e5c` is **ΔE 23.0 normal, 17.5 deuteranopia** — inside the "same green, slightly different" band that makes Spotify's palette unreadable as information. Two greens with a stated separation is a system; three greens where one arrived by accident is not.

**Action:** route both through `--bridge-healthy` (or a renamed `--ok`) and add a lint that rejects `emerald|green|teal|lime-\d{3}` utility classes in `src/`, exactly as tgsite's checker rejects raw hex outside `index.css`.

### 3. The bridge status triad has a measured accessibility defect. Fix it with a glyph, and know why lightness cannot save you.

`--bridge-healthy #16a34a` / `--bridge-degraded #d97706` / `--bridge-disconnected #dc2626` render as three bare coloured dots — `bridge-card.tsx:68-70`, `sidebar-pane.tsx:88-90`, `lib/bridges.ts:72-74`. Computed:

| Pair | luminance ratio | ΔE deuteranopia | ΔE protanopia |
|---|---|---|---|
| healthy vs degraded | **1.03:1** | 33.3 | **16.3** |
| degraded vs disconnected | **1.52:1** | **14.3** | 35.2 |
| healthy vs disconnected | **1.47:1** | — | — |

All three pairs fall far below the 3:1 luminance delta that [SC 1.4.1](https://www.w3.org/WAI/WCAG22/Understanding/use-of-color.html) accepts as a redundant channel, so **hue is the only channel carrying the information** — and each pair collapses toward ΔE ~15 for one of the two commonest dichromacies. For roughly 1 in 12 men these are three similar dots.

**And the lightness fix is structurally unavailable, which is worth stating in the doc so nobody attempts it.** I tested a deliberate lightness ladder (`#1a7f37` / `#b45309` / `#b91c1c`) chosen to keep all three at AA on white: the best achievable mutual ratios were **1.01, 1.29 and 1.27** — because forcing three colours to clear 4.5:1 against one background *confines them to a narrow luminance band by definition*. You cannot have three AA-compliant status colours on one surface **and** a 3:1 lightness ladder between them.

**Action:** the redundant channel must be **shape**, not lightness — which is exactly what Adobe Spectrum mandates ("you must also display text or an icon") and what VS Code implements with `A`/`M`/`D` badges. Adopt filled / half / hollow dot, or dot + glyph. This also happens to be the cheaper fix, since it costs no palette.

### 4. Adopt Spectrum's prohibition on coloured status text — keeper is already violating it.

`bbctl-run-sheet.tsx:154` and `bridge-login-sheet.tsx:366` render `cn("font-medium text-lg", "text-bridge-healthy")` — "Running ✓" and "Linked ✓". Computed: `#16a34a` on the light `--background` (`oklch(1 0 0)` = `#ffffff`) is **3.30:1**. Tailwind `text-lg` is 18px and `font-medium` is 500, so WCAG's large-text exemption does not apply (that needs 18pt = 24px, or 14pt bold = 18.67px bold). **This is a plain SC 1.4.3 AA failure in shipped code.** It passes in dark (6.01:1) — the classic single-hex-two-themes trap from Q1a/Warp.

**Action:** adopt Spectrum's rule verbatim into `DESIGN.md` — *green may be a background, border, dot or icon; never a text colour* — which fixes both sites without needing a new token, and prevents the class of bug rather than the instance.

### 5. Keep the light accent. Replace the dark accent — it is too bright by design, not by taste.

`--primary` light `#0f6e5c` is **L\* 41.3, 6.17:1 on white, 5.52:1 on warm paper** — comfortably inside the Q3C light band of L\* ≤ 46.8. Keep it.

`--primary` dark `#3ecfae` is **L\* 75.3**, scoring **10.13:1** on keeper's dark background against a 4.5:1 requirement. That overshoot is bought entirely in glare and halation (Q3D), and Material states the rule directly: "avoid using saturated colors… Saturated colors also produce optical vibrations against a dark background, which can induce eye strain."

**Proposed replacement, derived and verified rather than picked:** `oklch(0.70 0.11 175)` = **`#42b59a`**, L\* 66.9 — inside the Q3C dark band of L\* 55–70, at keeper's existing brand hue (174.4°, so no re-branding).

| Check | Result |
|---|---|
| on `.dark --background` `#0a0a0a` | **7.83:1** ✅ |
| on `.dark --card` `#171717` | **7.09:1** ✅ (clears Apple's 7:1 "strive for" target) |
| `--primary-foreground` `#06231c` on it | **6.57:1** ✅ |
| ΔE vs status green `#16a34a` | 36.5 normal / 34.2 deut / 34.6 prot — separation preserved |

### 6. Dark is the hero, light is first-class, and the reason is stated — not assumed.

Q3A is unambiguous that light wins on measured reading performance, that the advantage **grows as text gets smaller**, and that users cannot feel the difference. keeper has 376 `text-xs` sites. So light must be a genuinely good theme, not an afterthought — but dark is the right hero for an all-day tool at a desk, and `DESIGN.md` should say *that this is a comfort decision made against the acuity evidence*, the way tgsite names dark "hero mode" and light "daylight paper" without pretending either is free.

**Three consequences to write down:** (a) every accent needs two hexes — proven, not asserted: the L\* bands for 4.5:1 on white and on near-black have an **empty intersection**; (b) elevation needs two strategies — shadow in light, surface-lightness and hairlines in dark (keeper already does the dark half correctly at `index.css:162-163`); (c) shipping a manual override deviates from Apple's *"Avoid offering an app-specific appearance setting"* — defensible for a pro tool, but log it as a decision.

### 7. Give keeper a voice. Right now it has none, and this is the largest cheap win.

`--font-heading: var(--font-sans)` (`index.css:8`) means 21 `font-heading` usages render identically to body. `--font-mono` is **never defined**, so the 26 files using `font-mono` fall through to Tailwind's stock stack. The one surface where keeper is unmistakably a machine tool — sync paths, git status lines, elapsed/segment/size in the recording banner, CSV cells, the Notes editor — is set in whatever the browser picks.

Q1b's Craft finding is the template: **four faces from one family, all system, zero payload.** keeper is macOS-only, so SF Pro Text / SF Mono / New York are free and already correct as a platform choice. But the *distinguishing* move is the mono, because that is where keeper's character actually lives.

**Action:** define `--font-mono` explicitly and commit to one non-default mono for machine surfaces. This is the single change that would most move keeper from "shadcn app" to "someone's tool", and it costs one token plus a font decision.

### 8. Spend the identity in the titlebar band you already own.

`tauri.conf.json:27-28` sets `titleBarStyle: "Overlay"` with `hiddenTitle: true`, and `app-shell.tsx:250` draws a 28px drag band. **Ghostty's issue tail is the price list for this decision, and keeper has already paid it** — lost drag regions, lost native tabs, fullscreen control collisions. Having paid, the band should carry identity rather than sit empty: the mark at 16px, the app's own status vocabulary, and the accent bleeding into the bar the way Ghostty's `transparent` titlebar does.

**Do not go further.** Six columns do not need reimplemented window chrome to prove they have a personality.

### 9. The mark: pick Direction D, and know that Android's trademark is why.

The geometry verdict is settled by measurement — **outer contour = one filled shape, identity = the holes** (Perplexity 45% mush and Cursor 42% survive; Ollama at 89%, the sparkle at 83% and the Claude asterisk at 82% do not). But the deciding constraint for *this* brief is legal, and it came out of Q2B: Android's brand page states *"You may not file trademark applications for or claim trademark rights to the Android robot logo or any derivatives thereof."* Android's bugdroid is **a dome head with two thin antennae and two dot eyes, in green `#3DDC84`**. A green robot head with antennae and two eyes is not adjacent to that territory; it is inside it.

That eliminates **Direction B** (antenna + two eyes) and puts **Direction C** (two antenna studs + two eyes) on notice. **Direction D — "Cyclops"** — is a capsule head with **one 4×4 square aperture and no antennae**: structurally distinct from the bugdroid, and it happens to carry keeper's meaning better than the alternatives. The single square aperture reads as a **lens/shutter** (Recordings) and as a **keyhole** ("keep"). It also has the most robust metrics of the four — 148px ink, min ink run **4px**, one hole, zero antialiasing by construction.

**And the mark is not green.** In two of its three destinations — menu-bar template and tinted dock variant — colour does not exist. Green lives in the Icon Composer background layer and in the UI. Author at 16×16 by filling cells, upscale by integers to the 1024 master.

### 10. Redraw the ten tray icons in the same pass, on the grid.

All ten `tray-*-template.png` honour the black-and-clear contract correctly, but **72–75% of their inked pixels sit on partial alpha** (`tray-idle`: 166 opaque against 506 antialiased). That is the signature of scaling vector strokes down to 44px instead of authoring on grid, and at the 24pt menu bar it renders soft. Teenage Engineering's rule applies exactly: **design the mark and the glyph set in the same pass, in the same geometry**, or the head will look pasted on beside them.

### 11. Extend the ban list — five additions, one reframe, one scope fix.

Carry tgsite's five bans across, then:

- **Reframe the glass ban into three rules** (content glass banned permanently; chrome glass refused *with Apple's own Liquid Glass retreat cited as precedent*; modal scrims exempt — which is what keeper already ships). Say out loud that this is a deliberate refusal of a macOS 26 platform convention, because on keeper's own platform it now is one.
- **Scope the emoji ban** to keeper-authored chrome. The shortcode table and user-selectable space icons are features.
- **Ban the sparkle now, before the bots land.** `Sparkles`, `Sparkle`, `WandSparkles`, `Wand2`, `Stars`, `✨` (U+2728), `❇` (U+2747) on any AI affordance. Quote NN/g: *"When everything gets an AI sparkle, it becomes noise, not novelty."* **The robot head is the AI mark; a sparkle beside it would be admitting the mark doesn't work.**
- **Ban the *default* purple, not the hue** — the seven Tailwind ramp hexes. Then fix the two live instances: `--incognito: #6d28d9` (`index.css:92`) is `violet-700` verbatim, and `--sidebar-primary: oklch(0.488 0.243 264.376)` (`index.css:190`) is an un-overridden shadcn default nobody picked.
- **Add a green scarcity rule**, because the field is rotating toward green and keeper is rotating with it. Green is permitted on: focus rings, the single primary action in any view, selected-state indication, link/wikilink text, and the mark. Everything else is grey step and hairline. Enforce by counting `--primary` spends per view, the way tgsite counts its red budget.
- **Add a motion rule** a landing-page tell cannot satisfy: *motion is caused by state, never by scroll position.* `whileInView` has no legitimate use in a column of chat rows.
- **De-default the shadcn fingerprint** — the one-line grep that identifies un-customised shadcn is `--radius: 0.625rem`/`10px` plus chromaless `oklch(… 0 0)` greys. keeper has both. Pick a radius for a reason, derive the neutral ramp from the brand hue (which is also off-default now that Tailwind v4.3 ships tinted greys), and replace the stock `focus-visible:ring-[3px]`.
- **Set a house icon stroke.** `lucide-react@1.23.0` ships `LucideProvider` taking `size` and `strokeWidth`; 1.5px on a 16px grid de-signatures the whole set in one place. Draw the six or seven glyphs that are keeper's own vocabulary — bridge health, held, incognito, recording, sync state.

### 12. Where "impressive" actually comes from — and where it does not.

The brief asks for impressive *and* usable, and Q1 answers where the tension resolves. **Arc is the counter-example with numbers**: per-context gradient chrome, and 5.52% of DAUs ever used more than one Space before the product was retired to maintenance. Six greenish surfaces each repainting the chrome is that same idea. Don't.

The mechanisms that actually read as *made by someone* in a dense professional tool, all evidenced above and all cheap here:

- **A committed mono** where the machine speaks — the biggest single win available to keeper (#7).
- **Roughly 8 neutrals to 1 accent** (Reflect, measured), with the accent spent under a written scarcity rule (Linear: one hex, one occurrence).
- **Hairlines and surface lift instead of shadow** — keeper already does the dark half correctly.
- **Published density numbers** — Tana's 4px row padding and 20px indent per level, Obsidian's `--size-4-N` grid — plus Things' concession: ship **one density/scale control** rather than guessing the right row height for someone on a 30-inch display.
- **Content-derived column widths as named tokens.** A chat list, a file tree and a note body do not want the same measure. Uniform columns are the tell.
- **A published latency budget** (Superhuman's <50 ms) that keeper defends in code and cuts features to meet.
- **Microcopy in keeper's own vocabulary** — "held", "incognito", "bridge degraded" are already tokens; they just need to reach the surface. Factual and terse; the reader knows how git works.

None of that costs a row of density, and none of it is a thing a model produces by default. That is the whole point.
