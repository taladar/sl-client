---
id: permission-b2
title: The complete grant registry
topic: permission
status: done
origin: PERMISSION_ROADMAP.md
---

Context: [context/permission.md](../context/permission.md).

**B2 (from A2/A3/A4/A5). The complete grant registry — model, recording,
    read, revoke and all region-leave resets, in one warning-clean unit.**
    **Done 2026-06-26** — added the private `ScriptHolder` / `ScriptGrant` /
    `HolderKind` types and the
    `script_grants: BTreeMap<ScriptHolder, ScriptGrant>` field on `Session`
    (session.rs), the `object_by_full_id` / `holder_kind` /
    `drop_inworld_grants` helpers, recording in `answer_script_permissions`
    (new `experience_id` param plumbed through
    `Command::AnswerScriptPermissions` and all three runtimes + the REPL
    `opt_experience` arg), the public read accessors `granted_permissions` /
    `script_grants` (+ public `ScriptGrantInfo` view), the revoke mirror
    update, and the four region-leave resets (two teleport sites,
    `forget_sim_objects`, `KillObject`). Seven focused `lifecycle.rs` tests
    cover grant/deny/re-grant, the animation-only revoke, the teleport
    in-world-cleared/attachment-kept split, neighbour-crossing keep-all, the
    circuit-retired drop, and `KillObject`. Builds, clippy-clean (restriction
    lints), `cargo test --workspace` green.
    **Two adaptations vs the literal plan (no behavioural change):** (1) the
    granular revoke `Command` / `Session::revoke_script_permissions` /
    `circuit.send_revoke_permissions` **already existed** (built earlier under
    `MISSING_ROADMAP`'s outbound coverage as `RevokeScriptPermissions`, wired
    through every runtime), so B2 only **added the mirror update** to the
    existing method rather than creating a new `RevokePermissions` command —
    reusing the existing path, not duplicating it. (2) `ScriptHolder` could
    not `derive(Ord)` because `ObjectKey` / `InventoryKey` expose no `Ord`;
    the `BTreeMap` key order is a hand-written `Ord`/`PartialOrd` on the
    underlying UUIDs instead. **Deferred cleanup (not worth a version bump on
    its own):** next time functionality is moved into shared `sl-types`,
    derive `Ord`/`PartialOrd` on the `ObjectKey` / `InventoryKey` (and likely
    the other UUID key) newtypes there, then drop this hand-written impl and
    restore `#[derive(... Ord)]` on `ScriptHolder` — fold it into that batch,
    do not bump `sl-types` solely for it. (B2.5 still upgrades the empty-grant
    "remove" path to an explicit *denied* state.) Sub-steps (as implemented):
    - **State model** (`sl-proto/src/session.rs`): the private `ScriptHolder`
    (`{ task_id: ObjectKey, item_id: InventoryKey }`, deriving `Ord` for the
    `BTreeMap` key); `ScriptGrant` with `granted: ScriptPermissions`,
    `kind: HolderKind`, `circuit: Option<CircuitId>`,
    `experience_id: Option<ExperienceKey>`; the private `HolderKind` enum
    (`Attachment` / `InWorld`); and the field
    `script_grants: BTreeMap<ScriptHolder, ScriptGrant>` beside `sit` /
    `teleport` (init empty in constructor at `methods.rs:138`). Add private
    `object_by_full_id(&self, ObjectKey) -> Option<&Object>` (scan the nested
    `self.objects` maps) and
    `holder_kind(task_id: ObjectKey) -> (HolderKind, Option<CircuitId>)`
    applying the § State-model reference rule (attachment iff cached object
    `attachment_point().is_some()` and parented to our own avatar; else
    in-world / not-found; record the circuit found on). "Parented to our own
    avatar" uses the **B1.5** cached own-avatar region-local id — real
    attachment detection, no `InWorld`-only interim.
    - **Recording** (`answer_script_permissions`, `session/methods.rs`): add
    the `experience_id: Option<ExperienceKey>` parameter; keep the existing
    `ScriptAnswerYes` send first, then append the recording — compute
    `ScriptHolder { task_id, item_id }` and, using `holder_kind` for `kind` /
    `circuit`, **insert**
    `ScriptGrant { granted: permissions, kind, circuit, experience_id }`
    (replacing any prior entry) when `permissions` is
    non-empty, or **remove** the holder's entry when `permissions.is_empty()`
    (the initial deny path — **task B2.5 then upgrades this** to record an
    explicit *denied* state distinct from never-asked, per Open-question #2).
    Plumb `experience_id` by
    adding it to `Command::AnswerScriptPermissions` (`command.rs:563`);
    the driver fills it from the `Event::ScriptPermissionRequest` it answers.
    Update the runtime arms (`sl-client-tokio/src/lib.rs`,
    `sl-client-bevy/src/lib.rs`) and the REPL `CommandSpec`
    (`sl-repl/src/registry.rs`, a new optional `experience_id` arg defaulting
    to `None`); update the test caller in `sl-proto/tests/lifecycle.rs`.
    - **Read accessors** (public; the registry types stay private):
    `granted_permissions(task_id, item_id) -> ScriptPermissions` (empty when
    absent) and `script_grants() -> impl Iterator<Item = ScriptGrantInfo>`,
    plus the public `#[derive(Clone, Copy)] ScriptGrantInfo` view (`task_id`,
    `item_id`, `granted`, `is_attachment` flattening `HolderKind`,
    `experience_id`; the internal `circuit` is not surfaced). These read
    `granted` / `kind` / `experience_id`, so those fields are not dead.
    - **Granular revoke** (wire `Low 193`): add
    `Command::RevokePermissions { object_id, permissions }` (`command.rs`),
    dispatch it to `Session::revoke_permissions(object_id, permissions, now)`
    (`session/methods.rs`), and add `circuit.send_revoke_permissions(...)`
    (`session/circuit.rs`) build `AnyMessage::RevokePermissions` (`AgentData`
    + `Data { object_id, object_permissions: permissions.0 }`,
    `Reliability::Reliable`) — mirroring `send_force_script_control_release`.
    After the send, across grants with `holder.task_id == object_id`, clear
    `permissions & (TRIGGER_ANIMATION | OVERRIDE_ANIMATIONS)` from `granted`,
    removing any grant left empty. Wire all three runtimes (the bevy arm is
    compiler-forced, the tokio arm is parity-only, plus a REPL `CommandSpec`
    `revoke_permissions <object_id> <permissions-i32>` beside
    `release_script_controls`) — consolidated finding 1.
    - **Region-leave resets** (no message sent to the sim; reading `circuit`
    here is what makes that field non-dead), at the existing reset sites in
    `sl-proto/src/session/methods.rs`: add a private
    `drop_inworld_grants(&mut self)`
    (`script_grants.retain(|_, g| matches!(g.kind, HolderKind::Attachment))`)
    and call it at the two **teleport** `SitState::NotSitting` sites,
    `begin_handover` (`:696`) and `TeleportLocal` (`:1960`) — **not** the
    sit-timeout (`:3072`) or `stand` (`:3427`) sites; in `forget_sim_objects`
    (`:1439`),
    `self.script_grants.retain(|_, g| g.circuit != Some(circuit_id))`
    (both child-only callers); in the inbound `KillObject`
    handler (`:1180`), read the removed object's `full_id` (already resolved
    there for `region_handle`) and
    `self.script_grants.retain(|h, _| h.task_id != full_id)`; leave
    `promote_child_to_root` (`:790`) untouched (keep all grants). The
    taken-controls tracker is **not** reset here (B3 owns it).
    - **Tests** (`lifecycle.rs`): record → `granted_permissions`
    returns the subset; answer with `ScriptPermissions::empty()` → entry gone;
    re-grant replaces; revoke animation keeps `TELEPORT`, revoking last bit
    removes the entry; a real teleport clears the in-world grant and keeps the
    attachment grant (**both halves are now writable** — B1.5 supplies the
    own-avatar id for attachment detection, so the earlier
    `// TODO(attachment-detection)` gate is lifted: seed the attachment holder
    with `object_update_in` carrying an `attachment_point` and a `parent_id`
    resolving to the own-avatar object); a neighbour crossing keeps all; a
    `DisableSimulator` for a child circuit drops that circuit's grants; a
    `KillObject` for a granted object drops its grant.
