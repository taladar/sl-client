---
id: viewer-calling-card-offer-unanswered
title: A calling card offered to us is never shown or answered
topic: viewer
status: done
origin: gridspec-mute-list audit of local filtering (2026-10-16)
refs: [gridspec-mute-list, missing-out-batch-1, viewer-give-calling-card]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

`Event::CallingCardOffered` is emitted by the session (`sl-proto`
`session/methods.rs`, the `OfferCallingCard` arm) but no viewer system reads
it: the offer is never shown, never accepted or declined, and the mute list is
never consulted for it. Only the examples, the REPL and the survey list the
event.

## Reference

`llviewermessage.cpp` (`process_offer_callingcard`): an offer from a muted
resident (`isMuted(source, name, flagTextChat)`) or while Do Not Disturb is on
is declined at once; one whose sender's name is not known yet is dropped with a
warning; otherwise the `OfferCallingCard` notification asks, and its answer
sends `AcceptCallingCard` (into the Calling Cards folder) or
`DeclineCallingCard`.

## To do

A consumer next to `ingest_offers_invites` (`sl-viewer-people`
`offers_invites.rs`): decline for a muted sender (`MuteList::text_muted`) and
under Do Not Disturb, otherwise raise the catalogue's `OfferCallingCard` card
with Accept / Decline. Neither grid filters for a mute (`gridspec-mute-list`),
so the decline is the viewer's. OpenSim does not surface calling-card offers
(`missing-out-batch-1`), so the live check is aditi.

## Done

`sl-viewer-people::offers_invites` gained a second ingest system,
`ingest_calling_card_offers`, beside the IM-driven one — a calling-card offer
is its own `OfferCallingCard` message, not an IM dialog. `judge_calling_card`
is the reference's decision as one pure function:

- **Do Not Disturb, or a sender whose text chat is muted** (`text_muted`, by id
  or by the sender's grid name for a by-name entry): declined at once, with the
  busy reply to the sender when DnD is on — the reference's forced decline goes
  through the callback that sends `send_do_not_disturb_message`. A mute by id
  needs no name, so a muted sender is declined before their name resolves.
  Unlike the other offers, a calling card is *declined* under DnD, not held,
  because that is what the reference does with this one.
- **Otherwise** the `OfferCallingCard` card (heading, "{name} is offering
  their calling card.", the reference's bookmark line, Accept / Decline).
  Accept sends `AcceptCallingCard` into the Calling Cards folder (the root when
  there is none, nothing while the inventory has not loaded) and fetches that
  folder; Decline and the close × send `DeclineCallingCard` (plus the busy
  reply if DnD came on since the card was raised).

**One deliberate departure: an offer waits for its sender's name instead of
being dropped.** The reference reads the name off the sender's avatar object and
drops an offer from anyone not in view with a warning. Here the name comes from
the name cache (`AvatarState::name_record`, which a nearby avatar's NameValue
seeds just as the reference's lookup does), and an unresolved sender is asked
for with `request_name` while the offer waits in `PendingCallingCards` — so an
offer from someone out of view is shown rather than lost.

The card is a gallery element (`calling-card-offer-toast`) with its contract
rows, so the layout sweep covers it.

## Verified

`cargo test --release -p sl-viewer-people --lib -- offers_invites presence` —
24 green, three new: the card's Accept files into Calling Cards and fetches it,
falls back to the root, sends nothing with no inventory; Decline and × decline
and record their answers; the judgement (DnD, id mute without a name, by-name
mute on the grid name, the text-chat exception, waiting, the shown name); and
the busy reply only under DnD, from a contact set's own text.
`cargo test --release -p sl-client-bevy-viewer --lib -- ui_contract ui_test` —
32 green with the new element in the sweep.
`cargo test --release -p sl-client-bevy-viewer --test e2e_two_avatars
a_calling_card` — the fake-grid stage
`a_calling_card_offer_is_asked_about_and_declined_while_unavailable`, the grid
playing the offerer (an account it knows, so the name resolves): the card shows
and names the sender, Accept reaches the grid as `AcceptCallingCard` with a real
folder, Decline as `DeclineCallingCard`, and under Unavailable an offer raises
no card, is declined at once and the sender hears the busy reply.

Not run live: the driver has no raw-command verb, so a two-viewer stage needs a
viewer that can *send* an offer — the Give Calling Card menu entry of
[[viewer-give-calling-card]]. The grid side of the exchange on Second Life is
already covered by the `calling-card` conformance case.
