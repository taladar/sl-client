---
id: viewer-groups-list-keeps-a-group-the-agent-was-ejected-from
title: The Groups list keeps a group the agent was ejected from on Second Life
topic: viewer
status: done
origin: gridspec-groups (2026-10-14)
refs: [gridspec-groups]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

Measured by the `groups` census (`book/src/gridspec/groups.md` § Ejection):
Second Life tells an ejected member nothing structured — no `AgentDropGroup`,
no new `AgentGroupDataUpdate`, not at once and not within 20 s — only an
instant message from the ejecter, "You have been ejected from '<group>' by
<name>." OpenSim sends `AgentDropGroup` and a fresh membership list.

The Groups list (`sl-viewer-people/src/groups.rs`, `GroupsModel`) only
drops a group on `DroppedFromGroup` / `AgentDroppedFromGroup` or a new
membership list, so on Second Life an ejected agent keeps the group in its
list (and as its active group, where it was) until the next login. The
reference viewer keeps it the same way (`gAgent.mGroups` changes only on
`AgentGroupDataUpdate`).

## Done

The Groups list asks for the memberships again
(`Command::RequestAgentDataUpdate`) each time it comes into view — the
conversations floater opened on it, the Groups sub-tab chosen, the menu's
Groups entry — which it tells from its viewport going from no laid-out size
to some (`refresh_memberships_when_shown`, `sl-viewer-people/src/groups.rs`).
The answer replaces the whole list and the active group, so an ejected group
drops out the next time the list is looked at. The message text is not
parsed. A list already open when the ejection happens keeps the group until
it is shown again: nothing the grid sends marks that moment.
