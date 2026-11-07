---
id: server-fake-grid-friend-edit-rights
title: Fake grid — a friend's edit right lets it edit the granter's objects
topic: server
status: blocked
origin: server-fake-grid-agent-avatars-shared (2026-11-07)
refs: [gridspec-friend-edit-rights]
blocked_by: [gridspec-friend-edit-rights]
---

Context: [context/server.md](../context/server.md).

The fake grid keeps friends' rights (`GrantUserRights`) but never acts on the
edit right: a friend holding it is sent a stranger's update flags for the
granter's prims and refused every edit. Build what
[[gridspec-friend-edit-rights]] measures, per flavour, and take the
`Need::LiveGrid` off `e2e_two_avatars`'
`two_residents_befriend_trade_rights_and_contest_one_prim`, whose contested prim
is the first thing that waits on it.
