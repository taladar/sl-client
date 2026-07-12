---
id: test-object-edit
title: set name / desc / flags / shape / material / permissions / for-sale
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 8 — Objects & scene graph `[both]`
---

Context: [context/test.md](../context/test.md).

`object-edit` — set name / desc / flags / shape / material /
permissions / for-sale. `1av`. The whole build-tool edit surface exercised
against one self-manufactured cube (as `object-rez-derez` does), each change
confirmed by the reply that carries it back. The **administrative** edits land
in the object's extended properties, so they are read back with one
[`Command::RequestObjectProperties`] at the end: rename
([`Command::SetObjectName`]), re-describe ([`Command::SetObjectDescription`]),
toggle the next-owner *copy* bit ([`Command::SetObjectPermissions`]), and put
it up for sale as a priced copy ([`Command::SetObjectForSale`]). The
**geometric / physical** edits re-broadcast the object on the interest-list
stream, so each is confirmed by an [`Event::ObjectUpdated`] carrying the new
value: metal material ([`Command::SetObjectMaterial`] → [`Object::material`]),
phantom ([`Command::SetObjectFlags`] → the `FLAGS_PHANTOM` bit of
[`Object::update_flags`]), and a hollowed box ([`Command::SetObjectShape`] →
[`Object::shape`]). Baseline properties are read first so each edit is proven
a real *change*: the permission edit flips the next-owner copy bit away from
whatever the grid defaults it to (OpenSim starts a new prim at move+transfer,
copy clear; Second Life at full). Two client fixes fell out of live testing:
the `ObjectName`/`ObjectDescription` (and `ObjectImage` media-URL) encoders
were sending the variable string field without its trailing NUL, so OpenSim
dropped the last character — now NUL-terminated like every other string
field; and `Command::SetObjectPermissions` /
`Session::set_object_permissions` now take a typed [`Permissions`] mask
instead of a raw `u32` (re-exported from both runtime crates). Reuses the
`start_location` hook and the settle/reference machinery of `object-rez-derez`
(Default Region on OpenSim, where the workspace's rezzed test object is the
placement reference; a no-primitive landing region records `partial` on SL).
Green on OpenSim: all seven edits applied and confirmed, next-owner copy
`0x82000` → `0x8a000`, sale 25 L$ (copy), material / flags / shape RTTs ≈
30–90 ms. The Trash cleanup leaves one item per run — bounded inventory
residue, acceptable on a throwaway grid. `[both]`; the aditi run is deferred
with the batch (no aditi record this session).
