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

Either binary may also be run straight out of `target/release/`: it locates its
own `assets/` — beside the executable when installed, otherwise the crate it was
built from — so a bare run wears the same skin a `cargo run` does, and says so
in one `viewer assets` line at start-up. Set `BEVY_ASSET_ROOT` to the directory
*holding* `assets/` to point a run at a different tree.

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
| `--surface-bg` | a framed surface: a panel, a floater body, a tab, the menu bar and the toolbar strip, a toast card |
| `--surface-border` | that surface's frame, and a menu's separator rule |
| `--surface-radius` | a framed surface's corner radius |
| `--card-bg` | a background-only surface inside a framed one: a card, a bar, a tab page, the active tab |
| `--overlay-bg` | the scrim a floating layer sits on (the dock host behind docked floaters, an overlay over the world) |
| `--overlay-text` | text drawn straight over the rendered world — a beacon's label, a diagnostic read-out — which is read against whatever the camera sees, not against a panel |
| `--menu-bg` | a dropped-down menu's face; a role of its own because a classic skin floats its menus in a colour the floater body does not share (the flat skins give it `--surface-bg`'s value) |
| `--text-primary` | primary body text: a label, a table cell, a menu entry, a tab caption |
| `--text-muted` | secondary text: a caption, a hint, a column header, a resize grip |
| `--text-disabled` | text of a control whose action does not apply right now |
| `--text-heading` | a heading inside a page |
| `--text-error` / `--text-warn` / `--text-note` | text that reports a failure, a warning, or a note asking for the eye — roles of their own so a colour-blind overlay can keep them apart |
| `--experience-accent` | the experience family's accent text |
| `--folder-label` | an inventory folder's label, set apart from an item's (gold in the reference) |
| `--console-info` / `--console-error` | an RLVa console line that reports, or fails; a reply takes `--text-muted` |
| `--control-bg` | a button's / combo's resting background |
| `--control-bg-hover` | the background under the pointer (hovered button, highlighted menu entry) |
| `--control-bg-disabled` | a disabled control's background |
| `--control-border` | a control's resting border |
| `--control-border-disabled` | a disabled control's border |
| `--control-radius` | a button's corner radius |
| `--field-bg` | an editable field's recessed well |
| `--field-bg-focused` | the well of the field you are typing in (a search box's too) |
| `--field-bg-readonly` | the well of a field you can read and copy but not change |
| `--field-text` | the text being edited, and any text on a data surface (see below) |
| `--field-text-readonly` / `--field-text-disabled` | a read-only field's text, and a refused field's or list entry's |
| `--field-placeholder` | a field's prompt while it is empty, and the secondary text on a data surface |
| `--list-bg` | a scroll list's face — a scrim over the panel behind it in both shipped skins |
| `--combo-list-bg` | a combo's open drop-down; **opaque**, unlike `--list-bg`, because a drop-down floats and has no panel behind it |
| `--caret` / `--selection` / `--selection-unfocused` | the text caret and its two selection washes |
| `--focus-ring` | the keyboard-focus ring's colour |
| `--focus-ring-width` / `--focus-ring-offset` | its geometry: how thick the ring is, and how far it stands off the widget (a classic hairline is `1px` / `0px`) |
| `--text-shadow` | the drop shadow under **chrome** text — a label, a button's or a tab's caption, a floater title, a status read-out — as a whole `text-shadow` value (`1px 1px #000000a6`, or `none`); data text never takes it (see below) |
| `--accent` | accent bars, an active tab's frame, a chosen skin-tone swatch, a drag grip |
| `--accent-muted` | the accent dimmed to say "set, but not by you": a Friends-list right the friend grants you |
| `--selection-bg` | a lit control's (translucent) background: a toggled toolbar button, an active tab |
| `--list-row-bg` | a scroll list's ordinary row — transparent in both shipped skins, so the list's own face shows through |
| `--list-row-stripe` | every other row of a scroll list, by the row's data index |
| `--list-row-hover` | the row under the pointer |
| `--list-row-selected-bg` | a selected row's (translucent) background |
| `--list-row-selected-text` | a selected row's text, which a light list moves off the field family |
| `--drop-target` | the row a drag is over — drawn over the selection, so dropping onto the selected folder still shows where it lands |
| `--match-highlight` | the glyphs of a filter match inside an ordinary label |
| `--tile-bg` | an inventory gallery tile's backing, a scrim so a thumbnail reads against any panel |
| `--tile-hover` | the wash under the pointer on a dense grid's tile (an emoji cell) |
| `--inline-item-bg` / `--inline-item-bg-hover` | an item embedded in notecard prose, at rest and under the pointer |
| `--marker-selected` | the selected keyframe on the day-cycle timeline (a colour of its own because it sits on a sky gradient) |
| `--check-bg` / `--check-border` | a checkbox's box, unchecked |
| `--check-bg-checked` / `--check-border-checked` | the same box, checked |
| `--check-tick` | the tick's colour (the tick itself is a `content` glyph — see the pseudo-elements note below) |
| `--radio-bg` / `--radio-border` | a radio option's disc, unlit |
| `--radio-bg-checked` / `--radio-border-checked` | the lit option's disc |
| `--radio-pip` | the mark inside the lit disc |
| `--divider` | a splitter, a rule, a column resize handle |
| `--track-bg` | the trough a scrollbar thumb or slider handle runs in |
| `--scrollbar-thumb` | a scrollbar thumb |
| `--slider-thumb` | a slider handle or trackball marker |
| `--title-bar-active` | the focused floater's title band (a translucent wash over its body) |
| `--title-text-inactive` | an unfocused floater's title text |
| `--glyph-button-bg` | a title-bar glyph button's fill |
| `--pie-bg` / `--pie-line` / `--pie-selected` | the pie menu's disc, spokes and hovered wedge |
| `--pie-label-sub-pie` | a wedge caption that opens a sub-pie |
| `--pie-label-disabled` | a wedge caption that is present but unavailable |
| `--chat-recall` / `--chat-server-history` / `--chat-live` | the Conversations transcript bands |
| `--gain` | **meaning-bearing**: a positive / up / gain value; also an arrived teleport |
| `--loss` | **meaning-bearing**: a negative / down / loss value; also the parcel-restriction icons and a failed teleport |
| `--presence-online` / `--presence-offline` | **meaning-bearing**: a friend's presence dot |
| `--notice-tip` / `--notice-notify` / `--notice-alert` / `--notice-modal` | **meaning-bearing**: a notification toast's frame by kind, and its default button's |

#### Chrome and data surfaces

The tokens come in **two surface-and-text families**. Chrome — floaters, the
menu bar, toolbars, buttons — is painted from `--control-*`, `--surface-*` and
`--text-*`. A place where *data* lives — a text field, a scroll list, a combo's
drop-down — is painted from `--field-*`, `--list-bg` and `--combo-list-bg`. The
shipped skins make both dark, so the split is invisible in them; it exists for a
skin that puts its data on a light face inside dark chrome, as the reference's
classic skins do, where one pair of tokens cannot be both.

What makes that work is `.sk-list-surface`, which every scroll list's viewport
carries. **Inside it the generic text roles re-root onto the field family:** an
`.sk-text` resolves to `--field-text` and an `.sk-title` to
`--field-placeholder`, because a role only means something against the surface
it sits on. Two exceptions, both deliberate: a button inside a row brings its
own chrome with it, so its caption goes back to `--text-primary`, and a
*selected* row's text takes `--list-row-selected-text`. The state classes
(`.sk-active-text`, `.sk-highlighted-text`, `.sk-disabled-text`) are restated
for a data surface, so a list's face never swallows a state.

The text shadow follows the same line. `--text-shadow` reaches only chrome
text: inside `.sk-list-surface` or an `.sk-list-row`, and in a text field or a
drop-down option, the rules state `text-shadow: none`, because the reference
shadows its labels and captions and never a list cell, an editor or a menu —
and a dark shadow under a light list's black text only smudges it. A button in
a row keeps its caption's shadow, as it keeps its colour. The shipped skins set
`none`; the Graphite *Relief* theme sets one. A shadow is drawn behind the
glyphs and is no part of the text's measure, so turning it on moves nothing.

A refused checkbox, radio, field or combo does not have tokens of its own: it
greys to `--control-bg-disabled` / `--control-border-disabled`, and its caption
and mark to `--text-disabled`.

This table, the bevel table and the class tables below are held to
`common.css` by a test
(`the_skin_chapter_names_every_token_and_class_common_css_uses` in
`tests/shipped_skins.rs`): a token or a class the structural sheet uses that no
table in this chapter names fails it, and so does a table row naming a token
the sheet no longer reads. A skin author can take the tables as the whole
vocabulary.

On top of these each skin defines the **user-tunable palette** — the chat,
name-tag and minimap colours the preferences' *Colors & Skins* tab exposes.
Those are not consumed by `common.css`: the viewer reads them off the styled
root and feeds them into the settings store as the skin's declared defaults,
under any per-account override.

A skin **must** define every token it does not inherit; a `var()` that resolves
to nothing leaves the property unset (usually invisible). Both halves are
checked against the real stylesheets at build time
(`tests/shipped_skins.rs`), so a token a skin forgets fails the test rather
than showing up as one widget in the wrong colour.

### The role palette — skinning what a selector cannot reach

Most of the tokens above are consumed the ordinary way: a rule in `common.css`
gives a `.sk-*` class a `var(--role)`, and `bevy_flair` paints every node
carrying that class.

That only works where a **selector** can reach the paint, and a good part of the
widget set is painted from Rust against state the CSS engine cannot see — a
floater title bar that changes as focus moves, a tab strip repainted when the
selection moves, a recycled table row that lights while selected, a pie-menu
wedge that is a shader rather than a node. Those read the resolved role colours
instead, through a second mechanism:

```css
:root {
  -sk-color-surface-bg: var(--surface-bg);
  -sk-color-text-muted: var(--text-muted);
  /* … one line per role … */
}
```

Each `-sk-color-<role>` property writes one field of a `SkinPalette` component
that lands on the styled root, and the Rust paint systems read it from there. It
is **not a second vocabulary**: every value is one of the same `--<role>` tokens
the classes use, so a skin still defines each role exactly once and never has to
know which of the two routes a given widget takes.

As a skin author you never touch that `:root` block — it is part of
`common.css`, like the class rules. What it means for you is simply that
retuning a role token restyles *both* halves: `--accent` moves the active tab's
frame (Rust) and the divider grip (CSS) together.

A skin that omits a role leaves that field at the colour the widget module
declares as its unskinned fallback, so a partial third-party skin degrades to
the stock look rather than to black.

### The widget classes

`common.css` defines these; a widget opts into skinning by carrying the class.

| Class | What it styles |
| --- | --- |
| `.sk-panel` | a framed surface (bg, border, radius, padding) |
| `.sk-card` | a background-only surface (bg + radius, no border/padding) |
| `.sk-title` | an instruction / secondary line |
| `.sk-text` | primary body text |
| `.sk-error` / `.sk-warn` / `.sk-note` / `.sk-experience` | worn with `.sk-text`: a failure, a warning, a note, the experience accent |
| `.sk-folder-label` | worn with `.sk-text`: an inventory folder's label |
| `.sk-console-reply` / `.sk-console-info` / `.sk-console-error` | worn with `.sk-text`: an RLVa console line by kind (a typed command wears none) |
| `.sk-match` | text that matched an active filter term |
| `.sk-heading` | a heading inside a page |
| `.sk-button` | a button, plus `:hover`, `:disabled` and `:focus-visible` states |
| `.sk-button-compact` | worn with `.sk-button`: the same button at row scale (a table cell, a dense strip) |
| `.sk-action-button` | a flat action-column button — the refused state only, no resting look |
| `.sk-checkbox` / `.sk-checkbox-box` / `.sk-checkbox-tick` | a checkbox row (`:checked`, `:disabled`), its box, and the empty text node whose `::before` is the tick |
| `.sk-radio-group` / `.sk-radio` / `.sk-radio-indicator` / `.sk-radio-pip` | a radio group (`:disabled`), one option (`:checked`), its disc, and the node whose `::before` is the pip |
| `.sk-combo` / `.sk-combo-list` / `.sk-combo-option` | a combo's anchor box (`:disabled`), its open drop-down, one option row (`:hover`, `:disabled`) |
| `.sk-swatch` | a colour swatch's rim (its fill is the colour it carries); `:disabled` dims it |
| `.sk-tone-swatch` | an emoji skin-tone tile; `:checked` outlines the chosen one |
| `.sk-trackball` / `.sk-trackball-disc` | a sun / moon trackball (`:disabled`) and its disc's rim |
| `.sk-rights-cell` | a Friends-list permission icon: dim, `:checked` (a right you grant) or `:checked:disabled` (one granted to you) |
| `.sk-tile` | one tile of a dense grid (an emoji cell), with its `:hover` wash |
| `.sk-gallery-tile` | an inventory gallery tile's backing |
| `.sk-inline-item` | an item embedded in notecard prose, with its `:hover` |
| `.sk-day-marker` | a keyframe marker on the day-cycle timeline; `.sk-active` when selected |
| `.sk-teleport-title` / `.sk-teleport-arrived` / `.sk-teleport-failed` | the teleport-progress title, and the two outcomes it ends in |
| `.sk-presence-online` / `.sk-presence-offline` | a friend's presence dot |
| `.sk-pie-label` / `.sk-pie-label-sub-pie` / `.sk-pie-label-unavailable` | a pie slice's caption: ordinary, one that opens a sub-pie, one that cannot be picked |
| `.sk-overlay` / `.sk-overlay-text` | an overlay over the rendered world and its text |
| `.sk-disabled-surface` / `.sk-disabled-text` | the greyed state of either |
| `.sk-focusable` | the keyboard focus ring (stamped automatically onto every `TabIndex`) |
| `.sk-accent` | a leading accent bar + hanging indent (logical box demo) |
| `.sk-tab` / `.sk-tab-label` | a tab button with asymmetric top corners (`:checked` when selected, `:disabled` when refused) and its caption |
| `.sk-no-match` | worn with `.sk-tab-label`: a tab a live search left with no matching row |
| `.sk-gain` / `.sk-loss` | meaning-bearing colour swatches |
| `.sk-menu-bar` / `.sk-menu` | the top bar / a drop-down menu surface |
| `.sk-menu-bar-item` / `.sk-menu-item` | a bar button, an entry (`:disabled` greys it) |
| `.sk-menu-item-label` / `.sk-menu-accessory` | an entry's label spans, and its accelerator text or submenu arrow |
| `.sk-menu-item-match` | an entry that matched the menu search |
| `.sk-menu-separator` | the rule between two groups of entries |
| `.sk-floater` | a floater's body |
| `.sk-floater-title-bar` / `.sk-floater-title-text` | its title band and title, at rest; the focused floater's wear `.sk-active` / `.sk-active-text` |
| `.sk-floater-button` / `.sk-floater-glyph` / `.sk-floater-grip` | its title-bar buttons, their glyphs, the resize grip |
| `.sk-dock-host` | the strip docked floaters flow into |
| `.sk-tab-panel` | a tab page |
| `.sk-scrollbar-track` / `.sk-scrollbar-thumb` | a scrollbar, in both the tab strip and the windowed list |
| `.sk-divider` / `.sk-divider-grip` / `.sk-column-resizer` | a pane splitter, its nub, a table column's drag handle |
| `.sk-list-row` / `.sk-table-row` | one row of a scroll list, plus its `:hover`; worn with `.sk-stripe` on every other row and `.sk-active` when selected |
| `.sk-field` | an editable text field's box, plus `:focus` and `:disabled` |
| `.sk-read-only` | worn with `.sk-field` / `.sk-text-field`: a field that can be read and copied but not changed |
| `.sk-field-placeholder` | a field's prompt while it is empty, and a search box's leading glyph |
| `.sk-list-surface` | a scroll list's face — and the scope inside which the text roles re-root onto the field family (see *Chrome and data surfaces*) |
| `.sk-text-field` | the caret / selection colours of **every** editor (stamped automatically) |
| `.sk-search-field` / `.sk-search-clear` | the shared search box and its `×` button |
| `.sk-toolbar-bar` / `.sk-toolbar-button` / `.sk-toolbar-label` | the bottom toolbar strip, its buttons, and their labels |
| `.sk-toast` / `.sk-toast-text` | a notification toast card and its text |
| `.sk-toast-tip` / `.sk-toast-notify` / `.sk-toast-alert` / `.sk-toast-modal` | worn with `.sk-toast`: its kind, which frames the card |
| `.sk-toast-default` | the card's default button (the one Enter or expiry takes), framed in its kind's colour |
| `.sk-build-label` / `.sk-build-value` / `.sk-build-placeholder` / `.sk-build-disabled` | the Build Tools floater's text roles |
| `.sk-conversations` | the Conversations transcript bands |
| `.sk-status-readout` | a status-row read-out (region / coordinates / balance / time / FPS) |
| `.sk-parcel-icon` | a parcel-permission icon on the status row (see below) |

A few things are deliberately **absent** from that list even though they are
skinnable — the pie menu's disc and wedges above all (its captions are in it),
which are a **shader** rather than nodes the cascade can reach. They take their
colours from the role palette instead: retuning the role token restyles them,
and there is no class to write a rule against.

#### The state classes

Widget **state** is not in that group: hovered, selected, toggled and refused
are pseudo-classes where the engine can see the state (`:hover`, `:checked`,
`:disabled`, `:focus`, `:focus-visible`) and classes where only the viewer can —
a menu row lit by the keyboard, the floater a toolbar button toggles being open,
a list row's data index. A widget's own system adds and removes these, and a
skin restyles a selected row or a lit toolbar button by writing a rule like any
other:

| Class | State |
| --- | --- |
| `.sk-highlighted` / `.sk-highlighted-text` | lit by the pointer *or the keyboard* — a menu entry, a menu-bar button |
| `.sk-active` / `.sk-active-text` | toggled on or selected: a lit toolbar button, a selected row, the focused floater's title |
| `.sk-stripe` | every other row of a scroll list, by the row's **data** index (so the bands do not crawl as a recycled list scrolls) |
| `.sk-drop-target` | the row a drag is over |
| `.sk-attention` | wants attention — the Conversations button with unread messages; an animation, so a skin that would rather not blink overrides it with a static paint |
| `.sk-focus-within` | a container whose editor has focus — the search box, which brightens and rings while you type in it (there is no `:focus-within`) |

They come **last** in `common.css`, in the order highlighted, active, disabled,
because they carry the same specificity as the resting rules they override and
only file order separates them: a greyed row must never also read as lit.

### The status-bar parcel-permission icons

The top row's status area shows the current parcel's permission icons — voice,
fly, push, build, scripts, see-avatars and damage — each shown only while that
restriction is in force. Each icon is a **white-on-transparent glyph mask**
drawn with an `ImageNode`, so the skin **tints** it rather than shipping a
coloured image. Two non-standard `bevy_flair` properties control an image (they
also work on any other `ImageNode` a future widget adds):

| Property | Sets | Use |
| --- | --- | --- |
| `-bevy-image-color` | the image's tint (multiplies the glyph) | recolour a white mask |
| `-bevy-image` | the image itself (`url("skins/…/foo.png")`) | replace the glyph art |

Every icon carries the shared class `.sk-parcel-icon` **and** a per-icon class,
so a skin can restyle all of them at once or target one:

| Class | Icon |
| --- | --- |
| `.sk-parcel-icon` | all parcel icons (default tint `var(--loss)`) |
| `.sk-parcel-icon--voice` | voice disabled |
| `.sk-parcel-icon--fly` | flying disabled |
| `.sk-parcel-icon--push` | pushing restricted |
| `.sk-parcel-icon--build` | building disabled |
| `.sk-parcel-icon--scripts` | scripts disabled |
| `.sk-parcel-icon--see-avatars` | avatars hidden |
| `.sk-parcel-icon--damage` | damage enabled (a hazard) |

```css
/* Re-tint every parcel icon. */
.sk-parcel-icon {
  -bevy-image-color: var(--accent);
}

/* Replace just one glyph with your own art. The tint still multiplies, so ship
   a white mask (or set `-bevy-image-color: #ffffff` to show it as-is). */
.sk-parcel-icon--damage {
  -bevy-image: url("skins/myskin/parcel-damage.png");
}
```

Because the `-bevy-*` properties are not standard CSS, the commit-time `biome`
lint reports them as unknown. A file that uses any of them **must** carry a
file-level suppression as its first line (`common.css` does):

```css
/* biome-ignore-all lint/correctness/noUnknownProperty: bevy_flair -bevy-* */
```

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

**A bevel is the exception with no logical spelling.** `border-left-color` and
`border-right-color` are banned in a skin, but not because a colour has no
handedness: their one use is a bevel — a lit edge and a shaded one — and a
bevel's light source belongs to the rendering, not to the writing direction.
Mirrored under a right-to-left locale it would look lit from the wrong corner,
which is why no desktop toolkit mirrors one. So the side colours are physical
**on purpose**, and only the structural sheet writes them: the push button and
the field wells (`.sk-field`, `.sk-search-field`) read four tokens, and a skin
draws a bevel by setting those.

| Token | Paints |
| --- | --- |
| `--button-bevel-top-left` | a push button's top and left edges |
| `--button-bevel-bottom-right` | its bottom and right edges |
| `--field-bevel-top-left` | a field well's top and left edges |
| `--field-bevel-bottom-right` | its bottom and right edges |

They name physical edges rather than "light" and "shadow" because which corner
is lit is the skin's choice, and differs per widget: the Windows convention
raises a button and sinks a field (dark on opposite corners), while Vintage
draws both dark at the top-left. A flat skin gives all four its
`--control-border` and keeps a one-colour frame. A refused field drops back to
a flat `--control-border-disabled` frame whatever its bevel.

**Symmetric shorthands are fine** when they carry a *single* value, because a
single value is the same on every side and cannot be handed the wrong way:
`padding: 12px`, `border-width: 1px`, `border-radius: 6px`. Avoid the
**asymmetric** shorthand forms (`padding: 4px 8px 4px 24px`) — those expand to
physical left/right and will not mirror; use the logical longhands instead.

## Other ways this differs from plain CSS / `bevy_flair`

- **Colours** use `bevy_flair`'s parser: hex (`#rgb`, `#rrggbb`, `#rrggbbaa`),
  `rgb(...)`, `oklch(...)`, named colours, and `var(...)`. Alpha via 8-digit hex
  (`#1c1f26f2`) works.
- **Pseudo-classes** supported: `:hover`, `:active`, `:focus`, `:focus-visible`,
  `:checked` and `:disabled`. `:focus-visible` tracks the viewer's **keyboard
  (Tab) focus** — so a `:focus-visible` ring shows on Tab and hides on click,
  which is exactly what you want for a focus ring. `:checked` follows a widget's
  `Checked` (a ticked box, a lit radio option, the selected tab) and `:disabled`
  its `InteractionDisabled`. There is no `:focus-within` and no reliable
  `:has()`; the viewer stamps `.sk-focus-within` instead.
- **`::before` with `content`** works on a node the widget built for it — the
  checkbox tick and the radio pip are written this way, so which mark a box
  wears is the skin's choice (`content: "\2714";`). It is not general: a
  pseudo-element exists only where the widget opted the node in, so there is no
  `::before` to write on an arbitrary class.
- **A property is never reverted.** When a state rule stops matching,
  `bevy_flair` does not put back the value the property had before — it only
  ever applies the winning rule. So every property a state rule writes needs a
  **resting rule** that writes it too, or the state sticks: a `:hover` wash
  stays on every tile the pointer ever crossed, a tick stays after the box is
  unticked. `common.css` gives every state its resting counterpart, and any new
  state rule must come with one.
- **`var()` has no fallback value** — `var(--x, blue)` is not supported. Define
  every token.
- **Images are local bundled files only.** A texture token points at a file
  under the viewer's assets (`-bevy-image: url("skins/…/foo.png")`),
  **never a grid asset UUID** — grid textures (a texture-picker thumbnail, a
  texture display) are *content*, handled by the render pipeline, not the skin.
  `-bevy-image-color` tints an `ImageNode` (multiplying the image); it recolours
  a white glyph mask, as the parcel-permission icons above use it.
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
