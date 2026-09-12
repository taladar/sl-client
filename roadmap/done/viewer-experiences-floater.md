---
id: viewer-experiences-floater
title: Experiences floater — lists, profile, search
topic: viewer
status: done
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold, viewer-ui-virtualized-list]
refs: [viewer-experience-permission-dialog, viewer-region-experiences-panel,
  protocol-experience-search-paging,
  viewer-experience-profile-extended-metadata]
---

Context: [context/viewer.md](../context/viewer.md).

The experiences UI over the fully-implemented experience protocol
(`protocol-27` + the caps pairing `protocol-62`):

- **My experiences**: allowed / blocked lists with per-row revoke ("forget"
  / unblock), and the experiences the agent contributes to / owns.
- **Experience profile**: name, description, maturity, owner/group, slurl,
  the permission actions (allow / block / forget), and — for owned ones —
  the editable fields the caps expose.
- **Search**: find experiences by name (the experience-search cap), rows
  opening the profile.
- The events log tab (recent experience permission events) as the reference
  ships.

The in-the-moment grant dialog is separate
([[viewer-experience-permission-dialog]]); this floater is the management
surface it links out to.

Reference (Firestorm, read-only): `llfloaterexperiences`,
`llfloaterexperienceprofile`, `llpanelexperiences`,
`floater_experience_search.xml`.

Builds on: `protocol-27` / `protocol-62` experience surface.

## Parity-audit addendum (2026-08-19)

Parity-audit status update: the allowed/blocked lists, forget action,
name resolution, and the top-menu entry are ALREADY IMPLEMENTED
(`sl-viewer-notices/src/experiences_floater.rs`). The remaining
scope of this task is the experience **profile panel**, **search**,
and the **contributor / owned lists**.

## Events log: done, and what it left behind (2026-09-11)

The **events-log tab** is done — [[viewer-experience-event-stream]] landed the
`ExperienceEvent` ingest, the per-account log, its notifications, and a Recent
events section in this floater.

It also inherited this floater's list mechanism, which is the one the
build-once-update-in-place rule argues against: all three
columns (allowed, blocked, events) despawn their rows and respawn them when a
revision moves, rather than binding a pooled `ui_table` / `VirtualList` in
place. Three despawn-rebuilding columns in one window is more churn than the two
that were here before, and the events one is the one that grows without an upper
bound on rows. Converting **all three** together — one mechanism per window, not
two — belongs with whichever of the panels above is built first, since the
contributor / owned lists want the same widget.

## What landed (2026-09-11)

The window is now the reference's **seven tabs** in the reference's order —
search, Allowed, Blocked, Admin, Contributor, Owned, Recent events — and the
profile is its own keyed floater.

- **Every list is one mechanism.** All seven are `ui_table` over
  `virtual_list`, with a single `populate` / `bind` pair driving all of them:
  the pane is a `PaneViewport` component on the viewport, so a recycled row
  knows which list it belongs to without a system per tab. The despawn-rebuild
  the section above argues against is gone, including from the events column
  that motivated it. Each table sorts and persists its own sort / widths, and
  the consumer re-sorts its own rows when the widget's sort revision moves
  (the split `ui_table` documents).
- **The per-row Forget moved into the tab's action row**, which is where the
  reference has it (`LLPanelExperiences`'s `button_panel`, the picker's
  `profile_btn`) and the only place it can be with pooled rows: a row is a
  *view* of an item, so an action bound into one would be rebound on every
  scroll. Select, then act. Only the two **preference** tabs offer it; Admin /
  Contributor / Owned are relationships, not preferences, and have nothing to
  forget.
- **Search** is the picker panel: the query field (Enter or Find), a
  persisted max-rating filter, the rating / name / owner results table, Profile
  and the two paging arrows. The rating filter *hides* rows rather than
  re-querying, as the reference's `filterContent` does — the cap has no rating
  parameter — and it persists the **rating code** rather than the combo index
  the reference's `ExperienceSearchMaturity` stores, so reordering the list
  cannot silently change a saved filter. Paging was a heuristic until
  [[protocol-experience-search-paging]] landed; the arrows now follow the
  grid's own `next_page_url` / `previous_page_url` markers.
- **The profile is a keyed floater**
  (`sl-viewer-notices/src/experience_profile.rs`, `"experience-profile"`,
  keyed by experience id) — the reference's
  `LLFloaterReg::showInstance("experience_profile", id)`, and one more entry
  for [[viewer-keyed-floater-audit]]'s ledger. It shows the name, description,
  rating, owner (a `ui_name_link`, so it opens the owner's profile),
  home-location SLURL, grid-vs-land scope and the privileged note; offers
  Allow / Forget / Block with the current preference greyed; and, to an
  administrator (`IsExperienceAdmin`), an edit column over `UpdateExperience`.
  A **privileged** experience shows the note *instead of* the three buttons —
  the agent cannot decline it, and the reference does not even ask for the
  preference in that case.
- **The edit path preserves what it does not show.** `extended_metadata` (the
  reference's marketplace link and logo, LLSD-XML this viewer has no decoder
  for) is sent back verbatim, and every property bit but the two the toggles
  own is carried through — so an administrator renaming an experience cannot
  silently delete their own store link. What is *not* built is
  [[viewer-experience-profile-extended-metadata]], which also carries the
  group re-assignment and a note about the Owned tab's acquire button.
- New settings: `ExperienceSearchMaturity` (per account, in the `experiences`
  section) plus the seven tables' sort / widths keys, registered at runtime the
  way the block list's are.

### The tab strip runs down the side, and the sweeps are why

The reference's strip is a horizontal row. Seven of these labels are not: at
scale 1 in Latin they overflow the window by 16 logical px, and the layout
sweeps also caught the consequence — the strip clips, and a clipped horizontal
strip **slices a label mid-glyph**, which
`every_floater_survives_every_script` reports as "neither fully visible nor
fully hidden". Cyrillic overflows by 23, Devanagari by 114, and every one of
those multiplies with the UI scale.

The reference's own answer is `LLFloaterExperiences::resizeToTabs()` — widen the
*window* until the tabs fit. That cannot be right across scripts and scales; it
would make the window wider than a laptop screen in Devanagari at 2×. So the
strip is `TabPlacement::InlineStart` with a fixed `strip_width`, which is what
turns on the tab widget's **label truncation** (each label ellipsised in the
locale's own ellipsis) and its draggable divider. Preferences already does this,
for the same reason and with the same widget.

Two smaller things the same sweeps forced, both worth keeping in mind for the
next window built this way:

- **A specimen must be narrower than the window it stands for.** The floater
  sweep spawns a `FloaterContent::Specimen` into the window's content *slot*,
  and that slot carries 8 px of padding — so a specimen sized to the declared
  content width overflows it by exactly 16 logical px at every scale. That
  constant 16 was the first sweep failure and it survived widening the window
  *and* re-orienting the tab strip, because neither was where it came from:
  every one of those "floater `experiences`" failures was about the specimen,
  not the live window. The specimen now has its own width with the padding
  subtracted, and says so.
- A table inside a scrolling tab panel resolves to **zero** height on
  `flex_grow` alone, so it needs a `min_height` floor — and a layout sweep
  passes happily on a zero-height widget, so nothing else would have said so.

Still open from the original scope: nothing. The contributor / owned lists,
the profile and the search are all here; the two carve-outs above are filed.
