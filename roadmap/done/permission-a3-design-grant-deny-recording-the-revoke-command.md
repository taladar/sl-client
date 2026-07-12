---
id: permission-a3
title: Design grant/deny recording & the revoke command
topic: permission
status: done
origin: PERMISSION_ROADMAP.md
---

Context: [context/permission.md](../context/permission.md).

**A3. Design grant/deny recording & the revoke command.** How
`answer_script_permissions` records the granted subset into the registry
(and how a partial grant or an explicit deny is represented). Add the
missing granular revoke: a new `RevokePermissions` `Command` + `Session`
method (the wire message exists; wire it through `command.rs` /
`session/methods.rs` / `session/circuit.rs`), and define how
`release_script_controls` (`ForceScriptControlRelease`) updates the
mirror. Define the library-user query accessors (e.g.
`granted_permissions(holder) -> ScriptPermissions`,
`script_controls() -> …`). **Done — see § Grant/deny & revoke reference
(from A3) + task B2 in § Phase B.** Decided: recording happens *after*
the wire send inside `answer_script_permissions` (gaining one new param,
`experience_id`, passed back from the request — the only grant datum not
derivable at answer time, keeping the "no outstanding-request tracking"
decision); a non-empty grant **replaces** the holder's entry, an explicit
deny (empty grant) **removes** it, so a deny is the *absence* of a
registry entry (this resolves A2's open question). The new object-scoped
`RevokePermissions` command clears, from the mirror, **only** the bits the
sim honours (`TRIGGER_ANIMATION | OVERRIDE_ANIMATIONS`) across every grant
on that object; `release_script_controls` clears the *taken-controls*
tracker (A6) but leaves the grant registry untouched (the `TAKE_CONTROLS`
grant persists; only the live taken set resets). Accessors return public
types only (the A2 registry types stay private):
`granted_permissions(task_id, item_id)`, a `script_grants` iterator
yielding a new public `ScriptGrantInfo` view, and the A6-finalized
`script_controls`.

## Grant/deny & revoke reference (from A3)

How the registry (A2) is written, revoked, and read. The simulator stays
authoritative; every write here mirrors a wire action the client just took, and
the mirror is deliberately kept *conservative* (it never reports a permission
gone unless the sim actually drops it).

**Recording a grant (`answer_script_permissions`).** Recording happens at the
*end* of the existing method, after the `ScriptAnswerYes` is queued — the mirror
follows the wire, never leads it. The method gains one parameter,
`experience_id: Option<ExperienceKey>`: it is the only `ScriptGrant` datum not
derivable at answer time (`task_id` / `item_id` are already params; `kind` /
`circuit` come from the A2 `holder_kind(task_id)` helper), and the driver
already holds it on the `ScriptPermissionRequest` it is answering — passing it
back keeps A2's "no outstanding-request tracking" decision intact. The runtime
callers receive the answer as a `Command::AnswerScriptPermissions`, which today
carries no experience, so **that command gains the same
`experience_id: Option<ExperienceKey>` field**: the driver fills it from the
`ScriptPermissionRequest` it is answering, so the datum reaches the session
through the command boundary rather than via session-side request tracking
(consolidated finding 3). New shape:

    answer_script_permissions(task_id, item_id, permissions, experience_id, now)

Recording rule, on the holder `ScriptHolder { task_id, item_id }`:

- **Non-empty grant** → build `ScriptGrant { granted: permissions, kind,
  circuit, experience_id }` (kind/circuit from `holder_kind`) and **insert**,
  *replacing* any prior entry for that holder — a later `llRequestPermissions`
  supersedes the earlier grant, matching the sim (one live grant per script).
- **Empty grant** (`permissions.is_empty()`, i.e. an explicit deny / "grant
  nothing") → **remove** any existing entry. A `ScriptAnswerYes` with
  `Questions = 0` denies/replaces server-side, so the faithful mirror is
  *no entry*. **This resolves A2's open question: a deny is the absence of a
  registry entry — empty entries are never stored**, so accessors need no
  "entry-but-empty" case (absent ≡ empty granted).

**The granular revoke (`RevokePermissions`, wire `Low 193`).** The wire message
is **object-scoped** (`ObjectID` + `ObjectPermissions: U32`), not script-scoped,
and the sim **honours only the animation bits** (`TRIGGER_ANIMATION |
OVERRIDE_ANIMATIONS`) per Firestorm. New command + method:

    Command::RevokePermissions {
        object_id: ObjectKey, permissions: ScriptPermissions,
    }
    Session::revoke_permissions(object_id, permissions, now) -> Result<(), Error>

- **Send:** a new `circuit.send_revoke_permissions(object_id, permissions, now)`
  builds `AnyMessage::RevokePermissions` (`AgentData` + `Data { object_id,
  object_permissions: permissions.0 }`), `Reliability::Reliable` — same pattern
  as `send_force_script_control_release` / `send_script_dialog_reply`. The full
  requested bitfield goes on the wire (faithful to the caller's request).
- **Mirror update:** for every grant whose `holder.task_id == object_id`
  (object-scoped → may touch several scripts in one object), clear from
  `granted` only `permissions & (TRIGGER_ANIMATION | OVERRIDE_ANIMATIONS)` — the
  bits the sim will actually drop. Clearing the *requested* bits instead would
  desync the mirror (the sim still enforces e.g. `TELEPORT`), so the mirror only
  follows what the server honours. A grant whose `granted` becomes empty is
  **removed** (same invariant as the deny path: no empty entries).
  `TAKE_CONTROLS` is *not* revocable this way — releasing controls is
  `release_script_controls`.

**`release_script_controls` (`ForceScriptControlRelease`) mirror update.** This
forcibly releases **all** taken controls agent-globally (the sim echoes a
`ScriptControlChange(release)`). On send, the session resets its
*taken-controls* tracker (the session-global set designed in A6) to empty
immediately, rather than waiting for the echo. It does **not** touch
`script_grants`: the `TAKE_CONTROLS` *grant* persists (the script may re-take),
so "permission granted" (registry) and "controls currently taken" (A6 tracker)
stay separate concerns. The concrete clear lands with the A6 tracker; A3 only
fixes the policy (B3 below carries it).

**Query accessors (public; A2 registry types stay private).** The library user
reads the mirror through public signatures returning public types —
`ScriptHolder` / `ScriptGrant` / `HolderKind` are never exposed:

- `granted_permissions(&self, task_id: ObjectKey, item_id: InventoryKey) ->
  ScriptPermissions` — the granted subset for one script, `ScriptPermissions`
  empty when there is no grant (absent ≡ empty, per the deny rule).
- `script_grants(&self) -> impl Iterator<Item = ScriptGrantInfo> + '_` — every
  current grant, as a new small `#[derive(Clone, Copy)]` **public** view:

      struct ScriptGrantInfo {
          task_id: ObjectKey, item_id: InventoryKey,
          granted: ScriptPermissions,
          is_attachment: bool,            // HolderKind::Attachment, flattened
          experience_id: Option<ExperienceKey>,
      }

  The internal `circuit: Option<CircuitId>` (reset-scoping only) is **not**
  surfaced; `is_attachment` flattens `HolderKind` without leaking the enum.
- `script_controls(&self) -> …` — reserved here, **return type finalized in A6**
  (the taken-controls tracker, likely the live `ControlFlags` or an iterator of
  `ScriptControl`). A3 fixes only the name and the "reads the A6 tracker"
  intent.

No new `Event` is emitted on recording: a grant/deny/revoke is a *local* API
call the driver itself just made, so there is nothing inbound to report (A7
confirms the inbound event surface is unchanged).
