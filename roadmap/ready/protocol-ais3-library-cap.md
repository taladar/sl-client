---
id: protocol-ais3-library-cap
title: An AIS3 request for a library folder goes to the wrong capability
topic: protocol
status: ready
origin: measured doing object-asset-format on aditi (2026-09-06)
points: 2
refs: [protocol-ais3-nested-embedded]
---

Second Life serves the shared **Library** inventory over its own capability,
`LibraryAPIv3`, alongside the agent's own `InventoryAPIv3`. This workspace
handles both on the *reply* side — `sim_caps.rs` maps them to the same
`CapHandler::Ais3` and `methods.rs` tags the resulting folders and items with
`InventoryOwner::Library` — but every AIS3 *request* command
(`Ais3FetchFolderChildren` and its siblings, `sl-client-tokio/src/lib.rs`)
looks up `CAP_INVENTORY_API_V3` only.

So a library folder id sent to the inventory capability is a 404, observed on
aditi 2026-09-06:

```text
WARN sl_client_tokio::http: a CAPS GET was rejected
     capability="InventoryAPIv3" status=404 Not Found
```

The fix is to route by the folder's owner: a request for a folder known to be
the library's — or descended from the library root the login response names —
goes to `CAP_LIBRARY_API_V3`. The command carries only a folder id today, so
either the id is resolved against the cached tree before the request, or the
command grows an owner field.

Worth doing because the library is the one part of an account's inventory that
is **full-permission by construction**, which makes it the control group for
any question of the form "does the grid treat this item differently because of
its permissions" — `object-asset-format` wanted exactly that and could not
have it.

Acceptance: a library folder's children fetch on Second Life, and the items
arrive tagged as the library's.
