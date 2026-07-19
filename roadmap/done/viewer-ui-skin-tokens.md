---
id: viewer-ui-skin-tokens
title: Skin system — design tokens (bevy_flair CSS)
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-ui-framework
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

The skin / theming system, implemented with **`bevy_flair`** — real CSS
(selectors, pseudo-classes, `@keyframes`, `var()`, `@font-face`, hot-reloaded
`.css`), a better skin language than XUI and the natural home for the design
tokens. The reference's skinning is in practice a **design-token exercise**: its
6 skins × 21 themes are almost entirely `colors.xml` (447 named colours) + named
textures (939); **no theme overrides a layout**. So model skins as named colour
/ texture / font tokens with an id-keyed recursive merge for overlays, and
hot-reload the stylesheets at runtime.

For bidi we go beyond the reference: use CSS **logical properties**
(`inset-inline-start`, `margin-inline`, `padding-block`, `text-align: start`)
and a per-locale `direction`, so an RTL locale mirrors the layout with
**no separate skin**. No physical `left`/`right` in tokens or stylesheets.

**Do not copy** whole-file skin replacement (the reason a reference skin forks a
3,500-line `floater_tools.xml` and then breaks every release) — overrides are
token-level only.

Reference (Firestorm, read-only): `newview/skins/*/colors.xml` + textures,
`llui` colour/texture lookups. The [[viewer-ui-notification-host]] and every
panel consume these tokens.

## Done

New module `src/skin.rs` (`ViewerSkinPlugin`) + `assets/skins/`. Built on
**`bevy_flair` 0.8** (its own `bevy 0.19` line, so one Bevy build), added to
both the viewer (`lib.rs`) and the gallery (`gallery.rs`).

**Architecture — CSS engine + logical properties through the shipped resolver.**
`bevy_flair`'s built-in box properties are *physical* (`margin-left` →
`Node.margin.left`), which would neither mirror under RTL nor honour the widget
scaffold's `LogicalRect` model. So `bevy_flair` is used as the CSS engine
(selectors, `:hover`/`:focus-visible`, `var()` tokens, `@import`, hot-reload)
and the module *extends* it: it registers five flat reflectable components
(`SkinMargin`/`SkinPadding`/`SkinBorder`/`SkinInset`/`SkinRadius`,
`#[properties(auto_insert_remove)]`) and maps the **logical** CSS names onto
them (`margin-inline-start`, `padding-block-end`, `inset-inline-start`,
`border-inline-start-width`, `border-start-start-radius`, …).
`resolve_skin_boxes` then folds those into the physical `Node` against
`UiDirection`, reusing the scaffold's own `LogicalRect::resolve` mirror. Ordered
`.after(StyleSystems::ApplyComputedProperties).before(UiSystems::Layout)`. The
**physical** names are banned (`scan_banned_properties` + a build-time test over
every shipped skin, with a logical-replacement hint per property).

**Tokens are all-abstract role names, no literal-colour palette tier** (per the
scope discussion): a skin is a `:root { --role: value }` block with direct
values; a panel references a role, never an inline colour; a user variant is a
later `:root {}`. Meaning-bearing tokens (`--gain`, `--loss`) are present in the
same namespace, ready for the culture / colour-blind overlays — both now
unblocked.

**Overlay merge = a CSS cascade LAYER.** A theme imports its skin base
**into a layer** (`@import "skins/graphite/skin.css" layer(skin);`) then
redefines a token subset **un-layered** (`assets/skins/graphite/themes/dark.css`
overrides 5 of ~11), exactly the reference's id-keyed `colors.xml` merge. The
layer is load-bearing, not decoration: a *plain* `@import` places the imported
base's `:root` such that it **beats** the theme's own `:root` (verified live —
the dark overrides never took), because `bevy_flair` orders a same-specificity
conflict by placing imported blocks last. An un-layered rule beats any layered
one regardless of order (`bevy_flair` implements CSS layer precedence,
`builder.rs` sorts by `cmp_layers().then(specificity)`), so importing the base
into a layer and leaving the overrides un-layered makes the overlay win.
`@import` paths are asset-root-relative (`@import "skins/common.css";`) — Bevy
resolves those from the asset root, avoiding fragile `../`; nested imports
(theme → skin → common) work.

**Shipped:** two skins (`graphite`, `azure`) + one overlay (`graphite/dark`) +
`common.css` (structural class→token rules, shared, no colour literals, no
physical box props). Textures are **local bundled files only** (no grid UUIDs —
grid textures are content, not skin); fonts reuse the `UiFont` families.

**Selection + hot-reload as CLI (user-facing) flags:** `--skin`, `--theme`,
`--watch-skins` on the viewer binary (env `SL_VIEWER_SKIN`/`SL_VIEWER_THEME` are
debug-only). Watching flips `AssetPlugin.watch_for_changes_override`;
`bevy_flair` re-applies on `AssetEvent::Modified`. The gallery watches always
(it is the skin-authoring surface) and got a **top switcher control** (cycle
skin / cycle theme buttons + live label + skinned sample chips that recolour on
switch). The gallery's element cards + header bar carry the `sk-card` class so a
switch reskins the whole gallery, not just the chips. The sticky header needs
`GlobalZIndex(1)`: `bevy_ui` picking does **not** clip a scrolled node's
off-the-top content, so without it the page's scrolled-away content sits over
the header and swallows the switcher clicks whenever the page is not at the very
top.

**i18n-aware skins:** `sync_skin_attributes` bridges the active locale +
direction onto the `UiRoot` as CSS attributes (`dir="rtl"`, `lang="ja"`), so
overlays can select on them (`:root[lang="ja"]`, and the future
`[data-culture]`/`[data-vision]` profiles). Strings stay in Fluent;
theme-authored localized labels/numbers in CSS are the follow-up
[[viewer-ui-skin-l10n-functions]] (the loader leaves a preprocess seam).

**Scope deviations from the original brief (accepted in discussion / flagged):**

- Proof surface is the **gallery switcher + skinned chips** (per the mid-task
  request for a gallery control), *not* a migration of the `F5` demo panel —
  that panel stays a pure widget-scaffold demo. Real viewer panels adopt `.sk-*`
  classes in their own tasks; this task establishes the mechanism.
- Physical-property banning is enforced at **build time** (a test fails if a
  shipped skin uses one), not yet as a runtime rejection — user-authored skins
  are not loaded at runtime yet; the runtime validator lands with the user-skin
  / l10n follow-up (the scanner is written to be reused).
- Only the logical **longhands** are registered (`margin-inline-start`, …), not
  the `margin-inline` / `padding-block` shorthands — a later ergonomics add.

Tests (`cargo test -p sl-client-bevy-viewer --lib skin::`, 7): the
banned-property scanner, no shipped skin uses a banned property, corner
mirroring, a `SkinMargin` mirrors onto `Node` under RTL through the real
resolver, path resolution, and shipped-skin/theme file existence.

Follow-ups filed: [[viewer-ui-skin-l10n-functions]]. Unblocked:
[[viewer-i18n-cultural-color-meanings]],
[[viewer-i18n-colorblind-accessibility]].
