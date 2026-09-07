---
id: viewer-experience-event-stream
title: Experience event stream — the ExperienceEvent log and its notifications
topic: viewer
status: ready
origin: gap found wiring viewer-rlv-command-intake (2026-09-07)
refs: [viewer-notification-catalogue-experiences, viewer-experience-permission-dialog,
  viewer-experiences-floater]
---

Context: [context/viewer.md](../context/viewer.md).

**Two notification templates exist and nothing raises them.** The catalogue
port ([[viewer-notification-catalogue-experiences]]) landed `ExperienceEvent`
and `ExperienceEventAttachment` as data, and no code ever emits either, because
the viewer does not ingest the stream they describe.

The region tells the viewer, after the fact, what an experience it is running
under actually *did*: it sends a **`GenericMessage` with the method
`ExperienceEvent`** carrying an LLSD map. The reference dispatches it through
`gGenericDispatcher` into `LLExperienceLog`, which keeps it and notifies. The
fields are `public_id` (the experience), `ObjectName`, `OwnerID`, `ParcelName`,
`Permission` (an `ExperiencePermission` code — `4` is **Attach**), and
`IsAttachment`; `Time` and `Count` are the log's own, and a repeat of the same
five-field tuple on the same day increments `Count` rather than appending.

Scope:

- decode the `ExperienceEvent` generic message (`Event::GenericMessage` already
  carries the envelope) into a typed event, resolving the experience's name
  through the experience cache the permission surfaces already use;
- the per-account **event log** the reference keeps in `experience_events.xml`:
  events by day, a retention window (its default is 7 days), the same-tuple
  coalescing above, and the notify-on-new-event toggle;
- raise `ExperienceEvent` / `ExperienceEventAttachment` from it (the
  `IsAttachment` flag picks which), with the `EventType` substitution the
  reference builds from the permission code;
- surface the log where the reference does — the experience profile's event
  list, which is [[viewer-experiences-floater]]'s window.

This is also the **only** signal in the protocol that says "an experience
attached something to you", which is what [[viewer-rlv-blocked-objects]] needs
and the reason this gap was found.

Reference (Firestorm, read-only): `indra/newview/llexperiencelog.cpp` /
`.h` (the dispatcher handler, the day-keyed store, `notify`,
`getPermissionString`), `llfloaterexperienceprofile.cpp` (the event list).
