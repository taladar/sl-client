---
id: viewer-conference-start-ui
title: Start an ad-hoc conference from a multi-selection
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [chat-b2, chat-b5, test-conference-roster,
  viewer-social-people-panel, viewer-radar-multi-select,
  viewer-people-lists-multi-select]
---

Context: [context/viewer.md](../context/viewer.md).

The reference's **Start Conference Chat**: select several friends,
calling cards, or nearby people and open one ad-hoc conference session
with all of them (`llavataractions.cpp` `startConference`; entry points
in menu_inventory.xml on calling cards and
menu_people_nearby_multiselect.xml on the People panel).

Our receive side is done — the conversations panel opens tabs for
incoming ad-hoc sessions (`sl-client-bevy-viewer/src/conversations.rs`)
and conference invites arrived with [[chat-b5]] — and the session-open
machinery exists ([[chat-b2]]), but no UI verb starts a conference with
N invitees. Scope: multi-select in the people / friends lists
([[viewer-social-people-panel]] is single-select today), the Start
Conference context entry (people rows and calling-card inventory rows),
and the N-agent session-open command. Pairs with the
[[test-conference-roster]] live case for verification.

**One consumer is already waiting.** The radar is multi-select as of
[[viewer-radar-multi-select]], and its multi-selection menu's **IM**
entry opens *one direct conversation per selected row* — a stand-in for
the reference's `Avatar.IM`, which starts a conference when handed
several ids. When this task lands, that entry (`radar.rs`, the `"im"`
arm of `handle_radar_actions`) becomes the conference verb for a
selection of more than one, and the module's stated divergence goes
away. The same applies to whatever multi-selection
[[viewer-people-lists-multi-select]] brings to the People panel's
lists.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/menu_inventory.xml`
("Start Conference Chat"), `menu_people_nearby_multiselect.xml`,
`indra/newview/llavataractions.cpp` (`startConference`).
