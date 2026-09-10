---
id: viewer-environment-settings-unsupported-gate
title: Grey the settings creators on a grid that cannot store settings
topic: viewer
status: ready
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
