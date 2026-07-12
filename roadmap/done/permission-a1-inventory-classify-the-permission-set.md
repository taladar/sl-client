---
id: permission-a1
title: Inventory & classify the permission set
topic: permission
status: done
origin: PERMISSION_ROADMAP.md
---

Context: [context/permission.md](../context/permission.md).

**A1. Inventory & classify the permission set.** Enumerate every
`ScriptPermissions` flag and assign the client's role: *record-only* (the sim
enforces it; the client only mirrors the grant — `DEBIT`, `ATTACH`,
`CHANGE_LINKS`, `RETURN_OBJECTS`, `SILENT_ESTATE_MANAGEMENT`, `EXPERIENCE`),
*needs library-user cooperation* (`TAKE_CONTROLS` → the user routes the
control inputs; `CONTROL_CAMERA` / `TRACK_CAMERA` → the user drives the
camera), or *client-actionable via existing API* (`TELEPORT` →
`Session::teleport_to`; `TRIGGER_ANIMATION` / `OVERRIDE_ANIMATIONS` → the sim
plays them, nothing client-side). Output: a per-flag responsibility table that
drives A4. **Done — produced the classification reference + task B1 in
§ Phase B** (drafted as 8 record-only, 3 cooperation, 1 API action `TELEPORT`;
**A4 later reclassified `TELEPORT` as record-only → 9 record-only,
3 cooperation, 0 API action** — see the A4 correction).

## Classification reference (from A1)

The 12 grantable `ScriptPermissions` flags by the client's responsibility. The
simulator stays authoritative; every client record is a mirror, not a security
boundary. Roles (final, after A4): **record-only** — the sim enforces
end-to-end, the client only mirrors the grant and takes no action (any effect
arrives later on the ordinary message path) · **cooperation** — inert unless the
runtime routes control inputs or applies camera params; `sl-proto` surfaces the
grant and tracks the live state. There is **no** autonomous-action role — A4
established that no granted permission triggers a client-initiated `Session`
method (see § Auto-act policy reference).

| Flag | Bit | Role |
|------|-----|------|
| `DEBIT` | `1<<1` | record-only |
| `TAKE_CONTROLS` | `1<<2` | cooperation |
| `TRIGGER_ANIMATION` | `1<<4` | record-only |
| `ATTACH` | `1<<5` | record-only |
| `CHANGE_LINKS` | `1<<7` | record-only |
| `TRACK_CAMERA` | `1<<10` | cooperation |
| `CONTROL_CAMERA` | `1<<11` | cooperation |
| `TELEPORT` | `1<<12` | record-only (was "API action" — see A4) |
| `EXPERIENCE` | `1<<13` | record-only |
| `SILENT_ESTATE_MANAGEMENT` | `1<<14` | record-only |
| `OVERRIDE_ANIMATIONS` | `1<<15` | record-only |
| `RETURN_OBJECTS` | `1<<16` | record-only |

`TRIGGER_ANIMATION` / `OVERRIDE_ANIMATIONS` are record-only (the sim plays them;
this refines the A1 draft, which listed them as client-actionable but noted
"nothing client-side"). The 3 cooperation flags reuse event surfaces `sl-proto`
already emits — `TAKE_CONTROLS` via `Event::ScriptControlChange` /
`ScriptControl`, `TRACK_CAMERA` / `CONTROL_CAMERA` via the follow-cam events
(`FollowCamProperty` / `FollowCamPropertyValue`). **`TELEPORT` is record-only,
not an action** (the A1 draft misclassified it): a granted `llTeleportAgent`
teleports the agent *server-side* and arrives as a normal teleport handled by
`TeleportPhase`, so the client only mirrors the grant. `Event::ScriptTeleport`
(`llMapDestination`) is a **separate, permission-less** map beacon — not the
`TELEPORT` grant — and is left as a passthrough (A4).
