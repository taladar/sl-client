# The Firestorm Vintage skin — reference survey

Non-task background for the `viewer-skin-*` / `viewer-vintage-*` tasks. It
records **what the Vintage skin actually is**, in resolved numbers, so that no
implementation task has to re-derive it and so that two tasks cannot disagree
about a value.

Scope note: this survey is about **colour and widget shape**. Vintage's one
real *layout* deviation (its bottom bar) is its own task,
`viewer-vintage-bottom-bar`, and deliberate deviations on our side (the
Stand / Stop-flying / Stop-flycam placement) stay deviations.

The OpenSim tree contributes nothing here: a grid serves no UI skin data, so
the whole subject lives in the reference viewer's `skins/` tree and in ours.

## Where it lives and how the reference resolves it

Read-only, in `~/devel/3rdparty/phoenix-firestorm/indra/newview/skins/`:

- `skins.xml` — the skin registry. Vintage is one skin (folder `vintage`)
  with one theme, `Classic`, whose folder is empty, i.e. no overlay.
- `vintage/colors.xml` — 100 colour overrides.
- `vintage/textures/textures.xml` + `textures/{widgets,windows,containers,
  taskpanel}/` — re-points named widget images at Vintage's own art.
- `vintage/xui/en/widgets/*.xml` — per-widget defaults (16 files).
- `vintage/xui/en/*.xml` — whole-file floater/panel forks (layout; **not**
  something we copy — see `viewer-ui-skin-tokens`).

Resolution order for a colour: `default/colors.xml` first, then the skin's
own file replaces by name; a `reference="Other"` entry resolves through the
merged table. So a faithful Vintage palette is *the merge*, not the 100-entry
override file alone — several values a Vintage skin needs (`TextCursorColor`,
`TextFgReadOnlyColor`, `UserChatColor`) are inherited, not overridden.

To re-derive the table below: parse both `colors.xml` files into one dict
(`vintage` over `default`), then resolve `value` / `reference` transitively;
a `value` that is not three or four floats is itself a name.

## The identity, in one paragraph

Vintage inverts the modern skins. **Chrome** — floaters, menu bar, toolbars —
is flat dark grey (`#3e3e3e`) with a hard 1 px black frame. **Data surfaces**
— every text editor, scroll list, combo list, notecard, script editor — are
**light** sage grey (`#c8cfcc` focused, `#bac3be` at rest) carrying **black**
text. Selection is periwinkle (`#8d90c2`). Push buttons are raised bevels with
a blue-violet face (`#6473bd`) and a **warm gold** frame when pressed or
toggled. Static labels are steel blue (`#93a9d5`), not white. All text carries
a soft drop shadow. Every corner is square.

Our two skins (`graphite`, `azure`) are dark-on-dark throughout, rounded, and
shadowless. Nothing about that is wrong — but it means a Vintage skin is not
expressible as a value swap over the tokens we have today, which is what most
of the `viewer-skin-*` tasks are about.

## Resolved palette

Values are the merged `default` + `vintage` resolution, sRGB hex, with an
alpha byte appended when it is not opaque. "(v)" marks an entry Vintage
overrides itself; the rest are inherited from the default skin and are part of
the skin all the same.

### Chrome

| Reference colour | Value | Dresses |
| --- | --- | --- |
| `DkGray` (v) | `#3e3e3e` | floater body, and the menu bar via `MenuBarBgColor` |
| `TitleBarFocusColor` (v) | `#555555` | the focused floater header strip |
| `MenuDefaultBgColor` (v) | `#000000` | dropped-down menus — opaque black |
| `ChatHistoryBgColor` (v) | `#3e3e3ea1` | the chat transcript band |
| `SL-Background` (v) | `#6c6c6c` | the login / progress backdrop |
| `DefaultHighlightLight` (v) | `#73849b` | bevel-box highlight edge |
| `DefaultShadowLight` (v) | `#000000` | bevel-box shadow edge |

### Data surfaces (the light half)

| Reference colour | Value | Dresses |
| --- | --- | --- |
| `UIControlBGLightFocused` (v) | `#c8cfcc` | focused editor / list background |
| `UIControlBGLightUnfocused` (v) | `#b9c2bd` | the same, unfocused |
| `TextBgWriteableColor` (v) | `#bac3be` | a writable text editor |
| `TextBgReadOnlyColor` (v) | `#3e3e3e` | a read-only editor — back to chrome grey |
| `ComboListBgColor` (v) | `#e6e6e6` | the combo drop-down list |
| `ScriptBackground` (v) | `#c8d1cc` | the LSL editor |
| `NotecardBackgroundColor` (v) | `#c8cfcc` | a notecard |
| `GroupNotifyTextBG` (v) | `#daeeff` | a group notice's body |

### Text

| Reference colour | Value | Dresses |
| --- | --- | --- |
| `TextFgColor` (v) | `#000000` | text in any input widget |
| `TextFgDisabledColor` (v) | `#00000040` | the same, disabled |
| `LabelTextColor` (v) | `#93a9d5` | **every static label** — steel blue |
| `ButtonLabelColor` (v) | `#dcdcdc` | a push-button label |
| `ButtonLabelDisabledColor` (v) | `#8c90c2` | a disabled button label |
| `ButtonLabelSelectedColor` | `#ffffff` | a toggled button label |
| `TextFgReadOnlyColor` | `#e6e6e6` | read-only editor text (on chrome grey) |
| `TextFgTentativeColor` | `#666666` | placeholder text |
| `TextCursorColor` | `#000000` | the caret — black, on the light field |

### Lists and selection

| Reference colour | Value | Dresses |
| --- | --- | --- |
| `ScrollBgWriteableColor` (v) | `#c8cfcc` | the list background |
| `ScrollBGStripeColor` (v) | `#b8bfbb` | **every other row** |
| `ScrollHoveredColor` (v) | `#bec3c3` | the hovered row |
| `ScrollSelectedBGColor` (v) | `#8d90c2` | the selected row |
| `ScrollSelectedFGColor` (v) | `#000000` | selected-row text |
| `ScrollDisabledColor` (v) | `#00000040` | a disabled row |
| `TextBgSelectedColor` (v) | `#7375be` | an editor's selection band |
| `TextBgHighlightColor` (v) | `#7375bea8` | the search highlight |
| `ScrollbarThumbColor` (v) | `#ffffff` | tints `ScrollThumb_*` |
| `ScrollbarTrackColor` (v) | `#999999` | tints `ScrollTrack_*` |

`TextBgSelectedColor` is `UIControlBGSelectedCompensate`, a *pre-darkened*
twin of `UIControlBGSelected` that exists only because the reference applies a
hard-coded 0.7 alpha in code (its own comment says so). We have no such
multiplier, so a Vintage skin should use `#8d90c2` for both and drop the
compensation twin.

### Chat and money

| Reference colour | Value | Dresses |
| --- | --- | --- |
| `UserChatColor` | `#ffff00` | the user's own nearby-chat lines |
| `AgentChatColor` | `#ffffff` | other avatars' lines |
| `ObjectChatColor` (v) | `#b2e5b2` | object chat — pale green |
| `IMChatColor` (v) | `#e6e6e6` | IM lines |
| `SystemChatColor` (v) | `#e6e6e6` | system lines |
| `ChatTimestampColor` (v) | `#808080` | the per-line timestamp |
| `CurrencyColor` (v) | `#00ff00` | the L$ balance read-out |
| `MoneyTrackerIncrease` (v) | `#006600` | a credit |
| `MoneyTrackerDecrease` (v) | `#660000` | a debit |

### World-facing

| Reference colour | Value | Dresses |
| --- | --- | --- |
| `NetMapBackgroundColor` (v) | `#00000099` | the minimap backdrop |
| `MapAvatarColor` (v) | `#00ff00` | a minimap avatar dot |
| `MapAvatarFriendColor` (v) | `#ffff00` | a friend's dot |
| `MapAvatarSelfColor` | `#ffff00` | the own dot |
| `MapAvatarLindenColor` | `#0000ff` | a Linden's dot |
| `MapAvatarMutedColor` | `#666666` | a muted avatar's dot |
| `MapTrackColor` | `#ba001f` | the tracking beacon |
| `NameTagMatch` / `Mismatch` (v) | `#fab05c` | **both** name-tag cases |
| `AvatarListItemChatRange` (v) | `#000000` | a radar row within chat range |
| `AvatarListItemShoutRange` (v) | `#00000080` | within shout range |
| `AvatarListItemBeyondShoutRange` (v) | `#66000066` | beyond it |

The minimap-dot group is **already Vintage-accurate** in our skins — both
`graphite/skin.css` and the comment above it took these values from Vintage
during `viewer-minimap-avatar-dot-color`. It is the one group that needs no
work, and it is the precedent for the rest.

Two divergences worth naming, because they are choices and not oversights:
our `--chat-self` is white where the reference default is **yellow**, and our
`--name-tag-mismatch` distinguishes the mismatch case where Vintage
deliberately collapses both onto one orange.

## Measured widget art

Vintage's identity is as much in its textures as in `colors.xml`, and the
textures are what `colors.xml` *tints*. Sampled from the PNG/TGA files
(centre pixel, then the four edge pixels):

| Texture | Size | Face | Edges |
| --- | --- | --- | --- |
| `PushButton_Off` | 32×23 | `#6473bd` | dark top/left, light bottom/right |
| `PushButton_Selected` | 32×23 | `#515d9a` | gold `#ffd794` frame |
| `PushButton_Disabled` | 32×23 | `#363b54` | flat, no bevel |
| `TextField_Off` | 256×23 | `#bac3be` | sunken: black top/left |
| `TextField_Active` | 256×23 | `#c8d1cc` | sunken: black top/left |
| `floater_background` | 32×32 | `#3e3e3e` | 1 px pure black, all four |
| `floater_background_header` | 32×32 | `#555555` | 1 px pure black |
| `Tooltip` | 102×17 | `#b7b8bc` | darker `#9d9ea2` top |
| `Checkbox_Off` / `_On` | 15×15 | `#d8d8d8` | 1 px `#0a0a0a`, soft shadow |
| `RadioButton_Off` | 15×15 | `#ffffff` | dark ring |
| `ScrollThumb_Vert` | 14×57 | `#3c4c7c` | flat |
| `ScrollTrack_Vert` | 14×57 | `#999999` | flat |
| `ProgressBar` / `Track` | 152×15 | `#889ec7` / `#526680` | — |
| `ListItem_Over` | 280×24 | `#bec3c3` | flat |
| `ListItem_Select` | 280×24 | `#8c90c2` | flat |
| `SliderThumb_Off` | 14×14 | `#726c76` | bevelled |
| `SliderTrack_Horiz` | 104×6 | `#646e9d` | sunken |

Two things to read out of that table. First, **the bevel is inverted from the
Windows convention** — the dark edge is at the top-left and the light edge at
the bottom-right. Second, **state is a different texture**, not a different
tint: idle, pressed/toggled and disabled are three separate images, and the
pressed one changes hue family entirely (blue-violet to gold). Our widgets
paint their states as background colours from Rust, which is why
`viewer-skin-widget-state-classes` has to come before any of the art.

Every one of these is 9-sliced: `textures.xml` gives each a
`scale.left/top/right/bottom`, e.g. `PushButton_Off` is `12 12 18 13`.

## Widget defaults Vintage changes

Diffing `vintage/xui/en/widgets/*.xml` against `default/xui/en/widgets/`
isolates the shape deltas from the (much larger) whole-file floater forks:

- `text.xml`: `font_shadow="soft"` (default: `none`) — **every static label
  gets a drop shadow**, and `text_color` is `LabelTextColor`.
- `button.xml`: `label_shadow="true"`, `height="23"`, `pad_bottom="2"`.
- `tab_container.xml`: `tab_height="18"` (default 21), `halign="left"`
  (default `center`), `tab_max_width="150"`, `label_shadow="true"`, and all
  three tab positions share one image — Vintage draws no left/middle/right
  distinction.
- `floater.xml`: `header_height="20"` (default 25), `header_vpad="4"`
  (default 5), background images instead of a tinted colour, and
  `bg_*_image_overlay="White"` so the texture's own colour is the floater's.
- `simple_text_editor.xml`: `border_visible="true"` (default false).
- `line_editor.xml`: selection is `LineEditorBgSelectedColor`, not
  `EmphasisColor` — the periwinkle family instead of the brown one.
- `tool_tip.xml`: `max_width="200"`, `padding="4"`, `font="SansSerif"`.
- `progress_bar.xml`: the track tint goes white, so the art shows through.

Vintage does **not** ship a `fonts.xml`; the families are the default skin's.
Font choice is therefore not part of Vintage fidelity — the shadow is.

## What our skin system can and cannot express today

Already expressible as pure token values, no code:

- square corners (`--surface-radius: 0`, `--control-radius: 0`);
- every flat surface and text colour that already has a role token;
- the minimap / name-tag / chat palette, which is already token-driven
  through `skin_colors.rs`.

Expressible with CSS the engine already parses, but with nothing on our side
to attach it to:

- **9-sliced image surfaces.** `bevy_flair` maps `-bevy-image` and
  `-bevy-image-mode: sliced(t r b l)` onto `ImageNode`, which is registered
  `auto_insert_remove` — so a CSS rule can *insert* an image background on a
  node that has none. The reference's `scale.left/top/right/bottom` maps
  straight onto `sliced()`. What is missing is widgets that carry classes to
  target, art to point at, and state expressed in CSS rather than Rust.
- **Text shadows.** `text-shadow` is parsed and maps onto bevy's `TextShadow`.
  No skin uses it and no label carries a class that could.

Not expressible at all today:

- **A light data surface.** There is one `--control-bg` / `--text-primary`
  pair; Vintage needs a *second* family (light field / list surfaces with
  black text) that diverges maximally from the first.
- ~~**A bevel.**~~ Expressible since `viewer-skin-bevel-border-policy`
  (2026-09-24): `bevy_ui` has no inset box-shadow, so it is per-side border
  colours, written by `common.css` from `--button-bevel-top-left` /
  `-bottom-right` and `--field-bevel-top-left` / `-bottom-right`. The side
  properties stay banned in a skin; the light source is physical and does not
  mirror under RTL.
- **Row striping.** Neither `ui_table` nor `virtual_list` alternates a row
  background at all.
- **A skinnable icon set.** Inventory icons are emoji glyphs chosen in Rust
  (`inventory.rs` `item_glyph` / `folder_icon`); Vintage's identity includes
  the legacy icon set.

## Licensing

The measurements above are facts about the files, and are fine to record. The
**art itself is not ours to copy**: Vintage's widget textures and the legacy
`.tga` icon set are Firestorm / Linden Lab artwork under their own terms, not
the LGPL that covers the code. Any task that ships a Vintage-alike skin ships
**art authored for this project** that matches the described geometry and the
measured palette — the same rule the vendored `viewer-assets/character/` tree
follows, and the reason its README records provenance.
