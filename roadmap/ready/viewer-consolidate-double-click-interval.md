---
id: viewer-consolidate-double-click-interval
title: Consolidate the per-widget double-click interval constants
topic: viewer
status: ready
origin: user request (2026-07-27), while adding the avatar-profile group list
refs: [viewer-social-groups, viewer-avatar-profile-group-list]
---

Context: [context/viewer.md](../context/viewer.md).

Several lists have grown their **own** double-click-interval constant, all the
same 0.4 s:

- `groups.rs` `DOUBLE_CLICK_SECS` (the People-pane group list).
- `avatar_profile.rs` `GROUP_DOUBLE_CLICK_SECS` (the 2nd-Life group list).
- (audit for others — friends list, inventory, etc. — and fold them in too.)

Replace them with a **single** source of truth: either one shared const in a
common UI module (e.g. `crate::ui`), or — better — a **preference** (user-facing
options belong in the preferences floater, not per-site consts), so the
double-click speed is tunable like a real viewer's. Wire every double-click site
to it.
