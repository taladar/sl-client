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
