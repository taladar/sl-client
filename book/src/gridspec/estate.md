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
- A kick ends the session with the grid's reason in
  `DisconnectReason::Kicked`; the viewer logs it and exits, as it does for any
  disconnect — showing the reason in a window that stays open is
  `viewer-disconnect-screen`. Freeze, eject and the messages are alerts and
  show as system messages.
