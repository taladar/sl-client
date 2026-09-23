---
id: viewer-skin-text-colours-without-a-role
title: The text colours that name no role, and the four kinds of reason why
topic: viewer
status: in-progress
origin: viewer-skin-panel-text-roles census (2026-09-22)
points: 5
refs: [viewer-skin-panel-text-roles, viewer-audit-skin-token-coverage,
  viewer-i18n-colorblind-accessibility]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-skin-panel-text-roles]] made a panel's text skinnable by reading the
**role** back out of the colour it names: `skin::role_class` maps
`FALLBACK.text_primary` / `.text_muted` / `.text_heading` / `.text_disabled`
onto `.sk-text` / `.sk-title` / `.sk-heading` / `.sk-disabled-text`, and
`text_role(colour)` is the spawn-site form. A colour matching no role gets an
**empty** class list and goes on painting itself — deliberately, because a
panel with a colour of its own has not said which role it means and guessing
would be worse than leaving it.

This is the bill for that. **64 spawn sites over 22 constants** resolve to no
role today, plus **32 `Color::WHITE`** literals. Every one is already calling
`text_role`, so each fix is a one-line change to a *constant* — the call sites
need nothing.

Enumerate them with the census that found them:

```sh
# what every text spawn passes, ranked
ast-grep run -p 'TextColor($C)' -l rust --json=compact sl-viewer-* | …
# the ones that resolve to no role
rg -o 'text_role\([A-Z_]+\)' -g '*.rs' --no-filename | sort | uniq -c | sort -rn
```

## Four kinds, and they want different answers

### 1. A neutral between primary and muted (~14 constants, ~40 sites)

`CHROME_COLOR` (9), `SECTION_COLOR` (8), `VALUE_COLOR` (6), `HEADER_COLOR`
(5), `DEMO_TEXT_COLOR` (5), `BAR_LABEL` (3), `STATUS_COLOR`, `PREVIEW_COLOR`,
`GHOST_COLOR`, `DETAIL`, `CLOSE_GLYPH_COLOR`, `SECONDARY_COLOR`,
`DEMO_TITLE_COLOR`, `BODY_COLOR` — a spread of blue-greys scattered between
`text_primary` (0.90, 0.92, 0.96) and `text_muted` (0.62, 0.66, 0.74), none
within the 0.045 the earlier collapse used.

The real question is whether the palette wants a **third neutral** — a
"secondary" between primary and muted — or whether these are simply drift at a
wider tolerance and each belongs to one of the two that exist. Decide that
once; the fourteen follow from it.

### 2. Meaning-bearing, wanting a token of their own (5 constants)

`NOTE_COLOR` (gold), `HEADING_COLOR` (green), `ERROR_COLOR` (red), `WARN`
(amber), `CHECK_COLOR` (green). The same argument as `--gain` / `--loss`, the
four `--notice-*` kinds and the `--console-*` pair: the distinction has to
survive a skin, and a colour-blind overlay is exactly what retunes it. Follow
that precedent — a token each and a compound class over `.sk-text`.

### 3. The accent, spelled out longhand (2 constants)

`TAB_ACTIVE_BORDER` and `GROUP_LINK_COLOR` are both `srgb(0.52, 0.68, 0.95)`,
which is the accent. `.sk-active-text` already exists for it.
(`TAB_ACTIVE_BORDER` on *text* is also a naming smell worth following:
[[viewer-ui-tab-widget-dynamic-tabs]] owns that strip.)

### 4. Equidistant, so a judgement (1 constant)

`BAR_LABEL_DIM` has two definitions, `srgb(0.45, 0.45, 0.5)` and
`srgb(0.62, 0.65, 0.72)` — the first near `text_disabled`, the second near
`text_muted`. They are not the same idea and should not share a name.

## The 32 `Color::WHITE` literals

All 32 spawn-site literals are `Color::WHITE`, and reading them shows **three**
intents behind the one expression — which is why this is a decision list and
not a sweep:

- **Plain panel text** — `ui_element.rs`'s specimen prose and labels. The only
  kind that wants `text_role`.
- **An untinted glyph** — `emoji_picker.rs`'s cells and tone swatches. White
  *is* "do not tint" for a colour emoji; a role would tint it.
- **A deliberate skinless fallback** — `edit_tool.rs` and siblings, which say
  so at the site ("the skin recolours via the class token"). Already
  class-driven, and the white is what a stylesheet-less world falls back to.
  Exactly what `text_role` is built to leave alone.

## Not in scope

Name tags look like residue and are not: `NAME_TAG_MISMATCH` is precisely
`--name-tag-mismatch`'s fallback value, and the whole family is already
skinnable through the user-tunable palette rather than through classes.

## Done when

Every `text_role` call resolves to a role or is documented at the site as one
of the three deliberate `Color::WHITE` cases, and the census command above
returns nothing unexplained.

## Done (2026-09-23)

**Two neutrals, not three** — decided from the code rather than from taste.
Seven files looked like they used three neutral levels at once; every one
decomposed into something else: a section *heading* (`text_heading`), the
*accent* used as text, a *meaning*, or chrome that is simply muted. No panel
needs a third level of prose, so the 14 drifting constants collapse into the
four roles that exist.

What that came to:

- **28 constants across 21 files** now name a role. The bright band (~.86) is
  `text_primary`, the mid band (~.76) is `text_muted`, section and table
  headers are `text_heading`. `BAR_LABEL_DIM`'s two definitions split — `.45`
  was the *refused* label (disabled), `.62` a resting one (muted) — which is
  what "they are not the same idea" meant.
- **Meaning-bearing colours got tokens**, on the `--console-*` precedent:
  `--text-error`, `--text-warn`, `--text-note`, `--experience-accent`, with
  `.sk-text.sk-error` and friends, and a `skin::text_meaning(colour, class)`
  helper beside `text_role`. Ten sites.
- The two constants that **were the accent** spelled longhand now carry
  `ACTIVE_TEXT_CLASS`.
- The **32 `Color::WHITE`** are five: three untinted colour emoji (white *is*
  "do not tint"), and the two world overlays, whose literal is exactly what
  `--overlay-text` holds. Everything else became a role or the colour its own
  class paints.

### Two things the census turned up that were not on the list

**The radar was copying the name-tag palette's fallback values.** A user who
retuned `NameTagColorFriend` saw it over avatars' heads and not in the radar,
and nothing said the two were meant to agree. It reads the store now
(`setting_color`), and its jellied-complexity cell — which was borrowing the
muted-*avatar* colour to dim a *measurement* — takes the muted role.

**World overlays are skinnable now too.** A beacon's label and the pipeline
read-out were white by fiat, justified as "read against scenery, not chrome".
That is an argument for a different *token*, not for none: they carry
`.sk-overlay` / `.sk-overlay-text` (`--overlay-bg` / `--overlay-text`), and
being classes rather than bare tokens a skin can set their font as well as
their colour. The pipeline read-out had to be **parented to the UI root**
first: it was spawned with no parent at all, outside the tree bevy_flair
styles, so a class on it would have resolved to nothing.

### And a correction worth keeping

"A skinless fallback" was the wrong name for what a `TextColor` beside a class
is. `common.css` is an embedded asset and the UI root is always `Styled`, so a
running viewer never goes without a stylesheet. What the Rust colour still
decides is what the **headless harnesses** measure — the testkit builds the
layout stack without `FlairPlugin`, so no class resolves there — and the frame
before the sheet loads. Both want the styled answer, which is why those sites
now pass the colour their class paints rather than white.
