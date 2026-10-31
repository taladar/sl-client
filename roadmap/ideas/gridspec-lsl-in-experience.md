---
id: gridspec-lsl-in-experience
title: LSL behaviour on Second Life for a script saved into an experience
topic: gridspec
status: ideas
origin: gridspec-lsl-experience (2026-10-25)
refs: [gridspec-lsl-experience, server-lsl-lib-experience]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

[[gridspec-lsl-experience]] measured every experience function from a
script in **no** experience. None of our aditi avatars owns, administers or
contributes to an experience, so none can save a script into one; creating
one needs a Premium account.

## Discover

Once an avatar can save into an experience (one it owns or contributes to):
upload the probes with the experience (`UploadScript`'s `experience`), and
measure the `ScriptQuestion` the viewer is put (its `Experience` block), a
grant and a refusal, the grant's lifetime across scripts and objects,
`llAgentInExperience` after it, the key-value store (create, read, update
with and without a check, delete, count, keys, size, between two objects),
`llTeleportAgent` of a non-owner, `llSitOnLink`, `llAttachToAvatarTemp` to a
non-owner, and what a parcel that does not allow the experience answers.

## Document

`book/src/gridspec/lsl.md` § Experiences and the key-value store.

## Fake grid

Large — [[server-lsl-lib-experience]].

## Viewer

The experience card against the live question (`e2e_script_permissions`
holds it to the reference's reading only).

## From server-lsl-lib-experience (2026-10-31)

The runtime models a script in an experience from Linden Lab's
documentation (`sl-lsl-runtime/src/vm/experiences.rs`); each of these is a
guess to confirm or correct:

- the grant holds 7220 (`TAKE_CONTROLS | TRIGGER_ANIMATION | ATTACH |
  TRACK_CAMERA | CONTROL_CAMERA | TELEPORT`) and replaces what was held;
- an avatar not in the region is denied with reason 4; a blocked one with 4,
  at once;
- the key-value answers: `1,<value>` for create, read and update, `1` for
  delete, `1,<n>` count, `1,<k>,<k>…` keys in key order, `1,<used>,<quota>`
  with `used` the bytes of every key and value and the quota 134217728; a
  duplicate create and a missing delete `0,13`, a missing read and keys past
  the end `0,14`, a checked update of another value or of a missing key
  `0,15`, a key over 1011 or a value over 4095 bytes `0,3`;
- whether a land refusal (17) stops the key-value calls as it stops a
  request;
- `llSitOnLink`'s order (experience, admission, link, then the region's
  refusals) and `LINK_ROOT` of a lone prim naming it;
- what `llClearExperiencePermissions` and `llGetExperienceList` do in an
  experience (nothing and empty, as outside one).
