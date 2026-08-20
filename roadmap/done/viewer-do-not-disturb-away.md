---
id: viewer-do-not-disturb-away
title: Away / auto-AFK / Do-Not-Disturb modes + autoresponse
topic: viewer
status: done
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-chat-input-bar]
refs: [viewer-name-tags-decorations]
---

Context: [context/viewer.md](../context/viewer.md).

The presence modes and their behaviours:

- **Away** — manual toggle plus auto-AFK after N minutes of no input
  (configurable timeout); plays `ANIM_AGENT_AWAY`, sets the away agent flag,
  shows "(Away)" in the tag ([[viewer-name-tags-decorations]]), and clears on
  input.
- **Do Not Disturb (Busy)** — manual mode: suppress IM/inventory-offer
  toasts (queue them for later), send the configurable busy auto-response to
  IM senders, decline teleport offers, play `ANIM_AGENT_DO_NOT_DISTURB`.
- **FS autoresponse** — the Firestorm extension: an auto-reply mode separate
  from DND (respond but keep receiving), with per-mode reply texts and an
  "only to non-friends" option; shown in the own tag.

Scope: the mode state machine, the agent-flag / animation wire writes, the
IM-side auto-reply + toast queueing, the timeout setting, and the menu /
status entries to switch modes. The reply texts and timeouts persist in the
settings store per account.

Reference (Firestorm, read-only): `llagent` (busy/away), `fsautoresponse`
settings, `llimview` (DND queueing).

Builds on: the IM/chat session layer and the settings store.

## Parity-audit addendum (2026-08-19)

The parity audit found four away-family options beyond this task's
current scope. Sit the avatar down when going away (`AvatarSitOnAway`)
and quit the viewer after N seconds of AFK (`QuitAfterSecondsOfAFK`),
both on Firestorm's general preferences tab (our `AfkTimeoutSeconds`
setting is already registered and consumed here). And two autoresponse
variants alongside the busy/DND response: a distinct away-mode
autoresponse (`FSSendAwayAvatarResponse` + `FSAwayAvatarResponse` text)
and an autoresponse sent to *muted* senders
(`FSSendMutedAvatarResponse` + `FSMutedAvatarResponse` text) — the
response-text settings are partly registered already
(`AutorespondResponse`, `BusyResponse`); the mode wiring is what's
missing.

## Built

New `presence.rs` in the viewer owns all four modes.

- **The state machine.** `PresenceState` holds the two *session* modes — Away
  and Do Not Disturb — and the AFK clocks; the two autorespond modes are the
  persisted account settings `AutorespondMode` /
  `AutorespondNonFriendsMode`, so (like the reference) they survive a relog
  while the session modes deliberately do not. Auto-AFK fires after
  `AfkTimeoutSeconds` without input; the next input clears it, but only once
  away has held 10 s (the reference's `MIN_AFK_TIME` debounce, without which
  the mouse move one frame after the auto-AFK undoes it).
- **The wire writes.** Away and Do Not Disturb are broadcast the only way the
  protocol carries them — as signalled animations (`ANIM_AGENT_AWAY`,
  `ANIM_AGENT_DO_NOT_DISTURB`), sent on the mode edge. `ControlFlags::AWAY`
  is folded into the control word by the movement driver rather than sent by
  a second writer that would fight it for the same field. There is no local
  playback: the simulator's echo of our own `AgentAnimation` is what the own
  tag reads, so playing it locally too would only risk double-driving the
  pose.
- **The canned replies.** New `Command::AutoResponse` /
  `Session::send_auto_response` send the reply under the
  `DoNotDisturbAutoResponse` dialog (wired through both runtimes and
  `sl-repl` as `auto_response`), so the recipient's client can tell it from a
  typed IM and never answers it in turn. The decision is the reference's
  `getAutoresponseTextForAvatar` precedence — Do Not Disturb, then
  autorespond-to-non-friends, then autorespond, then away — with the blocked
  sender lifted in front of it, as a pure `reply_for` function under test.
  Sent **once per conversation** (a new `ConversationModel::has_conversation`,
  the reference's `hasSession`), and noted in that conversation through a new
  `ConversationNotice` message — the keyed sibling of `NearbyChatNotice`.
- **Toast queueing.** Do Not Disturb holds every corner toast
  (`DoNotDisturbQueue` in the notification host) and every offer / invite card
  (`DeferredOffers` in the offers host), replaying both on the falling edge, so
  nothing is lost. Modals are never held — a blocking confirm belongs to a flow
  the user is in right now. Silent inventory auto-accept still runs while the
  mode is on: filing an item interrupts nobody.
- **The UI.** A Comm ▸ Online Status submenu with the four check items, in the
  reference's order and wording (Do Not Disturb shows as *Unavailable*), each
  raising the reference's mode-set notification on the rising edge. The name
  tag grew the `Unavailable` and `Auto-Response` status entries (the latter own
  only, behind `ShowAutorespondInNameTag`, default off like the reference), so
  `viewer-name-tags-decorations` no longer owes them. Preferences: sit-on-away
  and quit-after-AFK on the general tab, the away and blocked-sender replies
  (each a toggle plus its text) alongside the existing replies on the chat tab.

Addendum items: `AvatarSitOnAway` and `QuitAfterSecondsOfAFK` are implemented;
so are the away autoresponse (`SendAwayAvatarResponse` + `AwayAvatarResponse`)
and the blocked-sender autoresponse (`SendMutedAvatarResponse` +
`MutedAvatarResponse`).

Verified live on the local OpenSim (2026-08-20): the Online Status submenu,
its check marks, and the mode toggles. The **IM auto-reply** is unit-verified
only (`reply_for`'s precedence and the blocked short-circuit) — the two-avatar
live check, and the do-not-disturb toast/offer replay against real incoming
offers, are still outstanding.

Not carried, and split out rather than silently dropped:

- The reference lets a **contact set** override the autoresponse text per set
  (`LGGContactSets::getAutoresponseForFriend`). Our contact sets carry a name,
  a colour and a pseudonym but no per-set reply — split out to
  [[viewer-contact-set-presence-extras]].
- The blocked-sender reply ships, but a blocked resident's IM is still
  *displayed*; honouring the text-mute aspect in chat and IM is its own gap,
  filed as [[viewer-muted-residents-text-still-shown]].
