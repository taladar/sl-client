---
id: viewer-social-friends-disable-unusable-actions
title: The Friends action column offered buttons that silently did nothing
topic: viewer
status: done
origin: split from [[viewer-social-groups-activate-none]], which gave the
  Groups column the treatment (2026-09-12)
refs: [viewer-social-people-panel, viewer-social-groups-activate-none]
---

Context: [context/viewer.md](../context/viewer.md).

The Friends list's action column
([`people`](../../sl-viewer-people/src/people.rs)) kept every button at full
contrast whether or not the current selection could support it. With no friend
selected all five pressed and returned without doing anything, which reads as a
broken button rather than an unavailable one.

## What was built

`friend_action_enabled` is the single predicate behind **both** the greying
(`refresh_friend_actions`) and the press refusal, the arrangement
[[viewer-social-groups-activate-none]] established for the Groups column beside
it. They have to be two places — Bevy's `InteractionDisabled` is advisory — so
routing both through one function is what makes a greyed button an inert one by
construction rather than by two call sites agreeing.

The rules follow the reference's `PeopleContextMenu::enableContextMenuItem`:

- An empty selection disables everything.
- **Offer Teleport** additionally needs somebody who can receive one.
  `LLAvatarActions::canOfferTeleport` is false for an *offline* buddy, and for a
  multi-selection Firestorm enables the offer when **any** of them can receive
  it (capped at 250, the most one `OfferTeleport` may name) — not when all can,
  which is the upstream LL behaviour their `FIRE` patch deliberately replaced.
  The **send** filters by the same rule, as `LLAvatarActions::offerTeleport`
  does: one `can_offer_teleport` decides both who enables the button and whom
  the message names, so a mixed selection offers to the online ones only.
- **IM**, **Profile** and **Remove Friend** need only a selection —
  `can_delete`'s "all are friends" is already true of every row of a *friends*
  list.
- **Block** is deliberately *not* gated here, though the reference's `can_block`
  greys it for yourself and for Lindens. Both refusals already exist on our
  block path in [`mutes::check_block`](../../sl-viewer-people/src/mutes.rs),
  which is where the reference's own authoritative one lives
  (`LLMuteList::add`, right down to only refusing when **text** chat is being
  muted — a Linden's voice or particles can be muted). Greying from a second
  copy would only add an answer that disagrees: `canBlock` reads the *cached*
  display name, so the reference's greying itself flickers as names resolve.

## Worth knowing

`refresh_friend_actions` is deliberately **not** behind the
`floater_shown(CONVERSATIONS_FLOATER_ID)` run condition its neighbours use. A
gated system skips the frame a selection changed behind a hidden window, and
the change flag it was waiting on is gone by the time the window returns — the
buttons would reopen greyed wrongly. Five colour comparisons behind change
detection cost nothing to run ungated.

The same sweep is still worth doing over the other action columns: the Blocked
list, Contact Sets, and the group profile's members / roles tabs all have
buttons that act on a selection.

## How it was verified

Unit: the enablement table for an empty selection, an offline friend, an online
friend and a mixed selection, plus that a mixed selection's offer names the
online friend only. The online gate was inverted to confirm the test fails
without it.

Live, only partly: the local test avatar **has no friends**, so the grid run
could only show the empty-selection state (all five greyed). The per-friend
states — Offer Teleport greyed for an offline friend, live for an online one,
and a mixed selection offering to the online one alone — rest on the unit tests
until somebody befriends the secondary account. The same gap applies to
anything else that needs a populated Friends list, which wants the second test
account.
