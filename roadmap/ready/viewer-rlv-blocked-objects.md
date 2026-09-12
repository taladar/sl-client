---
id: viewer-rlv-blocked-objects
title: RLV — the blocked-object list for an unapproved experience's attachment
topic: viewer
status: ready
origin: deferred from viewer-rlv-command-intake (2026-09-07) — the refusal path
  whose only producer this viewer does not have
refs: [viewer-rlv-command-intake, viewer-experience-permission-dialog]
blocked_by: [viewer-experience-event-stream, viewer-rlv-temp-attachment-gate]
---

Context: [context/viewer.md](../context/viewer.md).

RLVa keeps a **blocked-object list**: objects whose every command is refused
with `RLV_RET_FAILED_BLOCKED`, *except* a remove or a `@clear`, which are always
let through so a block can never strand a restriction on the agent.

The intake ([[viewer-rlv-command-intake]]) deliberately skipped this, and
`RlvOutcome` deliberately did **not** gain a variant for it, because the list
has exactly one producer in the reference and this viewer has none of it. That
producer is `RlvHandler::onExperienceAttach`: an experience the user has not
approved rezzes a **temporary attachment** on them, and RLVa refuses to be
commanded by it. Implementing the refusal path with nothing able to add an entry
would have been an unreachable branch.

## What it needs before it means anything

- **[[viewer-experience-event-stream]]** — the `ExperienceEvent` message with
  `Permission == 4` (Attach) is the only thing in the protocol that says an
  experience attached something, and it carries the experience id and the
  object's *name*;
- **[[viewer-rlv-temp-attachment-gate]]** — every step here asks "is this object
  a temporary attachment": the approval test, the resolution filter, and the
  detach hook that unblocks.

## Scope once they land

- `RlvSettings::isAllowedExperience(id, maturity)`: three clauses, and all three
  must hold — temporary attachments may interact with RLVa at all
  (`RLVaEnableTemporaryAttachments`), the user's minimum maturity is **set** and
  the experience's maturity is at least that (a threshold of 0 allows
  *nothing*, which is the reference's default and is easy to invert), and the
  experience is not in the blocked list. Two new settings for the roster in
  `sl-viewer-world-api::rlv`: `RLVaExperienceMaturityThreshold` (0–3, mapped
  onto PG / Mature / Adult) and `RLVaBlockedExperiences` (a `;`-separated list
  of ids, compared **case-sensitively** as strings — a reference quirk worth
  keeping so a list copied between viewers behaves the same);
- the list itself: entries of `(id, name, added-at)`, where the id starts
  **null** because the attach event only names the object. Resolving it needs
  the `AttachmentResources` capability (`Command::RequestAttachmentResources`
  already exists) — match the report's temp attachments by name, fill the id in,
  and immediately `@clear` that object;
- `RlvOutcome::FailedBlocked`, and the refusal in `RlvState::apply` ahead of
  everything else *except* `RlvParam::Remove` and `RlvParam::Clear`;
- expiry, in the pass [[viewer-rlv-command-intake]] already runs: an entry still
  unresolved after five minutes is dropped, and a temp attachment detaching
  unblocks its id.

The reference's console strings and the `RlvStrings::getStringFromReturnCode`
suffix for the new outcome come with it, so the RLVa console names the refusal
rather than falling through to its "failed" wildcard.

Reference (Firestorm, read-only): `rlvhandler.cpp`
(`addBlockedObject` / `isBlockedObject` / `removeBlockedObject`,
`getAttachmentResourcesCoro`, `onExperienceAttach`, the `processCommand` guard
at L462 and the blocked-object sweep in `onGC`), `rlvcommon.cpp`
(`RlvSettings::isAllowedExperience`).
