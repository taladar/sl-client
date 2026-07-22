---
id: viewer-inventory-cof-maintenance
title: Maintain COF links on wear / detach (accurate worn state)
topic: viewer
status: done
origin: split from viewer-inventory-context-actions (2026-07-21) — worn
  detection shipped best-effort
refs: [viewer-inventory-context-actions, viewer-inventory-replace-outfit]
---

Context: [context/viewer.md](../context/viewer.md).

The modern outfit protocol keeps the **Current Outfit Folder** authoritative:
wearing creates a link in the COF, taking off removes it, and every viewer
reads worn-ness from those links. Our wear / detach wiring
(`inventory_actions.rs`) sends the wear commands but does **not** write COF
links; worn detection is therefore best-effort — COF links + the legacy
`AgentWearables` set + a viewer-tracked set of our own attach / detach
commands (`WornAttachments`), which cannot see changes made by another viewer
mid-session and starts cold for attachments on a grid without COF.

This task closes the loop: on wear, `LinkInventoryItem` into the COF (and
remove the replaced slot's link); on detach / take-off, remove the link
(`RemoveInventoryItems` on the link); reconcile from the COF on login. On SL
this is what makes the worn markers exact; OpenSim's COF support varies —
keep the legacy fallbacks.

Reference (Firestorm, read-only): `llappearancemgr.cpp` (`updateCOF`,
`addCOFItemLink` / `removeCOFItemLinks`).

Shipped 2026-07-22: every wear / add / attach-to-point / drag-self-wear
now links the item into the COF (dropping the replaced slot's links,
never double-linking); take-off / detach / remove-from-outfit /
replace-outfit drop the matching links (batched RemoveInventoryItems),
each followed by a COF page re-query; login reconciliation stays the
existing seed from the COF's object links. Legacy fallbacks kept; all
of it no-ops on a grid without a located COF.

Follow-up (2026-07-22): the reference's "@type*100+order"
link-description layering tokens are now written on every clothing
link add (dense per slot) and the surviving links renumbered on
removal (`cof_order_description` / `parse_order_token` / the renumber
pass, unit-tested). Remaining pieces live in
[[viewer-outfit-layer-reorder]] (re-order UI + login read-back) and
[[viewer-bake-cof-layer-order]] (client compositor sorts by the
tokens).
