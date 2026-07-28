---
id: viewer-region-options-estate
title: Region / Estate floater — estate tab
topic: viewer
status: in-progress
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-region-options
blocked_by: [viewer-region-options-debug]
---

In progress (2026-07-28): the About Region **Estate**, **Covenant**, and
**Access** tabs are built. Estate access / limit / voice / teleport flags are
editable via the new `Command::SetEstateInfo` (`estatechangeinfo`, typed
`EstateFlags`); the four access lists (managers / allowed / allowed-groups /
banned) use the reusable `ui_table` widget with Add (avatar picker) + per-row
Remove over `UpdateEstateAccess`; covenant + estate identity are read-only (the
estate tab falls back to the covenant reply when `getinfo` is denied to a
non-manager). **Not yet closed:** the write paths are unverified on a live grid
(needs an estate-owner login), and **Allowed Groups → Add** needs a group picker
([[viewer-region-estate-group-picker]]).

Context: [context/viewer.md](../context/viewer.md).

The Region / Estate floater **estate** tab: covenant, access / allowed residents
and groups, estate managers, ban list, and region restart / sun controls — all
driven over the estate `EstateOwnerMessage`. Adds a tab to the floater shell
from [[viewer-region-options-debug]].

Reference (Firestorm, read-only): `llfloaterregioninfo`, `llpanelregion*`,
`llestateinfomodel`; the estate `EstateOwnerMessage`.

Builds on: `protocol-14` estate / region.

Deps: [[viewer-region-options-debug]].
