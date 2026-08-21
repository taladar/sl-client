---
id: viewer-conference-start-ui
title: Start an ad-hoc conference from a multi-selection
topic: viewer
status: in-progress
origin: Firestorm full-parity audit (2026-08-19)
refs: [chat-b2, chat-b5, test-conference-roster,
  viewer-social-people-panel, viewer-radar-multi-select,
  viewer-people-lists-multi-select, viewer-avatar-picker-search-finds-nothing,
  viewer-im-tab-shows-short-id-not-name, viewer-add-friend-offers-silently]
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

**The picker is ready.** [[viewer-avatar-picker-multi-pick]] landed, so the
shared avatar picker already opens in a multi mode
(`OpenAvatarPicker::many`) and answers with the whole list — this task invites
N from that reply rather than growing a picker of its own.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/menu_inventory.xml`
("Start Conference Chat"), `menu_people_nearby_multiselect.xml`,
`indra/newview/llavataractions.cpp` (`startConference`).

## Built (2026-08-21) — live verification pending

One verb, five doors. `conversations::StartConference` is the whole feature's
front: a list of residents and, optionally, the conference to put them in. Every
surface that has several avatars selected writes it and stops thinking about
conferences — the radar's multi-row **IM**, the People panel's Friends **IM**
button, the inventory's **Start Conference Chat**, and the new
add-participants ✚ on a conversation pane.

The handler does the counting the reference does at each call site
(`llpanelpeople.cpp:1514`: one id is `startIM`, several are `startConference`),
so no caller branches: our own agent and any repeat are dropped, **nobody** is
nothing, **one** resident is a plain IM tab, **several** are one conference.
Adding to a conference that is already open keeps its id, so one resident is
fine there. That decision is a pure `conference_plan`, unit-tested away from the
grid.

**The session id we mint is temporary, and that turned out to be the
load-bearing part.** Second Life gives an ad-hoc session a *different* id than
the client's and says so with a `ChatterBoxSessionStartReply` on the event queue
— which we did not read, so a conference started against SL would have talked
into a session the simulator does not know while its replies opened a second
tab. So this task grew a protocol half:

- `Event::ChatSessionStarted` and the `ChatterBoxSessionStartReply` decode.
  `Session` re-keys its own registry entry from the temporary id onto the real
  one before surfacing the event — merging into the entry an invitation may
  already have made, since the invite to our own conference can outrun the
  reply — and drops the session outright on `success: false`.
- The modern start path: `CHAT_SESSION_START_CONFERENCE` (`"start conference"`)
  over `ChatSessionRequest`, which **both** runtimes now prefer when the region
  publishes the cap, falling back to the deprecated
  `IM_SESSION_CONFERENCE_START` instant message otherwise (OpenSim, which has no
  such cap and no ad-hoc conferences at all). `Session::open_conference` is the
  pure half the cap path needs, mirroring how the accept / decline pair is
  split.
- Adding people to a conference that is already open turned out to be a
  *different* request, not a repeat start: `Command::InviteToChatSession`, the
  cap's `invite` method (`LLFloaterIMSession::inviteToSession`). It names a
  real session, so no start reply follows; without the cap it falls back to
  re-sending the conference-start IM, the only invite the legacy path has.
- The server direction, so the pair stays whole: the cap's `"start conference"`
  arm in `sim_caps`, `SimSession::chat_session_start_conference` (same registry,
  same `ConferenceStartRequested` event as the UDP door), and
  `enqueue_chatterbox_session_start_reply`, plus the `invite` arm and its
  `SessionInviteRequested`. A sim-caps test drives a start through the cap and
  the reply back through a real client; another grows an open session.

The viewer's tab follows the same move: `ConversationModel::rekey` carries the
transcript, unread count and active-ness across, merging if both keys exist. The
tab's press observer captured its key, so a moved conversation is re-spawned
rather than re-labelled — `spawn_conversation_tabs` now also prunes a view whose
conversation the model no longer has, which is what makes that automatic.

**The Friends list is multi-select**, the second consumer of
`TableSelectionMode::Multi` after the radar and the same two-way arrangement:
`SelectedFriend` keeps the answer by `FriendKey` (`mirror_friend_selection`
reads the widget's click through), and `rebuild_friends_view` re-projects it
onto the new row order, dropping anyone whose row went away. The hand-rolled row
highlight went away with it — the widget paints the selection now. The action
bar acts on the whole selection: IM is the conference verb, Offer Teleport is
one message naming everyone (its target was always a list), Block and Remove
Friend loop, and Profile — one avatar's window — opens for the row the selection
leads with.

Two entry points beyond the plan, both the reference's:

- **Add participants** (a ✚ beside a pane's ✕, on a 1:1 or a conference —
  never a group, whose roster is the group). It opens the shared multi picker
  ([[viewer-avatar-picker-multi-pick]], as this task's note predicted) and turns
  the answer into a conference: from a 1:1 that is the peer plus everyone
  picked — and the 1:1 **closes**, since a one-to-one becomes the conference
  rather than sitting beside it — from a conference an invitation into the
  session already open. This is `llfloaterimsession.cpp:573`.
- **Start Conference Chat** on an inventory **folder** of calling cards, not
  only on selected cards — the reference's two `Inventory.BeginIMSession`
  parameters (`"selected"` and `"everyone"`), which are one arm over a different
  set here. Its new `folder-has-calling-cards` condition keeps it off a folder
  of shirts.

Divergences worth naming: the People panel's Friends list has an **action bar**
where the reference has a context menu, so "the Start Conference context entry
on people rows" is that bar's IM button; and the People panel's *other* lists
(blocked, contact sets, groups) are still single-select — that is
[[viewer-people-lists-multi-select]], which now has both this and the radar to
follow.

## Verification: offline done, live blocked

OpenSim implements no ad-hoc conferences at all (its `GroupsMessagingModule`
handles group sessions only), so the whole feature is aditi-only — and aditi
*does* have the three avatars it needs (`primary`, `secondary`, `tertiary` in
`credentials.aditi.toml`), which also unblocked [[test-conference-roster]].

Offline coverage stands: the sim-caps start + invite round trips, the
client-side re-key and failed-start tests in `sl-proto/tests/lifecycle.rs`, and
the viewer's pure-model tests.

**Reached live on aditi (2026-08-21), once
[[viewer-avatar-picker-search-finds-nothing]] was fixed**: the
add-participants ✚ route ran end to end — the grid answered our start with a
`ChatterBoxSessionStartReply` and then pushed the new session's roster
(`ChatterBoxSessionAgentListUpdates`). Two things that run exposed, both since
fixed: the 1:1 the conference was started *from* stayed open beside it (the
reference closes it — a one-to-one **becomes** the conference), and the
brand-new session was swept for a server-history backlog it cannot have, which
the grid answers with an error.

Still to confirm on a grid: that a conference message actually **round-trips**
between all three avatars, and the re-key under a start reply whose
`session_id` differs from ours. The first attempt could not reach the feature
at all, because every route to a multi-selection of *those particular* avatars
was blocked by something else:

- the **radar** needs them in one region — they log in wherever they were, and
  we have no way to gather them;
- the **Friends list** needs them befriended — and
  [[viewer-add-friend-offers-silently]] means the offers went out invisibly,
  while the `sl-repl-tokio` peers holding those avatars cannot accept anyway;
- the **add-participants ✚** route needs a conversation to start from, and the
  1:1 tab it would start from is unusable: its title is a short id
  ([[viewer-im-tab-shows-short-id-not-name]]) and the picker that would name the
  second invitee finds nobody — it searches over the UDP avatar picker, which
  Second Life has retired in favour of the `AvatarPickerSearch` capability
  ([[viewer-avatar-picker-search-finds-nothing]]).

The picker fix opened the way in; what remains is the *other end* of the wire
path — the peers receiving their `ChatterBoxInvitation` under the grid's id and
a message crossing between them. A conformance `conference-roster` case
([[test-conference-roster]]) would assert exactly that without any UI, and is
the better closer for this than another hand-driven session.
