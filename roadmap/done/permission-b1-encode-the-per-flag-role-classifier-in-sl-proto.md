---
id: permission-b1
title: Encode the per-flag role classifier in sl-proto
topic: permission
status: done
origin: PERMISSION_ROADMAP.md
---

Context: [context/permission.md](../context/permission.md).

**B1 (from A1, amended by A4). Encode the per-flag role classifier in
    `sl-proto`.** Add a `PermissionRole` enum with **two** variants —
    `RecordOnly` / `Cooperation` (A4 dropped the planned `ApiAction`: no
    granted permission is client-actionable) — plus a total mapping from each
    `ScriptPermissions` bit to its role, per the § Classification reference
    table (note `TELEPORT` is `RecordOnly`, not an action), in a client-side
    module (e.g. `sl-proto/src/types/script.rs`) — kept in `sl-proto`, never
    pushed to shared `sl-types` (the flags themselves stay client-agnostic
    there). The grant registry (B2) still stores the raw granted
    `ScriptPermissions` bitfield wholesale, because the 9 record-only flags
    need no handler and the 3 cooperation flags reuse existing event surfaces
    (`Event::ScriptControlChange` for `TAKE_CONTROLS`, the follow-cam events
    for the camera flags). The session takes no autonomous action on any flag,
    so the classifier exists for the driver's benefit (deciding what to
    surface), not to branch session behaviour. `pub` and consumed only by
    drivers, it warns about nothing on its own and depends on nothing — it may
    land at any point. Smoke test: assert a few representative bit→role
    mappings. **Done** — `PermissionRole { RecordOnly, Cooperation }` plus the
    `const fn PermissionRole::for_flag(i32) -> Option<Self>` total per-bit
    mapping in `sl-proto/src/types/script.rs` (re-exported via `types.rs` /
    `lib.rs`); `for_flag` returns `None` for zero / unknown / multi-bit input
    so a driver calls it per set bit. Smoke test
    `permission_role_classifies_representative_flags` asserts the 3
    cooperation and representative record-only flags (incl. `TELEPORT`) plus
    the `None` cases. Landed ahead of the § Open-questions sign-off since
    B1 is independent and gates nothing (the blocker #1 only gates B2/B5).
