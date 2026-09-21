---
id: viewer-audit-skin-token-coverage
title: The skin system covers two widgets
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 8
---

Context: [context/viewer.md](../context/viewer.md).

Across the viewer crates there are **643 hardcoded `Color::srgb*` literals**
against **98 `ClassList` attachments**, and only five files reference a `sk-`
skin class at all: `menu.rs` (8), `skin.rs` (4), `ui_search.rs` (2),
`ui_combo.rs` (1), `ui_color_picker.rs` (1). `assets/skins/common.css` defines
35 classes.

So switching skin or theme restyles the menu bar and the search box — and
nothing else. `floater.rs` (9 colours + `DOCK_HOST_BACKGROUND`), `ui_tab.rs`
(12), `ui_text_input.rs` (7), `pie_menu.rs` (8), `ui_radio.rs` (4),
`ui_table.rs` (2) and `virtual_list.rs` (2) all declare theirs in Rust, as
dark-theme values (e.g. `floater.rs:112 FLOATER_BACKGROUND = Color::srgba(0.11,
0.12, 0.15, 0.95)`). Floaters, tabs, tables, radios, text fields, combos and pie
menus are **not skinnable**.

The two worst offenders by frequency are worth promoting first:
`const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96)` is copy-pasted **20
times** workspace-wide and `DIM_LABEL_COLOR: srgb(0.62, 0.66, 0.74)` **12
times** — and three copies have already drifted (`volume_panel.rs:66`,
`quick_preferences.rs:106`, `inspector_popup.rs:118`).

This is a decision as much as a task: either widen the CSS vocabulary to cover
the widget set, or scope what "skin" means in the docs. Right now the feature's
reach and its billing do not match.

Note one legitimate exception to keep: `notification_host.rs:194 kind_accent` is
explicitly meaning-bearing ("the kind accent is painted on it in Rust", `:97`)
rather than a fallback — though the toast card's own background *is*
skin-driven, so one card currently mixes both systems.

## The decision: widen the vocabulary

The decision this asked for is **widen**, and the reason a class-only widening
could not have worked is worth writing down: a CSS class can only paint what a
*selector* can reach, and most of the widget set is painted from Rust against
state `bevy_flair` cannot see — a floater title bar that changes as focus
moves, a tab strip repainted when the selection moves, a recycled table row
that lights while selected, a pie-menu disc that is a shader rather than a
node. Worse, a class actively *breaks* those: a `.sk-*` `color` rule beats the
Rust-painted `TextColor`, which is how disabled menu entries once rendered
white in the live viewer (see `sl-client-skin-tokens-bevy-flair`).

So the skin now reaches the widget set two ways, both fed from the **same role
tokens**:

- a `.sk-*` class where a selector can reach the paint (the preferred route —
  `bevy_flair` repaints, no code involved);
- `SkinPalette`, a shim component on the styled root, everywhere else. Each
  `-sk-color-<role>` CSS property (a `:root` rule in `common.css`) writes one
  field, and the Rust paint systems read it through the `SkinColors` system
  parameter. Same shim pattern as `SkinTextCaret` / `SkinChatBands`.

`SkinPalette::default()` (and the `FALLBACK` const, for the `const` contexts a
static table spec needs) holds exactly the colours the widget modules used to
declare, so an unskinned world — a unit test, the gallery before its first
dress, a third-party skin defining a subset — looks as it did.

### What is skinnable now

30 roles, up from 14. Newly covered, with the route each takes:

| Widget | Route |
| --- | --- |
| floater body, glyph buttons, resize grip, dock host | class |
| floater title band + title text (focus state) | palette |
| tab page, scrollbars, pane splitter, column resizer | class |
| tab background / border / label (active, disabled) | palette |
| text field box | class; its text is palette (read-only state) |
| search box, its clear button | class; the glyph is palette (disabled state) |
| combo anchor, popover, separator | class + palette (disabled state, row hover) |
| radio indicators, option labels | palette (three states) |
| table row selection, disabled header | palette |
| menu surface, entries, separators | class + palette (hover, filter match) |
| pie disc / spokes / hovered wedge | palette, pushed into the shader's params |
| pie captions (action / sub-pie / unavailable) | palette, via `PieLabelRole` |

The 64 copied label constants collapse onto `SkinPalette::FALLBACK.text_primary`
/ `.text_muted` / `.text_disabled`, which is what removes the drift the audit
found. Those panels are **not yet live-skinnable** — they are spawned from free
functions with no world access, and several build their table specs in `const`
context — but there is now exactly one place to widen them from, and that is
its own task (`viewer-skin-panel-text-roles`).

### The silent-failure guard

Three links have to hold for a role to reach a widget: the registered CSS
property, the `common.css` rule, and the skin's token. Every one of them fails
**silently** — an unregistered property parses to nothing, a missing token
leaves the field at its fallback — so each is asserted:

- `skin_palette.rs`: every property is on the registry, and the property table
  covers every field exactly once (reflection, so a new field cannot be
  forgotten);
- `tests/shipped_skins.rs`: `common.css` wires every registered property, and
  every shipped skin defines the token it reads;
- `tests/skin_palette_resolves.rs`: the real stylesheets through the real
  engine — graphite's `--surface-bg`, `--text-muted` and `--pie-selected` reach
  the styled root's `SkinPalette`, and differ from the fallback. This is the
  one link the static checks cannot see, and the one a `bevy_flair` upgrade is
  most likely to move.

### Kept as it was

`notification_host.rs`'s `kind_accent` stays Rust-painted and unskinned: it is
meaning-bearing (Tip / Notify / Alert / Modal), like `--gain` / `--loss`, and
the four kinds have to stay distinguishable whatever the skin. The toast card's
own surface is class-driven, so the card still mixes both systems — on purpose.
