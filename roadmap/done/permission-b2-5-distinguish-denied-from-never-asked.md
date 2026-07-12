---
id: permission-b2-5
title: Distinguish denied from never-asked
topic: permission
status: done
origin: PERMISSION_ROADMAP.md
---

Context: [context/permission.md](../context/permission.md).

**B2.5 (from Open-question #2). Distinguish *denied* from *never-asked*.**
    Reverses A3's "deny is the absence of an entry": the driver's UI that
    prompts the user may want to know it already denied a script, so the
    mirror records an explicit denial. Depends on B2 (the registry it
    extends); sequenced **before B4** (the query surface the UI reads).
    **Done 2026-06-26** — the per-holder registry value is now tri-state: a
    new private `GrantStatus { Denied, Granted(ScriptPermissions) }` field
    `status` on `ScriptGrant` (replacing the bare `granted` field), with
    `kind` / `circuit` / `experience_id` staying at the struct level so a
    denial carries the same reset-scoping data as a grant and the region-leave
    / revoke `retain` closures read those fields **unchanged**.
    `answer_script_permissions` now records `Denied` for an empty answer (was
    "remove the entry"), always inserting (replacing any prior answer); a
    never-asked holder stays absent. Added the public
    `script_permission_status(task_id, item_id) -> ScriptPermissionStatus`
    accessor (new public enum `NeverAsked` / `Denied` /
    `Granted(ScriptPermissions)` in `types/script.rs`, re-exported) and a
    `denied: bool` field on `ScriptGrantInfo` (set on denials, `granted` then
    empty), so `script_grants()` now also yields denials. `revoke` only
    touches `Granted` entries (a denial is always kept). Two new
    `lifecycle.rs` tests (`never_asked_denied_and_granted_are_distinct`,
    `teleport_drops_inworld_denial_keeps_attachment_denial`) plus the updated
    `answer_records_grant_and_empty_denies` deny half. Builds, clippy-clean
    (restriction lints), `cargo test --workspace` green.
    **One adaptation vs the literal plan (no behavioural change):** the plan's
    preferred enum was
    `ScriptPermissionStatus { Denied, Granted(ScriptGrant) }` as the *whole*
    map value; instead the status enum (`GrantStatus`) is a *field* of
    `ScriptGrant`, keeping the common `kind` / `circuit` / `experience_id`
    shared between the two states. This is the variant the plan's own
    tiebreaker selects ("whichever keeps the existing reset/revoke `retain`
    closures readable" — they stay literally unchanged) and avoids a name
    clash with the public `ScriptPermissionState` snapshot (B4); the public
    API (`script_permission_status` / `ScriptPermissionStatus` /
    `ScriptGrantInfo.denied`) is exactly as specified. Sub-steps:
    - **Model** (`sl-proto/src/session.rs`): make the per-holder state
    tri-state. Either widen the registry value to a private
    `ScriptPermissionStatus { Denied, Granted(ScriptGrant) }` (absent key ≡
    *never-asked* — the third state stays "no entry"), or keep `script_grants`
    for grants and add a parallel private `denied: BTreeSet<ScriptHolder>`.
    Prefer the enum (one keyed store, no chance of a holder in both);
    whichever keeps the existing reset/revoke `retain` closures readable. A
    denied entry
    carries the same `circuit` / `HolderKind` as a grant would, so the
    region-leave resets (B2) treat it identically (a denial on an in-world
    object is dropped on teleport, an attachment denial is kept).
    - **Recording** (`answer_script_permissions`): replace B2's "empty grant →
    remove" with "empty grant → record `Denied` for the holder" (still
    replacing any prior grant/denial). A subsequent non-empty answer for the
    same holder supersedes the denial with a grant, and vice-versa — one live
    state per script, matching the sim.
    - **Read accessors**: add a public tri-state
    `script_permission_status(task_id, item_id) -> ScriptPermissionStatus`
    (a new **public** enum `NeverAsked` / `Denied` /
    `Granted(ScriptPermissions)` — the internal `ScriptGrant` stays private)
    and a `denied: bool` (or a `status`) field on `ScriptGrantInfo`/the
    iterator so a denied holder is visible. `granted_permissions` is unchanged
    (still empty for both denied and never-asked — it answers "what is
    granted", the status accessor answers "which of the three").
    - **Tests** (`lifecycle.rs`): answer empty → `script_permission_status` is
    `Denied` (not `NeverAsked`); a never-answered holder is `NeverAsked`; a
    grant-then-deny and deny-then-grant each leave only the latest; a denial
    on an in-world holder clears on teleport, on an attachment is kept (reuses
    the
    B1.5 detection). Update B2's "empty → entry gone" assertion to the denied
    state.
