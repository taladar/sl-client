---
id: viewer-rigged-attachments-wearer-not-resolved
title: Worn rigged attachments (e.g. own shoes) don't render — wearer never resolved / too many rigged-pending objects
topic: viewer
status: bugs
origin: chasing own-avatar missing shoes on aditi (2026-08-11)
---

Context: [context/viewer.md](../context/viewer.md).

Symptom: an own-avatar worn rigged attachment (repeatably: **shoes**) does not
render at initial rez, while everything else rezzes in seconds. **Re-attaching**
(detach + attach) makes it appear — so the bind/skin/render code is sound; it is
an initial-rez object-state problem, and the fresh object a re-attach creates
resolves where the original does not.

## Evidence (live, `SL_VIEWER_LOG_ATTACHMENT_BIND=1`, 2-3 min run)

- The attachment-bind trace shows **255 distinct rigged objects, all on
  `circuit#3`, all stuck on `wearer avatar not resolved (parent chain)`** — i.e.
  `AvatarState::avatar_root_of` returns `None`: walking `object_parents` up from
  the object never reaches an avatar recorded in `by_scoped`, so
  `apply_rigged_attachments` skips them every frame forever.
- No `rendered no geometry` warns, no mesh fetch failures — so this is **not**
  the empty-finest-LOD case ([[viewer-mesh-hair-not-rendering]] candidate 1) nor
  a fetch stall; it is candidate 4 (wearer never resolves) at scale.
- **255 rigged-pending objects is far too many** for the single-digit avatars
  present across the 4 connected regions. So either many of these are
  **in-world rigged meshes** (a skin block but no wearer — should render
  statically / as an animesh control avatar, not defer to the attachment bind
  that can never resolve a wearer — a known limitation), or mesh objects are
  being **misclassified** as worn rigged attachments.

## Open questions / next diagnostics

1. **Is `circuit#3` the root (own) region or a child (neighbour)?** Determines
   whether the own shoes are even in this set. `by_scoped` IS populated for
   neighbour avatars (`apply_object` at `avatars.rs:2225`, called on child
   circuits too), so a neighbour avatar's own attachments *should* resolve —
   check whether the failing objects' chains reach a spawned avatar at all.
2. **Add a wearer-walk terminus probe:** log, per failing object, whether
   `avatar_root_of` (a) hits a broken chain (`object_parents` gap), (b) reaches
   a **non-avatar** root (→ genuinely in-world, not worn), or (c) reaches an
   avatar scoped-id **not yet in `by_scoped`** (→ wearer body not spawned yet).
   That classifies the 255 and isolates the shoes.
3. If the bulk are in-world rigged meshes: they should not sit in
   `apply_rigged_attachments` forever — render them in bind pose / route only
   true animesh to a control avatar, so the trace (and the wearer bind budget)
   isn't flooded.
4. For the shoes specifically: confirm they are a worn attachment whose wearer
   is the own avatar, and why their parent chain doesn't reach it at initial rez
   but does after a re-attach (a parent-linkage ordering, or
   `parent_id`/`scoped_parent` pointing at an object that isn't tracked yet).

Related: [[viewer-mesh-hair-not-rendering]] (same "worn rigged mesh not
rendering" family; this pins its candidate 4).
