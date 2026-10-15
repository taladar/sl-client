---
id: viewer-floater-update-in-place
title: Detail/property floaters — update values in place, don't rebuild structure
topic: viewer
status: ready
origin: user request (2026-07-27), while live-testing viewer-social-group-profile
  (a same-frame despawn/insert race crashed the avatar profile on release)
refs: [viewer-social-group-profile, viewer-social-profiles]
---

Context: [context/viewer.md](../context/viewer.md).

Several **detail / property** floaters rebuild their widgets by tearing the
whole panel down (`despawn_children`) and respawning it from state every time an
input changes — even though the panel's **structure does not change**, only the
values do. This is wasteful, drops focus / caret / selection mid-edit, and (the
reason this was filed) causes **same-frame despawn+insert races**: when several
async replies land in one release-speed frame, a widget is spawned and despawned
again before another system's queued `insert` on it flushes, panicking with
"Entity despawned". That crash is currently defused with `try_insert` at the
insert sites (the shared caret installer `ui_text_input::install_caret_style`,
and the picture/insignia/browser image inserts in `avatar_profile`,
`group_profile`, `browser_widget`) — a correct safety net, but it papers over
the churn rather than removing it.

**Audit 2026-10-15 — partly done.** The profile half landed with the group
profile itself (`450f9e36`): in `sl-viewer-people/src/avatar_profile.rs`
`rebuild_profile_tabs` keeps the 2nd Life tab retained (structure once per
subject, `update_second_life` in place, group rows reconciled), and the five
other tabs rebuild only when their `tab_signature` changes, so the reply
burst no longer respawns them; in `sl-viewer-people/src/group_profile.rs`
the General tab (`build_general_structure` / `update_general_values`) and
the roles list (`sync_roles_view`) update in place. `ui_texture_picker`'s
`rebuild_tree` now skips a rebuild when the visible tree did not change.
Still rebuilding wholesale: the group profile's `rebuild_details_area` /
`rebuild_compose_area` / `rebuild_notice_body`, `inventory_properties`
(tears its content down and respawns on every re-open of an open window),
`edit_contents` (`rebuild_contents_views` / `rebuild_one_view`), and the
avatar profile's signature-gated tabs (a changed pick / note still
respawns the tab). The bullets below are the original survey.

Convert these panels to **build the structure once and update values in place**,
rebuilding only on a genuine structural change (own-vs-other, a selected
pick/classified/role/member/notice changing, a folder changing):

- **`avatar_profile`** (`rebuild_profile_tabs`) — the worst offender: rebuilds a
  tab on *every* streaming reply (properties, then groups, then partner-name,
  then picks…). Needs a programmatic text/value-set path (the module doc calls
  out that it avoided one by rebuilding), plus persistent field handles.
- **`group_profile`** (`rebuild_general_tab` / `rebuild_roles_list` /
  `rebuild_details_area` / `rebuild_compose_area` / `rebuild_notice_body`) —
  same reply-driven churn (profile, then roles, then role-members…).
- **`inventory_properties`** — the "picker-list pattern" the two above copied;
  rebuilds on selection.
- **`edit_contents`** (`rebuild_contents_views` / `rebuild_one_view`) — prim
  contents.
- **`ui_texture_picker`** (`rebuild_tree`) — the folder tree.

**Not in scope / already correct:** the *list* surfaces — Friends (`people`),
the Groups list (`groups`), Inventory (`inventory`), the inventory gallery, the
emoji picker, the avatar picker — already use the good model: a **virtualized**
pool of persistent row entities whose bind systems only re-set values; their
"rebuild" recomputes a data projection, not entities. Leave them.

Keep the `try_insert` safety net regardless (a system inserting onto an
`Added<…>`/queried entity should always tolerate a concurrent despawn); this
task is about removing the needless churn, not the net.
