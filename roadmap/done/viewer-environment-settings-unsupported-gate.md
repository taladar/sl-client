---
id: viewer-environment-settings-unsupported-gate
title: Grey the settings creators on a grid that cannot store settings
topic: viewer
status: done
origin: Live aditi / OpenSim run of the My Environments window (2026-09-09)
refs: [viewer-environment-my-environments, viewer-environment-fixed-editor]
---

Context: [context/viewer.md](../context/viewer.md).

The reference refuses to mint a settings item on a grid that cannot hold one:
`LLSettingsVOBase::createNewInventoryItem` opens with

```cpp
if (!LLEnvironment::instance().isInventoryEnabled())
{
    LL_WARNS("SETTINGS")
        << "Region does not support settings inventory objects." << LL_ENDL;
    LLNotificationsUtil::add("SettingsUnsuported");
    return;
}
```

and `LLFloaterMyEnvironment::refreshButtonStates` greys the add menu on the same
predicate, which is

```cpp
!gAgent.getRegionCapability("UpdateSettingsAgentInventory").empty() &&
!gAgent.getRegionCapability("UpdateSettingsTaskInventory").empty()
```

We have neither the gate nor the notification. Add both: the New Sky / New Water
row of [[viewer-environment-my-environments]], the inventory's settings create
entries, and the editors' Save / Save As should all be inert with a reason on a
grid that advertises neither cap.

## What this does *not* solve, so nobody expects it to

It would **not** have caught the failure that prompted it. OpenSim registers
both caps (`BunchOfCaps.cs` —
`RegisterSimpleHandler("UpdateSettingsAgentInventory", …)` and the task variant
beside it), so it passes `isInventoryEnabled` and always did; what it lacked was
a `settings` arm in the *upload* handler, and creation no longer goes through
that path at all. This is parity for a grid that genuinely does not do settings,
not a guard against a grid that half does.

## The plumbing exists

`SlCapabilities(map)` is already published as a message on every capability-map
arrival, and `sl-viewer-platform/src/environment_assets.rs` shows the pattern
for picking one cap out of it. Mirroring "are both settings caps present" into a
resource is the whole of the new machinery.

The notification wants a `SettingsUnsuported` catalogue entry (the reference's
spelling, typo included — it is the template *name*, matched on the wire of its
own XML, so renaming it would be a divergence for no gain).

Reference (Firestorm, read-only): `llsettingsvo.cpp`,
`llfloatermyenvironment.cpp` (`refreshButtonStates`), `llenvironment.cpp`
(`isInventoryEnabled`), `menu_settings_add.xml`
(`MyEnvironments.EnvironmentEnabled`).

## Done (2026-09-11)

**The predicate needed a capability this client had never asked for.** The
reference's `isInventoryEnabled` is an `&&` over *both* settings caps, and this
client requested only the agent one — so asked as written the answer would have
been "no settings grid" everywhere, including Second Life. So
`CAP_UPDATE_SETTINGS_TASK_INVENTORY` joins `REQUESTED_CAPABILITIES`, and the
simulator side serves it: it is the settings sibling of the notecard task cap
and shares its `{ task_id, item_id }` two-stage upload body, so it is one entry
in `UPLOAD_CAPABILITIES` / `SERVED_CAPABILITIES`, one arm on the metadata
parser, one row in the pinned coverage table, and one more granted cap (58 →
59). That also keeps the offline fake grid on the supported side of the
gate, which it would not have been had the client asked for a cap nothing
served.

**One resource, read by five surfaces.**
`inventory_actions::SettingsInventorySupport` folds every `SlCapabilities` map
through `settings_caps_present` (the `&&`, on its own so it can be tested
without an ECS) and is `false` until the seed caps arrive. It lives in the
inventory crate because what it gates is a *create*, and both the inventory and
the environment editors sit above it. Each window's plugin `init_resource`s it
too, so a host without the inventory (the gallery) reads "no settings grid" and
draws the windows honestly greyed rather than panicking on a missing resource.

What reads it:

- The inventory **+ ▸ New Settings** entries, greyed through a new
  `can-create-settings` menu condition on the add button's host (the gear
  button's pattern, one host over).
- **My Environments**' bottom row — the three creators and the trash, which is
  what the reference's `refreshButtonStates` greys on `settings_ok`.
- The **sky / water editors**' Save and Save As. Import and Revert stay live:
  neither writes to the grid, and an imported preset is still previewable.
- The **day-cycle editor**'s Save and Save As, through `action_enabled`.
- The **WindLight bulk import**, refused before the folder chooser — every
  preset in the folder would be a create the grid drops.

Each of those also refuses in its press handler with the reference's
`SettingsUnsuported` notification (already in the catalogue) and a status line,
because greying is a frame behind a region cross: the press that gets through is
the race, and the reference answers it in exactly that place.

**Two fixes found on the way.** The inventory's **New Day Cycle** entry was
routed nowhere — `handle_inventory_add_actions` matched `new-sky` and
`new-water` and not `new-daycycle`, so the third entry had always been inert.
And the disabled-button paint that three windows in this crate now need is one
`rows::paint_action_button`, which the day-cycle editor's `paint_button` became
a colour-picking wrapper over: `InteractionDisabled` is advisory in Bevy, so the
half that actually *shows* a disabled button must not be able to drift between
windows.

**Not done here** (reference sites this task did not name, left as gaps rather
than half-gated): the folder context menu has no New Settings submenu at all in
this viewer (the reference's `llinventorybridge.cpp` disables one), and the
notecard reader's embedded-item copy does not refuse a settings item on such a
grid (the reference's `SOURCE_NOTECARD` drag test, `NoEnvironmentSettings`).

## Verified

`cargo test -p sl-proto --test sim_caps` (78 passed) — the seed grant (59 caps,
the pinned coverage table in order) and a new `UpdateSettingsTaskInventory`
two-stage upload round trip, carrying the holding object and item.

`cargo test -p sl-viewer-inventory --lib` — the `&&` pinned against each
single-cap map, and the three New Settings entries pinned to the
`can-create-settings` condition.

`cargo test -p sl-viewer-environment --lib` (79 passed) — the day-cycle
editor's `action_enabled` losing both saves (and nothing else) on a grid
without the caps, a bulk-import run refused before the chooser with the
reference's notification, and the crate's plugin-scheduling sweep with the new
systems in it.

Not verified live: the greyed state itself needs a grid that serves neither cap,
which neither aditi nor the local OpenSim is — both serve both. The path a live
run *can* show is the un-greyed one, which is the state before this change.
