---
id: viewer-groups-pane-empty-on-opensim
title: The Groups pane lists nothing while the profile's group list has the same groups
topic: viewer
status: bugs
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

## The two paths

- The **pane** ([`groups`](../../sl-viewer-people/src/groups.rs)) is fed by
  `GroupsModel::apply_memberships`, from `SlSessionEvent::GroupMemberships` —
  the grid's `AgentGroupDataUpdate` **push**. Both deliveries are decoded:
  the modern CAPS event-queue one (`group_memberships_from_caps_llsd`,
  `session/methods.rs` `"AgentGroupDataUpdate"`) and the legacy UDP message
  (`AnyMessage::AgentGroupDataUpdate`). Nothing *requests* it — it is a push.
- The **profile's** list comes from the profile fetch's `AvatarGroupsReply`
  (`SlSessionEvent::AvatarGroups`), which the profile floater asks for by
  agent id when it opens.

So the pane depends on a push the local grid apparently never sends, and the
profile depends on a request/reply that works.

## What to check first

1. Whether OpenSim sends `AgentGroupDataUpdate` at all for this account —
   watch the UDP stream and the event queue (the CAPS queue *is* alive here:
   the session log shows `event queue: polling started
   url=http://127.0.0.1:9000/CE/…`). If it never arrives, the fix is for the
   pane to seed itself the way the reference does rather than waiting for a
   push.
2. What the **login response** carries: SL's `login.cgi` reply has a
   `groups`/`group-memberships` array, and the reference seeds its group list
   from it before any push. If we drop that, the pane starts empty on *any*
   grid until a membership changes — and on SL the push at login would be
   hiding the same gap.
3. Whether this also reproduces on **aditi** (open the Groups pane right after
   login). If it does, this is not an OpenSim quirk at all but a missing seed.

## Why it matters beyond the pane

The Groups pane's **Info** button is the only advertised way into the group
profile floater, so an empty pane makes that window unreachable except through
the avatar profile's group list (double-click a row) — which is how
[[viewer-keyed-floater-audit]]'s group-profile conversion had to be checked.

Reference (Firestorm, read-only): `llpanelgroups` / `LLAgent::mGroups`
(seeded from the login response, then updated by `AgentGroupDataUpdate`).
