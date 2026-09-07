---
id: viewer-item-permission-gates
title: Item properties offered permission controls the item did not allow
topic: viewer
status: done
origin: user question while live-checking the keyed properties floater
  (2026-09-07)
refs: [viewer-inventory-open-and-properties, viewer-keyed-floater-audit]
---

Context: [context/viewer.md](../context/viewer.md).

Every editable control in the Item Properties window — the name and description
fields, Share with group, Anyone copy, For Sale and its type and price, and the
three next-owner boxes — was live whenever the item was **owned by the agent**.
One flag for seven different questions, so the window offered choices the item
could not honour: letting anyone copy an item the owner may not copy, selling
one they may not transfer, or granting a next owner a right the *creator* never
permitted.

## What the reference gates on

`LLFloaterProperties::refresh` computes each separately, and the masks it reads
differ per control:

| Control | Gate |
| --- | --- |
| Name / description | the agent may **modify** the item |
| Share with group | modify |
| Anyone copy | modify **and** `owner & COPY` **and** `owner & TRANSFER` |
| For Sale / type / price | modify **and** the agent may **transfer** |
| Next owner: modify | `base & MODIFY` — the creator's bound |
| Next owner: copy | `base & COPY` |
| Next owner: transfer | `next_owner & COPY` |

Two of those are easy to get wrong by reasoning alone: the next-owner boxes
follow the **base** mask (what the creator ever allowed) rather than this
owner's rights, and next-owner *transfer* follows the next owner's **copy**
bit, since forbidding transfer only means anything for an item they can hold a
copy of.

## The fix

`PermissionGates::of(item, owned)` reads all seven off the item's masks, and
each control is spawned with its own gate.
`each_control_follows_its_own_permission_bit` pins every row of the table
above, including a no-copy item (anyone-copy dies, the rest lives), a
no-transfer item (the whole sale block dies), and a creator's bound outliving a
permissive owner.

One behaviour changed beyond greying: a **no-modify item you own** now shows a
read-only name and description, which is what the reference does.
