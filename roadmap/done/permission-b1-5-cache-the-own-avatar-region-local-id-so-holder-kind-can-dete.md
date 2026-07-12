---
id: permission-b1-5
title: Cache the own-avatar region-local id, so holder_kind can detect attachments
topic: permission
status: done
origin: PERMISSION_ROADMAP.md
---

Context: [context/permission.md](../context/permission.md).

**B1.5 (from Open-question #1). Cache the own-avatar region-local id, so
    `holder_kind` can detect attachments.** Resolves the #1 sign-off blocker;
    B2's attachment detection and B5's attachment-kept-on-teleport test both
    depend on it, so it lands **before B2**. **Done 2026-06-26** — added the
    per-circuit `own_avatar: BTreeMap<CircuitId, RegionLocalObjectId>` field
    on `Session` (presence ≡ known, absence ≡ `None`; same per-circuit-cache
    convention as `regions` / `time_dilation`, and dropped alongside them in
    `forget_sim_objects`); a set-once `note_own_avatar` helper; fill source A
    in `upsert_object` (avatar object with `full_id == agent_id` →
    `note_own_avatar`, covering both full and compressed updates, which share
    that insert path — a terse update can introduce no new object so it is not
    a fill source); fill source B at `AgentMovementComplete` via the new
    `cached_own_avatar_local_id` scan; and the public
    `Session::own_avatar_id() -> Option<ScopedObjectId>` accessor. The private
    `is_own_avatar` helper is **deferred to B2** (where `holder_kind` consults
    the slot) to avoid a dead-code window — B1.5 exposes the slot via the
    per-circuit map and the public accessor instead. Four `lifecycle.rs` tests
    cover fill source A, the `pcode`/foreign-avatar guards, the set-once rule,
    and the movement-complete backstop. **Finding (refines fill source B):**
    `AgentMovementComplete` (wire `Low 250`) carries **no** avatar
    region-local id — only `AgentID` / `Position` / `LookAt` / `RegionHandle`
    / `Timestamp` / `ChannelVersion` — so B reads the id from the
    **cached own-avatar object** (the roadmap's named "/cached own-avatar
    object" source), making it a backstop to A (which already records at
    cache-insert time) rather than an earlier-than-`ObjectUpdate` path. The
    behaviour and scope are unchanged; only the "earliest reliable point"
    wording does not hold.
    - **State** (`sl-proto/src/session.rs`): add a per-circuit
    `Option<region-local id>` for our own avatar — the `LocalID` the simulator
    assigns our avatar's `ObjectUpdate`, wrapped in the existing
    `ScopedObjectId` / region-local-id newtype, never a bare `u32`. Hold it
    beside the rest of the per-circuit state, **initialised `None`** (no id is
    known until our own avatar object is seen on that circuit). Per-circuit
    because a region-local id is unique only within a circuit and our avatar
    gets a fresh one in each region.
    - **Fill source A — `pcode::AVATAR`** (`session/methods.rs`): today there
    is no `pcode::AVATAR` arm in the object-update path; add one so an
    `ObjectUpdate` (or terse update) for an avatar whose
    `full_id == self.agent_id()` records its region-local id into the
    circuit's slot (the general "we saw our own avatar object" signal).
    - **Fill source B — `AgentMovementComplete`** (`session/methods.rs`): when
    it fires for a circuit whose slot is still `None`, set it from the avatar
    local id the movement-complete / cached own-avatar object carries (the
    earliest reliable point, before the first `ObjectUpdate` may arrive).
    - **Set-once rule**: set the slot the first time *either* source observes
    it while still `None`, then leave it (our own local id is stable for the
    life of that circuit).
    - **Use**: a private `is_own_avatar(parent_local_id, circuit)` (or have
    `holder_kind` consult the slot) — a holder is parented to *us* iff its
    `parent_id` resolves, on the same circuit, to the cached own-avatar
    region-local id. `holder_kind` (B2) uses this for the `Attachment` branch;
    while the slot is still `None`, detection falls back to `InWorld` (the
    conservative default) for that brief window only.
    - **Tests** (`lifecycle.rs` / `sim_session.rs`): seed our own avatar via
    `object_update[_in]` with `full_id == agent_id` → the slot is set and a
    holder parented to it classifies as `Attachment`; an
    `AgentMovementComplete` with no prior own-avatar `ObjectUpdate` also sets
    it; a holder parented to *another* avatar / an in-world prim stays
    `InWorld`. No new wire message — a pure session-state addition.
