# Writing Skins & Themes

The viewer's UI is skinned with **real CSS**, powered by
[`bevy_flair`](https://github.com/eckz/bevy_flair). A skin is a set of named
**design tokens** — colours, textures and fonts — that every panel and widget
reads by name, so you can restyle the whole viewer without touching a line of
Rust and without moving a single control.

This chapter is the skin author's reference. It assumes you can read CSS; it
concentrates on **what is different here** from plain CSS, `bevy_ui` and stock
`bevy_flair`, because those differences are where a skin goes wrong.

> **The one rule that governs everything:** a skin changes *colour, texture and
> font*, never *layout*. Layout is code (the widgets), authored once and
> bidi-correct. This mirrors the reference viewer, whose skins are almost
> entirely a `colors.xml` of named colours — and it is deliberate: whole-file
> layout replacement is the reason a reference skin forks a 3,500-line
> `floater_tools.xml` and then breaks on every release.

## Where skins live

```text
sl-client-bevy-viewer/assets/skins/
├── common.css              # structural rules: class → token. Shared by ALL skins.
├── graphite/
│   ├── skin.css            # the Graphite skin: token VALUES only
│   └── themes/
│       └── dark.css        # a theme OVERLAY on Graphite
└── azure/
    └── skin.css            # the Azure skin: the same tokens, different values
```

- **`common.css`** maps widget classes (`.sk-button`, `.sk-card`, …) onto
  tokens (`var(--control-bg)`, …). It is **shared** and contains *no colour
  literals*. You rarely touch it — only when a new widget class is introduced.
- **`<skin>/skin.css`** assigns concrete values to the tokens. This is the file
  you write for a new skin.
- **`<skin>/themes/<theme>.css`** is an *overlay*: it re-imports the base skin
  and redefines a **subset** of the tokens (a "dark" variant, a "high-contrast"
  variant, …).

## Running and selecting a skin

The viewer binary takes three flags:

| Flag | Meaning |
| --- | --- |
| `--skin <name>` | the skin directory to wear (`graphite`, `azure`) |
| `--theme <name>` | a theme overlay under that skin (e.g. `dark`); omit for the base |
| `--watch-skins` | **hot-reload**: re-apply the `.css` live as you edit it |

```sh
# Author a skin with live reload:
cargo run --release --bin sl-client-bevy-viewer -- --skin graphite --watch-skins
```

The **UI gallery** is the fastest way to iterate — it renders every widget on
one screen, watches the skin files automatically, and has a
**skin/theme switcher** at the top:

```sh
cargo run --release --bin sl-client-bevy-viewer-gallery
```

Edit a `.css`, save, and the running viewer/gallery restyles instantly — no
recompile.

## The token model

A skin is a block of **abstract role tokens** on `:root`, each with a direct
value:

```css
:root {
  --surface-bg: #1c1f26f2;
  --control-bg: #2a2f3a;
  --accent: #5cb8fa;
  /* … */
}
```

Two conventions that are **stricter than CSS habit**, on purpose:

1. **Every token is a *role* name, never a *colour* name.** Write `--accent`,
   `--control-bg`, `--gain` — never `--blue-500` or `--dark-grey`. There is no
   "palette tier" of literal-colour-named tokens. This is what lets a theme
   overlay (or a future culture / colour-blind profile) remap a *meaning*
   without knowing which physical colour it lands on.
2. **A widget references a role token, never an inline colour.** In `common.css`
   you write `background-color: var(--control-bg)`, not
   `background-color: #2a2f3a`. A meaning-bearing surface (a gain/loss delta, a
   status dot) must use a semantic token (`--gain`, `--loss`), never a literal —
   those are the tokens the localisation profiles remap.

### The role tokens we ship

Defined by every skin's `skin.css`, consumed by `common.css`:

| Token | Role |
| --- | --- |
| `--surface-bg` | a framed panel's background |
| `--surface-border` | a framed panel's border |
| `--surface-radius` | a framed panel's corner radius |
| `--card-bg` | a background-only surface (card, bar) |
| `--text-primary` | primary body text |
| `--text-muted` | secondary / instruction text |
| `--control-bg` | a button's resting background |
| `--control-bg-hover` | a button's hovered background |
| `--control-border` | a button's resting border |
| `--control-radius` | a button's corner radius |
| `--focus-ring` | the keyboard-focus ring |
| `--accent` | accent bars, highlights |
| `--gain` | **meaning-bearing**: a positive / up / gain value |
| `--loss` | **meaning-bearing**: a negative / down / loss value |

A skin **must** define every token it does not inherit; a `var()` that resolves
to nothing leaves the property unset (usually invisible).

### The widget classes

`common.css` defines these; a widget opts into skinning by carrying the class.

| Class | What it styles |
| --- | --- |
| `.sk-panel` | a framed surface (bg, border, radius, padding) |
| `.sk-card` | a background-only surface (bg + radius, no border/padding) |
| `.sk-title` | an instruction / secondary line |
| `.sk-text` | primary body text |
| `.sk-button` | a button, plus `:hover` and `:focus-visible` states |
| `.sk-accent` | a leading accent bar + hanging indent (logical box demo) |
| `.sk-tab` | a tab shape with asymmetric top corners (logical corner demo) |
| `.sk-gain` / `.sk-loss` | meaning-bearing colour swatches |

## Making a new skin

1. Copy an existing `skin.css` into `assets/skins/<yourskin>/skin.css`.
2. Keep `@import "skins/common.css";` at the top.
3. Change the token **values**. Do not add structural rules — those belong in
   `common.css` and are shared.
4. Register the skin id in `src/skin.rs` (`SKINS`) so the gallery switcher and
   the tests know about it.

That is the whole job: a second skin is a second set of *values*, never a second
layout.

## Making a theme overlay — and the cascade-layer rule

A theme redefines a **subset** of a skin's tokens. Everything it does not
mention falls through to the base skin — exactly the reference viewer's model,
where a `themes/dark/colors.xml` overrides a handful of the base's named
colours.

**This is the single most important gotcha in the whole system:** you must
import the base skin **into a cascade layer**, and leave your overrides
**un-layered**:

```css
/* skins/graphite/themes/dark.css */
@import "skins/graphite/skin.css" layer(skin); /* base → a layer */

:root {
  /* overrides → un-layered */
  --surface-bg: #0a0c10fa;
  --card-bg: #0e1218;
  --control-bg: #14181f;
  --accent: #7cc9ff;
}
```

Why the `layer(skin)` is not optional: with a *plain* `@import`, `bevy_flair`
orders a same-specificity conflict such that the
**imported base `:root` wins over your overriding `:root`** — so your theme
silently does nothing. In CSS, an **un-layered** rule beats **any** layered rule
regardless of source order (and `bevy_flair` implements this). Importing the
base into a layer therefore demotes it below your un-layered overrides, so the
overlay wins. See the
[CSS cascade layers reference][mdn-layers].

Register the `(skin, theme)` pair in `src/skin.rs` (`THEMES`).

## Custom CSS properties — logical box & corner properties

This is where the viewer's CSS deliberately **departs from stock `bevy_flair`**.

`bevy_flair`'s built-in box properties are *physical* (`margin-left`, `left`,
`border-top-left-radius`). Physical properties do **not** mirror under a
right-to-left locale and would fight the viewer's logical layout model. So the
viewer **registers a set of logical properties** and **bans the physical ones**.

**Use these logical properties** (they mirror automatically under RTL):

| Logical property | Replaces (banned) |
| --- | --- |
| `margin-inline-start`, `margin-inline-end` | `margin-left`, `margin-right` |
| `margin-block-start`, `margin-block-end` | `margin-top`, `margin-bottom`¹ |
| `padding-inline-start`, `padding-inline-end` | `padding-left`, `padding-right` |
| `padding-block-start`, `padding-block-end` | `padding-top`, `padding-bottom`¹ |
| `border-inline-start-width`, `border-inline-end-width` | `border-left-width`, `border-right-width` |
| `border-block-start-width`, `border-block-end-width` | `border-top-width`, `border-bottom-width`¹ |
| `inset-inline-start`, `inset-inline-end` | `left`, `right` |
| `inset-block-start`, `inset-block-end` | `top`, `bottom`¹ |
| `border-start-start-radius` | `border-top-left-radius` |
| `border-start-end-radius` | `border-top-right-radius` |
| `border-end-start-radius` | `border-bottom-left-radius` |
| `border-end-end-radius` | `border-bottom-right-radius` |

¹ The *block* axis (top/bottom) does not flip — there is no vertical writing
mode here — but the properties are named logically so the vocabulary is one
thing.

`inline-start` is the **leading** edge: the left under a left-to-right locale,
the **right** under a right-to-left one. Write `padding-inline-start: 24px` for
a hanging indent and it lands on the correct side in every locale, with no
separate skin.

**These physical properties are banned** and a shipped skin that uses one
**fails the build** (a test scans every skin):

```text
margin-left  margin-right  padding-left  padding-right
border-left-width  border-right-width  border-left-color  border-right-color
left  right  inset
border-top-left-radius  border-top-right-radius
border-bottom-left-radius  border-bottom-right-radius
```

**Symmetric shorthands are fine** when they carry a *single* value, because a
single value is the same on every side and cannot be handed the wrong way:
`padding: 12px`, `border-width: 1px`, `border-radius: 6px`. Avoid the
**asymmetric** shorthand forms (`padding: 4px 8px 4px 24px`) — those expand to
physical left/right and will not mirror; use the logical longhands instead.

## Other ways this differs from plain CSS / `bevy_flair`

- **Colours** use `bevy_flair`'s parser: hex (`#rgb`, `#rrggbb`, `#rrggbbaa`),
  `rgb(...)`, `oklch(...)`, named colours, and `var(...)`. Alpha via 8-digit hex
  (`#1c1f26f2`) works.
- **Pseudo-classes** supported: `:hover`, `:active`, `:focus`, `:focus-visible`.
  `:focus-visible` tracks the viewer's **keyboard (Tab) focus** — so a
  `:focus-visible` ring shows on Tab and hides on click, which is exactly what
  you want for a focus ring. There are **no pseudo-elements** (`::before`,
  etc.).
- **`var()` has no fallback value** — `var(--x, blue)` is not supported. Define
  every token.
- **Images are local bundled files only.** A texture token points at a file
  under the viewer's assets (`-bevy-image: url("skins/…/foo.png")`),
  **never a grid asset UUID** — grid textures (a texture-picker thumbnail, a
  texture display) are *content*, handled by the render pipeline, not the skin.
- **`@import` paths are asset-root-relative** — `@import "skins/common.css";`
  resolves from the assets root, not the importing file's directory. Do **not**
  use `../`. Nested imports (theme → skin → common) work.
- **`!important` is not supported** and is ignored.
- **`text-align`** takes the logical `start` / `end` (not `left` / `right`).

## Locale-aware skins

The active locale is bridged onto the UI root as CSS **attributes**, so a skin
or overlay can react to it with an attribute selector:

```css
:root[dir="rtl"] {
  /* … right-to-left tweaks … */
}
:root[lang="ja"] {
  /* … a Japanese-specific font or accent … */
}
```

- `dir` is `ltr` / `rtl`.
- `lang` is the language tag (`en`, `ja`, `ar`, …), or `und` when no locale
  plugin is loaded (e.g. in the gallery).

You normally do **not** need `[dir=…]` for layout — the logical properties
already mirror. Reach for it only for a genuinely locale-specific *token* value.
The culture-colour and colour-blind accessibility work will hook in the same
way, through future `[data-culture]` / `[data-vision]` attributes.

## Bidi / RTL testing

Because layout uses logical properties, an RTL locale mirrors the whole UI with
**no separate skin**. To check your skin mirrors correctly:

- In the gallery, press **`D`** to flip the layout direction, or start any
  binary with `SL_VIEWER_UI_DIRECTION=rtl`.
- Watch the `.sk-accent` bar and any asymmetric corners move to the trailing
  edge.

## Reference

- [`bevy_flair`][flair] — the CSS engine (selectors, `var()`, `@import`,
  animations, hot-reload).
- [MDN: CSS logical properties][mdn-logical] — the `*-inline-*` / `*-block-*`
  vocabulary.
- [MDN: cascade layers][mdn-layers] — why theme overlays need `layer()`.
- [MDN: CSS custom properties (`var()`)][mdn-vars] — the token mechanism.

[flair]: https://github.com/eckz/bevy_flair
[mdn-logical]: https://developer.mozilla.org/en-US/docs/Web/CSS/CSS_logical_properties_and_values
[mdn-layers]: https://developer.mozilla.org/en-US/docs/Web/CSS/@layer
[mdn-vars]: https://developer.mozilla.org/en-US/docs/Web/CSS/Using_CSS_custom_properties
