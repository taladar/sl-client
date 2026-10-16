---
id: viewer-calling-card-offer-unanswered
title: A calling card offered to us is never shown or answered
topic: viewer
status: bugs
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
