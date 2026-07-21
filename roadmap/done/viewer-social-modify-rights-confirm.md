---
id: viewer-social-modify-rights-confirm
title: Confirm before granting a friend edit-my-objects rights
topic: viewer
status: done
origin: follow-up from viewer-social-people-panel (2026-07-21)
refs: [viewer-social-people-panel, viewer-ui-notification-host]
---

Context: [context/viewer.md](../context/viewer.md).

The People / Contacts Friends table ([[viewer-social-people-panel]]) lets you
toggle the rights you grant a friend by clicking the "They can …" checkboxes,
sending `GrantUserRights`. The **edit-my-objects** right
(`FriendRights::CAN_MODIFY_OBJECTS`) is far more dangerous than see-online /
see-on-map — it lets the friend edit, delete or take your rezzed objects — so
the reference viewer pops a **confirmation dialog** before granting it (and a
notice when it is revoked). We currently toggle it directly with no prompt.

This task adds that gate: when the user clicks the edit-objects checkbox **to
grant** it, show a confirm dialog naming the friend and the consequence; only
send `GrantUserRights` (and flip the checkbox) on confirm. **Revoking** it, and
toggling the two harmless rights, stay immediate. The dialog should reuse the
viewer's notification / modal host ([[viewer-ui-notification-host]]) rather than
a bespoke popup, so it matches every other confirm.

Reference (Firestorm, read-only): the `GrantModifyRights` /
`RevokeModifyRights` notifications (`notifications.xml`), raised from
`LLPanelPeople` / the relationship-rights change path.

Builds on: the friend-rights toggle in `people.rs` (the click observer
`on_toggle_right`, which today grants unconditionally).

## Done (2026-07-21)

Implemented in `people.rs`. Clicking a **They can … / edit-objects** checkbox to
**grant** it no longer sends `GrantUserRights`; instead it stages a
`PendingGrantConfirm { friend, rights }` and opens a **modal confirm** — a
full-window scrim (blocking the click behind it) over a warning-bordered box
naming the friend ("Give _name_ permission to edit, delete or take your
objects?") with **Cancel** / **Grant**. Only **Grant** applies the toggle +
sends the command; Cancel clears it. Revoking edit-objects, and toggling
see-online / see-on-map either way, stay immediate (`on_toggle_right` gates only
on `kind == Edit && !current.can_modify_objects()`). The modal sits at a very
high `GlobalZIndex` so it is never occluded by a raised floater.

**Bespoke, pending the host:** [[viewer-ui-notification-host]] is not built yet,
so this is a self-contained modal in `people.rs` (resource +
`spawn_grant_confirm_modal` + `drive_grant_confirm`). When the notification /
modal host lands, migrate this confirm onto it (matching the reference's
`GrantModifyRights` notification) and drop the bespoke overlay.

**Live-verify still pending:** the gating logic is unit-tested, but the
on-screen modal only appears when you click an empty edit-objects checkbox,
which needs a **friend in the list** — verify it live (dialog shows, Cancel is a
no-op, Grant sends `GrantUserRights`) once the test account has a friend.
