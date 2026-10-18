# Estate

What each grid does with the Region/Estate floater's estate traffic: how the
estate is read, what a resident without rights is told when it tries an
estate manager's commands, and what each avatar is told when a manager acts on
another — the access lists, the region and estate messages, the covenant,
freezing, ejecting, sending home, kicking and restarting the region. The
messages themselves are described in
[Region & Estate Information](../content/region.md#estate-information).

Measured on 2026-10-17 by the census `estate`, two avatars a run: **A** is the
estate's owner where the test avatars hold estate rights, **B** another
resident. On OpenSim A is the estate owner and both stand at the Default
Region's centre; every leg is run. On aditi neither test avatar holds rights on
any estate (`gridspec-aditi-test-land`), so both stand at the build location in
Mauve (mainland) and only what a resident without rights sees is measured;
every manager's leg reads "not measured" there. Each run is held to these
answers as `Measured` constants; the census runs on the live grids only, since
the fake grid does not yet answer the actions
(`server-fake-grid-estate-actions`).

## Reading the estate

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `getinfo`, by a resident | nothing | nothing | nothing (both flavours gate on the account's estate rights) |
| `getinfo`, by the estate owner | not measured | the configuration, then four `setaccess` replies in the order managers, allowed agents, allowed groups, banned agents, each sent even when empty | the same |
| `getinfo`, by an estate manager | not measured | answered as to the owner, the manager on the managers list | the same |
| the configuration (`estateupdateinfo`) | — | owner A, flags `0x14128026`, sun 0, a parent estate id, the covenant as the nil id when there is none, no abuse address | its fixture's |
| `EstateCovenantRequest`, by anybody | answered: no covenant, the estate named `mainland`, owned by the nil id | answered: no covenant, the estate's name, owned by A | answered, as the fixture holds it |
| the `EstateAccess` capability, by a resident | HTTP 200, every list `undef` | HTTP **401**, no body | each flavour as its grid |
| … by the estate owner or a manager | not measured | HTTP 200, every list an array, empty ones `[]` | arrays, on both flavours |
| a ban as the capability gives it | not measured | `id`, `banning_id` (who banned), `ban_date` as `yyyy-mm-dd hh:mm`, `last_login_date` always `na` | the ban with no banner and unknown dates (`0000-00-00 00:00`, `na`) — the fake grid keeps no ban details |

The covenant summary and `getinfo` disagree on OpenSim about an estate without
a covenant: the summary says none (no id), the configuration names the nil id.
Both read as "no covenant". The reference viewer reads its Access tab from the
capability and ignores the payload of a `setaccess` reply, reading the
capability again instead; this viewer does the same where the capability
answers with lists, and shows a ban's last login, date and banner as the
reference does.

## What a resident without rights is told

B, holding no estate rights, tries each estate command on A.

| B asks to | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| kick A (`kickestate`) | nothing | nothing | nothing |
| send A home (`teleporthomeuser`) | nothing | nothing | nothing |
| message the region (`simulatormessage`) | nothing | nothing | nothing |
| message the estate (`instantmessage`) | nothing | nothing | nothing |
| cancel a restart (`restart -1`) | nothing | nothing | nothing |
| add itself to the allowed agents | nothing | nothing | nothing |
| set the covenant | nothing | nothing | nothing |
| ask for the telehub | nothing | nothing | nothing |
| freeze A (`FreezeUser`) | an alert keyed `AvatarFreezeFailure`: "Freeze failed because you don't have admin permission for that parcel." | nothing | — (`server-fake-grid-estate-actions`) |
| eject A (`EjectUser`) | an alert keyed `AvatarEjectFailed`: "Eject failed because you don't have admin permission for that parcel." | nothing | — |

A is told nothing of any of them, on either grid. Freezing and ejecting are
the parcel's powers rather than the estate's, which is why Second Life answers
them where it answers no estate command.

## Messages

A region message (`simulatormessage`, "Send message to region") and an estate
message (`instantmessage`, "Send message to estate").

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| what everybody in the region is told, sender too | not measured | a non-modal `AgentAlertMessage`, the text after a `/` — the same for both, and with no sender | the same, on both flavours |
| an estate method the grid does not know | not measured | nothing (logged as unknown) | nothing |

The `/` is OpenSim's mark on every non-modal agent alert; the reference reads
it as "show as a system message" and strips it, as this viewer does. The
reference's region message names its sender's legacy name in its fourth
parameter; this client sent the agent id there until 2026-10-17. Its estate
message is the `instantmessage` method (sender name, text), which the client
did not have.

## Access lists

A changes the lists about B, while B is in the region.

| A | OpenSim | fake grid |
| --- | --- | --- |
| allows B | A is sent the allowed agents **and** the banned agents (`setaccess` ×2); B is told nothing | the same |
| removes B from the allowed agents | the two lists again | the same |
| makes B a manager | A is sent the managers list alone; B nothing | the same |
| as a manager B reads the capability and `getinfo` | both answered, as to the owner | — |
| bans B | A is sent the two lists; B is **sent home**: `TeleportStart` with `VIA_HOME`, then — B's home being in the region — `TeleportLocal`, so B stays | the lists; B is not moved (`server-fake-grid-estate-actions`) |
| … where B has no home set | B is told "Home set not" (a teleport failure), then kicked: "Your access to the region was revoked and TP home failed - you have been logged out." | — |
| unbans B | the two lists | the same |
| removes B from a list it is not on | an alert "User is not on the region ban list" (for the ban list) and the lists | the lists |

Second Life is unmeasured; its fake flavour answers as OpenSim does.

## The covenant

| behaviour | OpenSim | fake grid |
| --- | --- | --- |
| A sets the covenant (`estatechangecovenantid`) | nobody is told anything | the same |
| the summary after | names the new covenant, with a time | the same |
| A clears it (the nil id) | nobody is told anything | the same |

## Freeze and eject

| behaviour | OpenSim | fake grid |
| --- | --- | --- |
| A freezes B | A: alert "Avatar Frozen."; B: alert "A has frozen you for 30 seconds.  You cannot move or interact with the world." | — (`server-fake-grid-estate-actions`) |
| A unfreezes B | A: "Avatar Unfrozen."; B: "A has unfrozen you." | — |
| a freeze left to wear off | B, 30 s later: an agent alert "/The freeze has worn off; you may go about your business." | — |
| A ejects B | A: "Avatar Ejected."; B: "You have been ejected by A", and moved to the nearest place it may be | — |
| … with a ban | the same alerts; B goes onto the parcel's ban list | — |

In the alerts, "A" stands for A's legacy name. They are `AlertMessage`s, but
for the freeze wearing off.

## Home, kick and restart

| behaviour | OpenSim | fake grid |
| --- | --- | --- |
| A sends B home (`teleporthomeuser`) | B: `TeleportStart` `VIA_LOCATION`, `TeleportStart` `VIA_HOME`, `TeleportLocal` (B's home is in the region); A nothing | — |
| A sends everybody home (`teleporthomeallusers`) | A **and** B are sent home, four times over each (one per region of the local megaregion's estate) | — |
| A kicks B (`kickestate`) | B: `KickUser` "You have been kicked out", then the circuit closes; A nothing | — |
| A cancels a restart with none pending (`restart -1`) | nothing | nothing |
| A restarts the region in 30 s | both: agent alerts "/Region will restart in 30 seconds" and, 15 s later, "/Region will restart in 15 seconds"; at 30 s both are kicked, "The simulator is going down." | the first notice to everybody in the region, in each flavour's form; no countdown and no restart |

OpenSim's restart schedule counts down from the delay in steps of 2 min, then
30 s, then 15 s, a notice at each; `-1` is "delay by an hour" there, up to three
times, while the reference viewer sends it as "cancel". Second Life's notice is
not measured — no test avatar may restart a region there. The reference viewer
reads it as an `AlertMessage` keyed `RegionRestartMinutes` (parameters
`MINUTES`, `NAME`) or `RegionRestartSeconds` (`SECONDS`, `NAME`), its
parameters serialized LLSD, and closes its countdown on a system message
beginning "Region restart cancelled.". `FakeSl` sends the keyed form.

## Region info

Measured on 2026-10-17 by the census `region-info` (two avatars, the same
standing as above: OpenSim's A the estate owner in the Default Region, aditi's
two residents in Mauve). Every run, on all four grids, is held to these
answers; what the region is configured with — its flags, rating, product,
limits, estate ids, water and terrain limits — is recorded but not held, since
it is the region's and not the grid's.

### What a region says of itself

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `RequestRegionInfo`, by a resident | answered | answered | answered (both flavours) |
| … what a resident reads against a manager | — | the same record | the same |
| … who else is told | nobody | nobody | nobody |
| the extended flags (`RegionInfo3`) | sent | **not sent** | each flavour as its grid |
| the chat ranges (`RegionInfo5`) | 10 / 20 / 100 m, no offsets, flags 0 | not sent | each flavour as its grid |
| the combat block | flags `0x5`, on death 0, throttle 100, regeneration 1, the rest 0 | not sent | each flavour as its grid |
| the record's flags against the handshake's | the same | without the estate's bits: `ALLOW_ACCESS_OVERRIDE`, `EXTERNALLY_VISIBLE`, `ALLOW_DIRECT_TELEPORT` | each flavour as its grid (`ESTATE_ONLY_FLAGS`) |
| the product (`RegionInfo2`) against the handshake's | the same (`023`, `Mainland / Full Region`) | the same (empty) | the same |
| the legacy 8-bit agent cap against the wide one | the same | the same | the same |
| the redirect when there is none | `-1,-1` | `0,0` | each flavour as its grid |
| the estate sun and sun hour | no estate sun, hour 0 | no estate sun, hour 0 | the same |
| the handshake's extended flags (`RegionInfo4`) | sent | sent | sent |
| the handshake's data centre | named | none | each flavour as its grid |
| the handshake's protocols | `0x1` | `0x8000000000000000` | each flavour as its grid |
| the rating: record and map against the handshake | **record and map `Adult`, handshake `Mature`** | all `Pg` | one rating everywhere (not held on `FakeSl`) |

OpenSim states the region's own flags in its record and in `SimStats`
(`EstateManagementModule.GetRegionFlags`) and adds the estate's only in the
handshake; the estate's bits are best read from the estate's own record
(`getinfo`). Mauve's rating disagreement was seen on one region and has no
explanation: the reference shows the record's rating in the Region tab's combo
and the handshake's in the covenant tab and the location bar.

### The two capabilities

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `DispatchRegionInfo` | granted | not granted | each flavour as its grid |
| … a resident's write (the values it read) | HTTP **403**, `Forbidden`, no LLSD; nobody told | — | the same on `FakeSl` |
| … an estate manager's write | not measured | — | applied as `setregioninfo` would be, the two extra settings with it |
| `RegionSchedule` | granted | not granted | each flavour as its grid |
| … a read, by a resident, with no schedule | HTTP 200, `undef` | — | the same on `FakeSl` |
| … a resident's write | HTTP **403**, `Forbidden: You don't have permission to make this change.` | — | the same on `FakeSl` |
| … an estate manager's write | not measured | — | stored, and read back |

### Changing the settings

A resident's `setregioninfo` and `setregiondebug` (the values it read) are
answered with nothing on both grids. The rest needs estate rights, so it was
measured on OpenSim only; the fake grid's Second Life flavour keeps OpenSim's
answer.

| A, the estate owner | OpenSim | fake grid |
| --- | --- | --- |
| saves the Region tab with every setting flipped (`setregioninfo`) | a `RegionInfo` to **both** avatars, then each one's next `SimStats` carries the changed flags; no handshake | the same |
| … what the record keeps | every flipped flag, the agent limit and the bonus; the fly-over and search settings are not sent over UDP | the same (over the capability on `FakeSl`, which keeps them too) |
| … restored | the record is as before | the same |
| switches scripts off on the Debug tab (`setregiondebug`) | **no** `RegionInfo`: only each avatar's next `SimStats` says `SKIP_SCRIPTS`; a record read after carries it | the same |
| … and back on | the same, the bit cleared | the same |

## God tools

Measured on 2026-10-18 by the census `god-tools`, three avatars a run: **A** a
grid god where a test account is one, **B** a resident, **C** the estate owner
where the test avatars hold estate rights. On OpenSim A is an account of user
level 250 (the local grid sets `allow_grid_gods`; OpenSim's default does not)
and C the estate owner, all three at the Default Region's centre with a cube of
A's beside them; the parcel there is C's. On aditi no test account is a god and
none holds estate rights, so all three are residents at the build location in
Mauve, and every god's leg reads "not measured: no god account on Second Life".
Each run is held to these answers; the census runs on the live grids only, as
the fake grid answers the requests for powers and the capability but none of
the god's actions (`server-fake-grid-god-tools`).

### Asking for powers

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a resident asks (`RequestGodlikePowers`, godlike) | an `AlertMessage` keyed `SpecialPowersRequestFailedLogged`: "Request for special powers failed. This request has been logged." | an agent alert, "/Request for god powers denied" | each flavour as its grid |
| … gives them up | nothing | nothing | nothing |
| a god asks | not measured | `GrantGodlikePowers` at its user level (250) | the same on `FakeOpensim` (`AccountConfig::god_level`) |
| … gives them up | not measured | `GrantGodlikePowers` at level 0 | the same |
| the estate owner asks | not measured (a resident's refusal) | `GrantGodlikePowers` at **220** (`region_owner_is_god`) | the same on `FakeOpensim` |
| who else is told | nobody | nobody | nobody |

OpenSim sends a grant only when the level its viewer was last told changes
(`GodController.RequestGodMode`), so a resident giving up powers it never had is
told nothing.

### What a resident is answered

B, holding no powers, acts on A or on the region.

| B | Second Life | OpenSim |
| --- | --- | --- |
| freezes A (`GodKickUser` over UDP) | nothing | nothing |
| freezes A over `UntrustedSimulatorMessage` | HTTP 200, an `undef` body; nothing done | HTTP 200, no body; nothing done |
| … naming A as the god, with its own session | HTTP 200, `undef` | HTTP **401**, no body |
| posts a message the capability does not know | HTTP **404**, `Not Found` | HTTP **400**, no body |
| sends a `GodlikeMessage` (`telehub`, `info ui`) | nothing | nothing |
| rewrites the region (`GodUpdateRegionInfo`, with the values it states) | nothing | nothing |
| returns A's objects (`SimWideDeletes`) | nothing | nothing |
| forces the parcel's owner to itself (`ParcelGodForceOwner`) | nothing, the parcel unchanged | nothing, unchanged |
| marks the parcel as content (`ParcelGodMarkAsContent`) | nothing | an alert, "Request denied. You're not priviliged." |
| forces the owner of A's cube to itself (`ObjectOwner`, override) | nothing, the cube A's | nothing, the cube A's |

The fake grid answers the capability as each grid does
(`UntrustedMessageStyle`): OpenSim's statuses without a body, Second Life's
`undef` and `404`.

### What a god does

OpenSim only. A grid god is a god in OpenSim's permission checks
(`Permissions.IsGod` is "an administrator": user level 200 and up, or the
region's owner) **whether or not it is in god mode**; only the parcel's mark as
content looks at god mode.

| A | OpenSim |
| --- | --- |
| out of god mode, freezes B (over the capability) | A: agent alert "/User Frozen"; B: an agent alert with the reason, "/" before it |
| out of god mode, forces the owner of its cube to B | done, nobody told |
| in god mode, freezes B, over UDP or the capability | as out of it |
| unfreezes B, either way | A: "/User Unfrozen"; B: the reason |
| kicks B | B: `KickUser` with the reason, then the circuit closes; A and C nothing |
| C (estate owner, god mode 220) kicks A (250) | A: "/Kick from C ignored, kick reason: …" (C's id in the text); C nothing |
| kicks everyone (the all-agents id) | B and C kicked with the reason; A not |
| sends the `GodlikeMessage` | nothing — OpenSim handles no `GodlikeMessage` |
| rewrites the region (`GodUpdateRegionInfo`) | nothing — no handler |
| returns B's objects (`SimWideDeletes`) | nothing — the handler is not subscribed |
| forces the parcel's owner to itself, and back | A: the parcel's properties, owned by A, then C; B and C nothing |
| marks the parcel as content | A: an alert, "Please check ini files.\n[LandManagement] config section." (no default owner configured), the parcel unchanged |
| forces the cube's owner to B, and back | done, nobody told |

The reference sends the god kick, freeze and unfreeze over
`UntrustedSimulatorMessage` (its `message.xml` flavours `GodKickUser` `llsd`),
the `KickFlags` as four big-endian bytes; this client does the same where the
capability is granted, and the UDP message otherwise. Second Life's god tools
are unmeasured: no test account is a god there.

## In the viewer

- Both restart notices are read into one `Event::RegionRestart`
  (`sl_proto::RegionRestartNotice`), and the viewer shows one countdown card
  for either, reset by each later notice, closed by a cancellation, a region
  change or the end of the session (`sl-viewer-notices/src/region_restart.rs`).
  The notice's alert is not also shown, as the reference's default
  countdown shows none.
- A keyed alert's parameters are read as the serialized LLSD they are
  (`AlertInfo::parameters`); they were read as `KEY=value` text before, which
  no grid sends, so a keyed alert showed without them.
- The region's flags are kept as last stated — by the handshake, a
  `RegionInfo` or a `SimStats` — in the session (`region_blocks_fly`) and the
  viewer (`SlRegionFlags`), so the status bar's fly, push, scripts and damage
  icons follow a change made while the agent stands in the region, which they
  did not when they read the handshake alone.
- The Region tab has the reference's "Block parcel fly over" and "Block land
  show in search" boxes, greyed where the region grants no
  `DispatchRegionInfo` (OpenSim): only that capability carries them, and Apply
  takes it where it is granted. An Apply that saves a rating other than the
  handshake's raises `RegionMaturityChange`, as the reference's does — on a
  region like Mauve, at every Apply.
- A kick ends the session with the grid's reason in
  `DisconnectReason::Kicked`; the viewer logs it and exits, as it does for any
  disconnect — showing the reason in a window that stays open is
  `viewer-disconnect-screen`. Freeze, eject and the messages are alerts and
  show as system messages.
- A grant of god powers is kept as the agent's god level
  (`sl-viewer-notices/src/god_mode.rs`, `AgentGodLevel`) and raises the
  reference's tip, `EnteringGodMode` at the new level or `LeavingGodMode` at
  the old one. Both grids' refusals show: Second Life's keyed alert from the
  catalogue, OpenSim's "/…" as a system message. The god tools floater itself
  is `viewer-god-tools`.
