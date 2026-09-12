---
id: viewer-experience-event-stream
title: Experience event stream — the ExperienceEvent log and its notifications
topic: viewer
status: done
origin: gap found wiring viewer-rlv-command-intake (2026-09-07)
points: 3
refs: [viewer-notification-catalogue-experiences, viewer-experience-permission-dialog,
  viewer-experiences-floater, viewer-rlv-blocked-objects,
  protocol-experience-environment-push]
---

Context: [context/viewer.md](../context/viewer.md).

**Two notification templates existed and nothing raised them.** The catalogue
port ([[viewer-notification-catalogue-experiences]]) landed `ExperienceEvent`
and `ExperienceEventAttachment` as data, and no code ever emitted either,
because the viewer did not ingest the stream they describe.

The region tells the viewer, after the fact, what an experience it is running
under actually *did*: a **`GenericMessage` with the method `ExperienceEvent`**
carrying an LLSD map. The reference dispatches it through `gGenericDispatcher`
into `LLExperienceLog`, which keeps it and notifies.

## The wire

`sl-wire::experience_event` is the codec, built to the same shape as
[[protocol-experience-environment-push]]'s because the envelope *is* the same
one: the experience id rides in the message's **invoice**, parameter 0 is a
serialized LLSD map, parameters 1 and 2 are the object and parcel names, and
both are optional exactly as the reference's handler treats them.

`ExperienceEventPermission` is the `Permission` field typed. The wire value is
**not** the LSL `PERMISSION_*` bit — it is the *index* into the reference's
`SCRIPT_PERMISSIONS` table (`llscriptruntimeperms.h`), whose entry *n* carries
the bit `1 << (n + 1)`. That is why `4` is Attach and not TriggerAnimation, and
it is the kind of thing only reading the table settles: the nine indices the
reference has strings for are named, and every other one is kept verbatim as
`Other(i32)` rather than rejected, because a log that dropped a report would be
lying about what happened.

Every field *inside* parameter 0 is optional, which is a deliberate difference
from the environment push's decoder. The push rejects an `action` naming none of
its three cases, because acting on an environment change nobody can parse is
worse than not acting; a report is only a record, and a sparse one is still a
true statement about something that happened. A missing `Permission` becomes
`None`, a missing `OwnerID` the nil uuid, a missing `IsAttachment` false.

`sl-proto` surfaces it as `Event::ExperienceEvent`, decoded off both
`GenericMessage` and `LargeGenericMessage` (the reference registers one handler
on the dispatcher both envelopes reach), with the same forward-it-raw fallback
the push has: a report that will not decode comes back out as the envelope it
arrived in, because this is the *only* record the agent gets of what an
experience did to them. Server side, `SimSession::send_experience_event`, and
`Action::ReportExperienceEvent` in `sl-fake-grid`'s timeline.

## The log

`sl-viewer-notices::experience_log` is `LLExperienceLog`: a per-account
`experience_events.json` beside the account `settings.toml`, following the
[[viewer-audit-notification-store-overwrite]] rules (an unreadable file is
preserved rather than treated as empty, writes are atomic and serialized),
because the log is the only copy of a record the user may want much later.

- **Coalescing** is the reference's: a repeat of the same (experience, object,
  owner, parcel, permission) tuple on the same local day bumps the previous
  entry's count and timestamp instead of appending — and, as in the reference,
  only the **last** entry is a candidate, so an interleaved second object
  starts a new row for each. A ride that re-seats you a hundred times is one
  row that says 100.
- **Retention** is the `ExperienceLogDays` setting, defaulting to the
  reference's 7 and clamped to its spinner's 0–14. Zero keeps nothing, which is
  how the reference spells the log being off. Pruning runs on load and on each
  arrival.
- **Notification**: with `NotifyAllExperienceEvents` set, every recorded report
  — a coalesce included, because the reference hooks `notify` to the same signal
  a coalesce fires — raises `ExperienceEvent` or `ExperienceEventAttachment`,
  picked by `IsAttachment`, with the `EventType` substitution built from the
  permission.

Two divergences, both on the record. The retention window and the notify toggle
are ordinary **settings** rather than fields inside the log file: this viewer
already has a per-avatar settings scope, which is what the reference's
`PER_SL_ACCOUNT` file *is*, so keeping a second copy inside the log would only
be a second place for them to disagree. And an entry carries an absolute
timestamp rather than a day-keyed map with a time-of-day string beside it, so
expiry is arithmetic instead of `sscanf`-ing the key a row was filed under.

The unknown-permission strings are a third, smaller one. The reference reaches
its two unknown outcomes by accident: `getPermissionString` falls through to the
literal key name (`"ExperiencePermission5"`) for an index with no string, and to
a missing-string marker when the field is absent altogether — `strings.xml` has
`ExperiencePermissionShortUnknown` but no long `ExperiencePermissionUnknown`.
Both land on a real sentence here, with the raw code interpolated.

## The surface

The Experiences floater grows a third section — Recent events — under the
allowed / blocked lists. It is the reference's Events tab of the same floater
(`LLPanelExperienceLog`), rendered as a section rather than a tab because this
floater has no tab strip. Rows are newest first, showing the localized time, the
short permission label, the experience name and the object; a repeat shows its
count. Beside it are the reference's Notify checkbox (settings-bound) and Clear.

The list **scrolls** rather than paging. The reference's next / prev buttons
exist because its list is clipped at a page size, but the retention window
already bounds the log by time, and scrolling reaches every row without a pager
— which matters more than matching the chrome, since a row behind a button that
is not there is a row that has been lost.

Experience names resolve through the floater's existing `names` cache, with one
addition: `request_logged_experience_names` asks for the name of any experience
the **log** mentions that the cache does not know. The two permission lists get
that for free when their GET replies, but a logged event can name an experience
on neither list — a forgotten one — and a row showing a bare id would be the
only place in the floater that does.

## Verification

Unit: the wire codec round-trips each case (including an unnamed index and a
report with no `Permission` at all), pins the nine named codes against the
reference table, and pins that the experience id is the invoice and never a
parameter. The log's tests pin the coalescing rule and its three ways of *not*
coalescing (next day, interleaved event, different permission), the retention
window including the zero-day case, that a prune dropping nothing is not a
write, the local-day boundary actually following the offset, and the
unreadable-vs-empty distinction.

End-to-end: `sl-fake-grid`'s `a_script_reports_what_an_experience_did` drives
the real client through two reports that differ in permission, in
`is_attachment` and in object name — the three fields the log coalesces on — and
asserts both arrive as typed events in the order the script wrote them.

## What this unblocks, and what it deliberately leaves

[[viewer-rlv-blocked-objects]] loses one of its two blockers: the `Permission ==
4` report is the only thing in the protocol that says an experience attached
something, and it carries the experience id and the object's name, which is
exactly what RLVa's blocked-object list is built from.

**Not done here:** a real region sends an `ExperienceEvent` beside a
`PushExpEnvironment`, and the reference logs the push itself through
`handleExperienceMessage` ([[protocol-experience-environment-push]]'s message,
which carries no `Permission` and so shows as an unknown operation there). This
viewer does not feed its pushes into the log, and `sl-fake-grid` sends the two
actions independently on purpose, so a test can drive either half alone. Wiring
the push into the log — as permission 17, which is what `ChangeEnvSettings`
*is*, rather than as the reference's blank — is a small follow-up worth doing
when something needs it.
