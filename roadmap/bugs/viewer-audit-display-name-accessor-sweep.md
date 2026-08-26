---
id: viewer-audit-display-name-accessor-sweep
title: Ten display sites call the wire-name accessor, so display names are ignored
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/viewer.md](../context/viewer.md).

`AvatarState::name_of` (`sl-viewer-world-api/src/lib.rs:3544`) is documented as
"the **grid's** answer, which is what a wire action (a mute entry) has to
carry". The display path is `shown_name_of` (`:3582`) / `label_text` (`:3515`),
which resolve through `NameRecord::preferred_name` (`:3191`) — alias, then
display name, then legacy.

Display sites calling the wrong one, so both display names **and** the user's
contact-set pseudonyms are silently ignored:

- `sl-viewer-people/src/avatar_profile.rs:1031` (whose own doc says "The display
  name"), feeding the profile's name and partner at `:1342` and `:1345`;
- `sl-viewer-people/src/group_profile.rs:1481` and `:3312` (member roster);
- `sl-viewer-places/src/about_land.rs:2869` (object-owner list);
- `sl-viewer-places/src/about_region.rs:2592`;
- `sl-viewer-places/src/about_landmark.rs:895`;
- `sl-viewer-edit/src/edit_params.rs:2122`;
- `sl-viewer-inventory/src/inventory_properties.rs:367`;
- `sl-viewer-pickers/src/avatar_picker.rs:689`.

`radar.rs:1189` and `:1374` use `label_text` correctly — so the radar and the
friends list in the same floater disagree about what an avatar is called.

The sweep also deletes six duplicated local helpers with **four different**
fallbacks: `edit_params.rs:2119` (`PENDING_NAME`), `about_landmark.rs:893` and
three `name_of` closures (`format!("({agent})")`), and
`sl-viewer-map/src/minimap.rs:3238`, which ends
`.map(ToOwned::to_owned).unwrap_or_default()` — so right-clicking an
unresolved avatar yields **blank** context-menu entries.
