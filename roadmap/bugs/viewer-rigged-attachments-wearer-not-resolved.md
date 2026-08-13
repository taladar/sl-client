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

## FIX LANDED (2026-08-11, partial): request unknown parent objects

`upsert_object` (`sl-proto`) now requests any `parent_id` a child references but
we don't track (`RequestMultipleObjects`, deduped via `requested_parents`,
cleared on arrival). Regression test `unknown_parent_object_is_requested`.
**Live-verified on aditi: the own-avatar shoes now render**, and the
stuck-attachment count dropped sharply.

**Residual (2026-08-11, second aditi run, wearer-walk probe + owner/attach
enrichment):** down to **10** skips, all still `UNTRACKED`, concentrated on
**2 shared neighbour roots** on `circuit#3` (`205112639` ×4, `205112653` ×6) —
whole linksets whose root **still** never arrives even though a child referenced
it (so it was requested once). Stable: no new skips accrued as the camera
explored. Cause: the dedupe asks **once and never retries**, so a lost UDP
request — or a child-agent the neighbour sim won't serve the root to — leaves
those children stuck.

Two things the enrichment established:

- **These 10 are NOT the mesh heads.** Their `attach_point`s are **Left Hand
  (5)** and **Right Ear (14)**, not Skull. So the user-reported *bloated
  system-head / missing-hair* avatars are a **separate** stall — most likely the
  **aditi asset-CDN outage** that ran through this whole session (3 textures
  permanently `503 Service Unavailable — DNS failure`, 65 failures) or a missing
  bake, **not** this wearer-resolution gap. Judge avatar completeness on a run
  where the CDN is healthy.
- **`owner_id` is nil** in these neighbour `ObjectUpdate`s, so resolving the
  wearer by owner does not work: SL only carries the real owner via
  `ObjectProperties` (a `RequestObjectProperties` round-trip), not the object
  update. The new `TrackedObject::owner_id` is still correct (own objects /
  cases where the sim does send it) and the `attach_point` in the probe is what
  actually identified the residual here.

**Residual fix:** re-request a parent that is still untracked after a timeout
(periodic sweep of `requested_parents` with backoff / a cap), and/or verify the
neighbour child-agent is entitled to the root object. Then those linksets
resolve. For per-avatar attribution of the residual, request `ObjectProperties`
for the untracked terminus root (owner in the update is nil).

## ROOT CAUSE (2026-08-11): untracked parent objects — no request for the parent

The wearer-walk terminus probe on a clean run: **170 attachments stuck on
`wearer unresolved after 1 hop; terminus UNTRACKED — its parent/root object
never arrived`**. So the attachment's own update arrived, but its
**parent (linkset root) object is not tracked** — the chain breaks one hop up.

`ObjectUpdateCached` cache-misses *are* requested (`try_dispatch_object` →
`request_object_ids` → `send_request_multiple_objects`, on root **and** child
circuits, `methods.rs`). The gap: that only requests ids the sim **explicitly
lists** in a cached update. When a child object arrives referencing a
`parent_id` we have **never been told about** (never in a cached update / not in
our interest list / its update lost), nothing requests it → the parent stays
untracked forever → `avatar_root_of` fails → the attachment never binds.

- **Persistent (the 170):** neighbour-region attachments whose linkset-root
  parent was never streamed on the child circuit.
- **Intermittent (own shoes, main circuit):** the parent usually arrives on its
  own, but a late/lost parent update has no fallback request → shoes missing
  that session.
  **A Firestorm relogin in between re-published the whole outfit**, so the next
  login got a complete object stream and the shoes rezzed — which is why the
  symptom is intermittent and re-attach / relogin "fixes" it.

### Perf-branch regression suspects (the object-update fast path)

The user recalls avatars were more complete before the perf branch, and the
untracked-parent symptom points at the perf-branch object-update optimisations:

- **`9f64f5f0` "motion-only fast path for terse object updates"** — fingerprints
  non-motion blocks and skips the cascade for motion-only updates; also changed
  `reconcile_parent` to skip re-parenting an already-`parented` child unless the
  parent changed. If a child is marked `parented` before its root is actually
  tracked (or an identity/parent block is skipped), the parent linkage can be
  lost.
- **`0fcae2a5` "coalesce repeated object updates in the pending queue"** —
  merges repeated upserts per id and claims to preserve **root-before-child**
  ordering. If that ordering is imperfect, or a root's upsert is dropped/merged,
  a child is built with an untracked parent that never fills in.

Bisect these two (revert-test on aditi with the wearer-walk probe) before
building the defensive fix below — one of them likely *is* the regression.

### Fix (defensive, and correct regardless of the regression)

When an object update (or an attachment referenced by
`apply_rigged_attachments`) has a `parent_id` for which no object is tracked,
**request that parent** via `Command::RequestObjects` /
`send_request_multiple_objects` on the object's circuit (the reference viewer's
`LLViewerObjectList::processUpdateCore` requests unknown parents). Throttle /
dedupe so one unknown parent is asked for once. This fixes both the neighbour
flood and the intermittent own-avatar attachments even if the regression above
is the trigger.

## Update (2026-08-11): the shoes are a SEPARATE, earlier stall

Added the wearer-walk terminus probe (`avatar_root_walk` + the detailed log in
`apply_rigged_attachments`, gated by `SL_VIEWER_LOG_ATTACHMENT_BIND=1`). A
second live run — everything rezzed and stable, **shoes still missing** — logged
**zero** bind-stage trace lines at all (no `not yet bound`, no
`wearer unresolved`). So:

- The earlier **255 wearer-unresolved** objects were that neighbour region's
  **in-world rigged meshes** (content-heavy region), *not* the shoes — they only
  appeared when that region was loaded.
- The **shoes never reach the `RiggedMesh`-pending bind stage** — no trace
  covers them, no mesh failure/`gave up` is logged, and `rendered no geometry`
  is 0. So they are stuck at **`PendingGeometry::Mesh`** (mesh never decoded /
  never requested) or are **built but invisible** (material/alpha/skin), *not*
  the wearer-resolution flood.

**Next for the shoes specifically:** an **own-avatar worn-attachment stage
probe** — enumerate the objects parented to the own agent and log each one's
`PendingGeometry` stage (Mesh / RiggedMesh / built) + whether its mesh was
requested/decoded — to see exactly where the shoes sit. (The in-world
rigged-mesh flood below is a real but *separate* concern.)

## Open questions / next diagnostics (the in-world rigged-mesh flood)

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
