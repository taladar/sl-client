---
id: viewer-audit-display-name-accessor-sweep
title: Ten display sites call the wire-name accessor, so display names are ignored
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/viewer.md](../context/viewer.md).

`AvatarState::name_of` is documented as "the **grid's** answer, which is what a
wire action (a mute entry) has to carry". The display path is `shown_name_of` /
`label_text`, which resolve through `NameRecord::preferred_name` — alias, then
display name, then legacy.

Display sites calling the wrong one, so both display names **and** the user's
contact-set pseudonyms are silently ignored:

- `sl-viewer-people/src/avatar_profile.rs` (whose own doc says "The display
  name"), feeding the profile's name and partner;
- `sl-viewer-people/src/group_profile.rs` (member roster);
- `sl-viewer-places/src/about_land.rs` (object-owner list);
- `sl-viewer-places/src/about_region.rs`;
- `sl-viewer-places/src/about_landmark.rs`;
- `sl-viewer-edit/src/edit_params.rs`;
- `sl-viewer-inventory/src/inventory_properties.rs`;
- `sl-viewer-pickers/src/avatar_picker.rs`.

`radar.rs` uses `label_text` correctly — so the radar and the friends list in
the same floater disagree about what an avatar is called.

The sweep also deletes six duplicated local helpers with **four different**
fallbacks: `edit_params.rs` (`PENDING_NAME`), `about_landmark.rs` and three
`name_of` closures (`format!("({agent})")`), and `sl-viewer-map/src/minimap.rs`,
which ends `.map(ToOwned::to_owned).unwrap_or_default()`.

## Which accessor a site wants (the rule this sweep applies)

The reference folds a contact-set pseudonym into `LLAvatarNameCache` itself
(`mCustomNameCheckCallback`, `llavatarnamecache.cpp`), so **everything** drawn
from the name cache shows it — the profile included. Here that is
`preferred_name`, reached as `AvatarState::shown_name_of` (`Option<&str>`) or
`AvatarState::label_text` (already-fallen-back `String`).

`name_of` stays for the three things that are not a drawing:

- a name that travels on the **wire** — `RequestBlock` / `Command::Unmute`
  (a mute entry names the muted avatar), `RequestDerender`,
  `RequestRenderException`;
- a name **filed in a store** that must outlive everyone's presence — a
  contact-set entry (`OpenAddToContactSet`);
- a **classification** made from the grid's legacy name — `minimap.rs`'s
  `" Linden"` suffix test, which a pseudonym must not be able to flip;

plus the `is_none()` "has this resolved yet, do I ask?" tests, which are asking
about the grid's answer by definition.

## Fix

Every display site above now reads `label_text` (or `shown_name_of` where the
site already had a meaningful `None` branch), and the six duplicated helpers are
gone — `avatar_profile`'s `BuildContext::name_of`, `group_profile`'s and
`about_land`'s and `about_region`'s `name_of`, `about_landmark`'s `agent_label`
and `inventory_properties`'s `name_of` closure — so one unresolved-name fallback
(`label_text`'s leading id fragment, the same one a name tag shows) is left
where there were four. `edit_params`'s `agent_label` survives as the wrapper
that *requests* the name, but its text now comes from `label_text`;
`PENDING_NAME` stays for the land-impact cell that also uses it.

Three sites the audit did not list, found by sweeping every `.name_of(` in the
viewer crates, had the same defect and are fixed with it:

- `FriendsModel::roster` (`sl-viewer-world-api`) — the avatar picker's Friends
  tab, which ignored the alias while `FriendsModel::rows` (the People pane)
  applied it, so the two lists disagreed;
- `people.rs`'s friend online/offline notification and the rights-grant
  confirmation prompt (`FriendsModel::shown_name_of`, which this promotes from
  `pub(crate)` to `pub` for them);
- `hover_tooltip.rs`'s object-owner line, and `beacons.rs`'s tracked-avatar
  beacon label.

Two of the audit's claims were already stale and are **not** changes here:

- `minimap.rs`'s `agent_label` no longer labels the context menu (that is
  `menu_agent_labels`, which resolves through `shown_name_of` and shows a
  translated `(loading)` while a name is outstanding). What is left of it feeds
  the contact-set entry, where the grid name is what a record wants;
- `about_landmark.rs`'s and `avatar_profile.rs`'s `is_none()` calls are
  resolution tests, not labels.

`NameRecord::grid_name`'s doc no longer claims a profile is one of its callers.

## Verify

Unit, one per defect class and crate:

- `people.rs::every_friend_surface_draws_the_alias` — the roster, the rows and
  `shown_name_of` all show a pseudonym given after the name resolved, while
  `name_of` still answers the grid's name (what a mute entry carries);
- `group_profile.rs::members_sort_under_the_name_the_row_shows` — the member
  roster's sort key follows legacy → display name → pseudonym, so a row never
  sorts under a name nobody can see;
- `edit_params.rs::the_creator_line_shows_the_display_name` — the build
  floater's creator/owner line follows the same three, and queues exactly one
  name request for an unresolved agent;
- `about_landmark.rs::the_owner_row_shows_the_display_name` — the parcel-owner
  row follows the same three.

Live: not required — every changed site is a pure text substitution off the same
cache, and a live grid would only be exercising `AvatarState`'s name merge,
which is already pinned in `avatars.rs`.
