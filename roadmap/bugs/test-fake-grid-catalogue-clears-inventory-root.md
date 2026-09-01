---
id: test-fake-grid-catalogue-clears-inventory-root
title: --catalogue clears the account inventory, so no SL-derived viewer can log in
topic: test
status: bugs
origin: first Firestorm cross-check harness run (2026-09-01)
points: 2
refs: [test-firestorm-crosscheck-runner, test-fake-grid-fixed-port-scenario, test-fake-grid-render-fixtures]
---

Context: [context/testing.md](../context/testing.md).

`--catalogue` replaces the stock scenario content with the named prim
catalogue — and takes the **account inventory** with it. The login response
then omits `inventory-root` entirely, even though the client asked for it.
Probed directly against both:

```text
with --catalogue          inventory-root=None
stock (no --catalogue)    inventory-root=[{'folder_id': '…-00000000fa01'}]
```

`inventory-lib-root` and `inventory-skeleton` disappear the same way.

That is fatal for any viewer descended from the Linden client. Firestorm's
`process_login_success_response` requires all five of `agent_id`,
`session_id`, `circuit_code`, `gFirstSim.isOk()` and
`gInventory.getRootFolderID().notNull()` (`llstartup.cpp:5303-5311`), and
returns false without them — reported to the user as a bare "Login failed."
after the login has otherwise *succeeded* (`handleLoginSuccess` runs, the
circuit is opened, `map-server-url` and the SLT timestamp are processed). The
viewer is left sitting on an empty black window with only the Viewer and Help
menus.

This matters more than a normal fixture gap: `--catalogue` is precisely the
flag [[test-firestorm-crosscheck-runner]] needs, because the catalogue is what
makes both viewers look at the same named objects. As it stands the one
configuration the cross-check exists to run is the one that cannot be logged
into.

The cause is in `sl-fake-grid/src/runtime.rs:647-652`: `inventory_root` is
`skeleton_of(sim.agent_inventory()).0.first()`, so an empty agent inventory
yields `None` rather than a root folder. The catalogue scenario should keep
the stock inventory skeleton — it is replacing *region* content, and the
account's inventory is not region content. Failing that, always synthesise a
root folder, since a login response without one is not a valid response to a
request that asked for it.

Worth a guard either way: a fake grid that answers `login: true` while omitting
a field the reference viewer treats as mandatory is worse than one that
refuses, because the failure surfaces far from its cause. Assert in
`sl-fake-grid` that a successful response carries every field the reference
viewer's success check requires.
