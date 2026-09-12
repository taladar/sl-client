---
id: viewer-groups-pane-empty-on-opensim
title: The Groups pane lists nothing while the profile's group list has the same groups
topic: viewer
status: done
origin: seen on the local OpenSim grid while live-checking the keyed group
  profile (2026-09-06)
refs: [viewer-social-group-profile, viewer-keyed-floater-audit]
---

Context: [context/viewer.md](../context/viewer.md).

On the local OpenSim grid (Groups V2 wired up per
`sl-client-opensim-groups-v2-setup`) the **Groups pane** in the Conversations
window is empty, while the **avatar profile's 2nd Life tab lists the agent's
groups** in the same session. So the memberships are known to the grid and
reach the viewer by one path and not the other.

## What it turned out to be

Not a missing push, and nothing to do with OpenSim. `sl-repl-tokio` logging
into the local grid receives `Event::GroupMemberships` with all 33 memberships
**0.5 s after the login reply**, decoded from the CAPS event queue — the
protocol half works, on the push at `OnMakeRoot` and again on every
`RequestAgentDataUpdate`. (Worth recording for its own sake: OpenSim's login
response carries no group array — only `max-agent-groups` — and neither does
Second Life's, so there is no login seed to be missing. The reference does not
seed `LLAgent::mGroups` from the login reply either.)

The defect is in the viewer, in `rebuild_groups_view`
([`groups`](../../sl-viewer-people/src/groups.rs)): it stamped
`view.built_revision` and rebuilt `view.rows` **before** checking whether the
pane existed, then wrote `VirtualList::item_count` only if it did. The
memberships are a single push landing within a frame or two of login, while the
group list is two deferred spawns behind it — the People pane waits for the
conversations strip, and the group list waits for the People pane, each an
extra frame because the parent is inserted through `Commands`. Lose that race
and the revision is already stamped when the list appears, so the early-out
fires forever: the widget stays at zero rows for the whole session, with no
second push to rebuild from. `refresh_groups` had the same shape for the count
line (and never relocalised it on a locale switch).

`rebuild_friends_view` in [`people`](../../sl-viewer-people/src/people.rs) was
written the same way. The Friends list usually gets away with it because name
resolution keeps bumping the model's revision for seconds after login, so a
late pane self-heals — but the race is the same one and it is fixed alongside.

Every other list of this shape (`blocked`, `radar`, `contact_sets_panel`,
`group_profile`, `asset_blacklist`, `avatar_render_floater`) returns early on
an absent UI **before** stamping, and so was already correct.

## Fix

Both rebuilds now re-size the list whenever its `item_count` disagrees with the
view, not only on the revision edge — a pane that appeared after the push
adopts the rows already built, leaving the scroll where the user put it. The
count line is refreshed on `ui.is_added()` and on `translator.changed()` as
well as on a model change. Regression tests in both modules drive the exact
order: model first, pane second, then assert the list is not empty. Each half
of the fix was disabled in turn to confirm its test really fails without it —
the list stays at `0` rows, the count line stays `""`.

Live-verified on the local OpenSim grid: the Groups pane lists the memberships
and its Info / IM / Activate / Leave buttons all behave.

## Why it mattered beyond the pane

The Groups pane's **Info** button is the only advertised way into the group
profile floater, so an empty pane made that window unreachable except through
the avatar profile's group list (double-click a row) — which is how
[[viewer-keyed-floater-audit]]'s group-profile conversion had to be checked.

Reference (Firestorm, read-only): `llpanelgroups` / `LLAgent::mGroups`
(updated by `AgentGroupDataUpdate`).
