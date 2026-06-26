# God & Estate Admin Tools

A handful of the protocol's operations are reserved for **gods** — the grid's
administrators (Linden Lab staff on Second Life, the estate/grid owner on
OpenSim) — and for **estate managers**. They overwrite a region's settings,
force-reassign land, eject or freeze avatars, wipe an owner's objects across a
whole simulator, or save the world state. None of them are usable by an ordinary
avatar: the simulator checks the requesting agent's rights and silently drops
the request if it lacks them. This chapter covers that admin surface — how a
client acquires god powers, the enforcement and region/parcel operations it can
then issue, and the two messages a god *receives*.

> These commands are guarded entirely on the **server**. The client always lets
> you send them; whether anything happens depends on the rights the simulator
> has recorded for your agent. On a default OpenSim grid only the grid owner
> holds god rights, so most of these are no-ops there unless you log in as the
> owner.

## Acquiring god powers

God powers are not implied by login — a god-capable account must *request* them,
exactly as the reference viewer's "Admin → Request Admin Status" menu does.
`Command::RequestGodlikePowers { godlike }` asks the simulator to grant
(`godlike = true`) or relinquish (`godlike = false`) the powers. The wire
`Token` field the viewer sends is checked on the simulator and ignored by the
viewer, so this workspace neither sends a meaningful token nor surfaces one.

The simulator answers a successful grant with a `GrantGodlikePowers`, surfaced
as `Event::GodlikePowersGranted { god_level }`. The `god_level` is the granted
tier (Linden gods run at various levels; `0` revokes god mode). Until that event
arrives at a non-zero level, the admin operations below will be rejected.

### Forced object selection

A god (or a server-side action acting on the agent's behalf) can push a
selection onto the viewer with `ForceObjectSelect`, surfaced as
`Event::ForceObjectSelect { reset_list, objects }`. `objects` is the list of
[`ScopedObjectId`](world.md)s to select; `reset_list` says whether to clear the
agent's current selection first (the wire `ResetList` flag). This is a
server-to-client push — there is no matching command.

## Land enforcement (parcel owner / estate manager)

Two operations let a parcel owner or estate manager remove a disruptive avatar.
Unlike the rest of this chapter they do **not** need full god rights — land
rights over the parcel suffice.

- `Command::EjectUser { target, action }` sends an avatar away from the agent's
  land. `target` is the `AgentKey` to eject; `action` is
  an `EjectAction` — `Eject` (just send them away) or `EjectAndBan` (also add
  them to the parcel ban list). The wire `Flags` field is `0x0` / `0x1`
  respectively, matching the reference viewer's `handleEjectAvatar`.
- `Command::FreezeUser { target, action }` freezes an avatar in place so it
  cannot move or act, or releases it again. `action` is a `FreezeAction` —
  `Freeze` (`0x0`) or `Unfreeze` (`0x1`), mirroring `handleFreezeAvatar`.

## Region administration (god)

- `Command::GodUpdateRegionInfo { update }` overwrites a region's settings
  wholesale from a `GodRegionUpdate`. The simulator replaces *all* of the
  region's god-tools parameters from this one message, so every field is always
  sent. `GodRegionUpdate` carries the region's [`RegionName`](region.md)
  (`sim_name` — the simulator can *rename* the region from this field), the
  `estate_id` and `parent_estate_id` (the mainland estate is `1`), the 64-bit
  `region_flags` (`RegionFlagsExtended`, built with `sl_wire::RegionFlags`; its
  low 32 bits are also sent in the legacy `RegionFlags` block, exactly as the
  reference viewer truncates), the `billable_factor` land-tier multiplier, the
  `price_per_meter` land price in L$, and the `redirect_grid`
  [`GridCoordinates`](region.md) teleports are bounced to (`(0, 0)` for no
  redirect).
- `Command::SimWideDeletes { owner, flags }` deletes — or returns — every object
  a given `owner` (an `AgentKey`) has placed across the
  whole region. `flags` is a `SimWideDeleteFlags` selecting a subset:
  `others_land_only` (only objects on land the owner does *not* own),
  `always_return_objects` (return to the owner instead of deleting), and
  `scripted_only` (only scripted objects). The all-`false` default (its
  [`Default`]) wipes everything the owner has in the region. This one needs
  **estate-manager or god** rights.

## Parcel administration (god)

Three operations act on a single parcel, identified by its region-local id
(see [`ScopedParcelId`](world.md), which pairs the region with the local id):

- `Command::ParcelGodForceOwner { parcel, owner }` force-reassigns a parcel's
  ownership to `owner` (an `OwnerKey`). The wire carries no
  group flag, so the new owner is always treated as an agent.
- `Command::ParcelGodMarkAsContent { parcel }` marks a parcel — and the content
  on it — as owned by the governor/maintenance account (Second Life's "Governor
  Linden").
- `Command::ViewerStartAuction { parcel, snapshot }` starts a land auction on a
  parcel. `snapshot` is an optional [`TextureKey`](world.md) advertising the
  parcel (`None`, a nil id on the wire, for no snapshot).

## Events directory (god)

`Command::EventGodDelete { event, query_id, query_text, flags, query_start }`
deletes an [events-directory listing](search.md) and asks the simulator to
**re-run** the search so the viewer can refresh its result page. `event` is the
`EventId` to delete; the four `query_*` fields carry the current events search
(its `QueryId`, the search `query_text`, the [`DirFindFlags`](search.md) scope,
and the 0-based `query_start` index of the page) so the simulator can answer
with a refreshed `DirEventsReply` correlated by `query_id`.

## Region state (god)

`Command::StateSave { filename }` asks the simulator to save the region (world)
state to disk. An empty `filename` lets the simulator pick the autosave name,
exactly as the reference viewer does.

---

> **In this codebase**
>
> - Types: `EjectAction`, `FreezeAction`, `SimWideDeleteFlags`, and
>   `GodRegionUpdate` are in `sl-proto/src/types/map.rs` (each `*Action` /
>   `*Flags` type carries the `to_wire` / `from_wire` codec pair against the
>   reference viewer's flag constants). `GodRegionUpdate` reuses the
>   `RegionName` and `GridCoordinates` newtypes and a 64-bit
>   `RegionFlagsExtended`. Parcel ids are `ScopedParcelId`
>   (`sl-proto/src/scoped_id.rs`); the wire-level local id is
>   `sl_wire::RegionLocalParcelId`; the events-search ids
>   (`sl_types::search::EventId`, `QueryId`) and `DirFindFlags`
>   (`sl-proto/src/types/directory.rs`) come from the [search](search.md)
>   surface.
> - Commands `RequestGodlikePowers`, `EjectUser`, `FreezeUser`,
>   `SimWideDeletes`,
>   `GodUpdateRegionInfo`, `ParcelGodForceOwner`, `ParcelGodMarkAsContent`,
>   `EventGodDelete`, `StateSave`, and `ViewerStartAuction` are in
>   `sl-proto/src/command.rs`; their `Session` helpers
>   (`request_godlike_powers`, `eject_user`, `freeze_user`, `sim_wide_deletes`,
>   `god_update_region_info`, `parcel_god_force_owner`,
>   `parcel_god_mark_as_content`, `event_god_delete`, `state_save`,
>   `viewer_start_auction`) are in `sl-proto/src/session/methods.rs`.
> - Events `Event::GodlikePowersGranted` and `Event::ForceObjectSelect` are in
>   `sl-proto/src/types/event.rs`.
> - Server events: the simulator side decodes each outbound god message
>   into a matching `ServerEvent` variant (`RequestGodlikePowers`, `EjectUser`,
>   `FreezeUser`, `SimWideDeletes`, `GodUpdateRegionInfo`,
>   `ParcelGodForceOwner`, `ParcelGodMarkAsContent`, `EventGodDelete`,
>   `StateSave`,
>   `ViewerStartAuction`) in `sl-proto/src/sim_session.rs`, ahead of the
>   `ServerEvent::ClientMessage` fall-through; the eject/freeze/delete flag
>   bytes round-trip through the same `from_wire` decoders, and an unrecognised
>   flag falls through to `ClientMessage`. The two inbound messages are emitted
>   by `SimSession::send_grant_godlike_powers` and `send_force_object_select`.
> - In the [REPL](../tools/sl-repl.md): `request_godlike_powers <godlike:bool>`,
>   `eject_user <target> [ban:bool=false]`,
>   `freeze_user <target> [unfreeze:bool=false]`,
>   `sim_wide_deletes <owner> [others_land_only] [always_return_objects]
>   [scripted_only]`, `god_update_region_info <sim_name> <estate_id>
>   <parent_estate_id> <region_flags> <billable_factor> <price_per_meter>
>   <redirect_grid_x> <redirect_grid_y>`,
>   `parcel_god_force_owner <local_id> <owner>`,
>   `parcel_god_mark_as_content <local_id>`,
>   `event_god_delete <event_id> <query_id> <query_text> <flags>
>   [query_start=0]`, `state_save [filename=]`, and
>   `viewer_start_auction <local_id> [snapshot=]`.
