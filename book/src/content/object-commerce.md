# Object Interaction & Commerce

Beyond the editing surface in [3D World Information](world.md) (rez, move,
link, set permissions), a viewer can *buy* objects and their contents, ask what
an object costs to pay, spin an object interactively, and rez objects through
the raycast and notecard paths. This chapter covers those commerce and
interaction messages.

## Buying

An object offered for sale advertises a **sale type** (`SaleType`: not for
sale, the original, a copy, or its contents) and a price. To buy it the client
sends `ObjectBuy` (`Command::BuyObject`) listing one or more
`ObjectBuyItem`s — each the object's region-local id plus the advertised sale
type and price, which must match what the object reports or the simulator
rejects the purchase. A derezed purchase is delivered into the inventory folder
named in the command.

To buy a single item out of an object's *contents* rather than the object
itself, the client sends `BuyObjectInventory` (`Command::BuyObjectInventory`)
with the object, the item, and the destination folder; on success the simulator
copies the item into the agent's inventory.

There is no dedicated success reply — inventory and the world update the usual
way (an `UpdateCreateInventoryItem`, an `ObjectUpdate`/`KillObject`).

## Pay prices

Before paying a scripted object (a vendor, a tip jar), a viewer asks for its
button layout with `RequestPayPrice` (`Command::RequestPayPrice`). The simulator
answers with `PayPriceReply` (`Event::PayPriceReply`): a default pay amount and
a list of quick-pay button amounts. LL's convention uses `-1`/`-2` for
"hide"/"default" buttons, so a button amount can be negative.

## Condensed properties (the property family)

The full [`ObjectProperties`](world.md) reply needs the object selected. For the
lighter owner/permissions/sale summary a viewer shows on hover or in the pay and
abuse-report dialogs, it sends `RequestObjectPropertiesFamily`
(`Command::RequestObjectPropertiesFamily`) — no prior selection needed — and the
simulator answers with `ObjectPropertiesFamily`
(`Event::ObjectPropertiesFamily`). The `request_flags` field (e.g.
`OBJECT_PAY_REQUEST` `0x04`) is echoed back so the viewer can route the reply to
the dialog that asked.

## Spinning

A viewer's grab tool can spin (rotate) an object in place: `ObjectSpinStart`
(`Command::SpinObjectStart`) begins, `ObjectSpinUpdate`
(`Command::SpinObjectUpdate`) streams the latest rotation as the user drags, and
`ObjectSpinStop` (`Command::SpinObjectStop`) ends. The object is identified by
its full id.

## Raycast and notecard rez

Two further rez paths complement the inventory rez and the offset
[`DuplicateObjects`](world.md):

- **Duplicate on ray** (`ObjectDuplicateOnRay`,
  `Command::DuplicateObjectsOnRay`): copy the selected objects and drop the
  copies against the surface a ray hits — the "copy and place in world" gesture.
  The ray (`ray_start` → `ray_end`, an optional `ray_target_id`) places the
  copies exactly; `copy_centers` and `copy_rotates` control how each copy's
  offset and rotation carry over.
- **Rez from notecard** (`RezObjectFromNotecard`,
  `Command::RezObjectFromNotecard`): rez one or more objects embedded as
  inventory items inside a notecard asset, placed with the same ray fields and
  given the supplied permission masks.
- **Restore to world** (`RezRestoreToWorld`, `Command::RezRestoreToWorld`):
  return an inventory item to the exact position it last occupied in-world. The
  message is `UDPDeprecated`, but a viewer may still send it; the full inventory
  item (with its permissions, sale info and CRC) travels in a `RestoreItem`.

## Server side

A simulator built on `SimSession` decodes every inbound message above into a
matching `ServerEvent` (`BuyObject`, `BuyObjectInventory`, `RequestPayPrice`,
`RequestObjectPropertiesFamily`, `SpinObjectStart`/`SpinObjectUpdate`/
`SpinObjectStop`, `DuplicateObjectsOnRay`, `RezRestoreToWorld`,
`RezObjectFromNotecard`) and can answer the two queries with
`SimSession::send_pay_price_reply` and
`SimSession::send_object_properties_family`.

---

> **In this codebase**
>
> - Types are in `sl-proto/src/types/editing.rs` (`ObjectBuyItem`,
>   `NotecardRez`, `RestoreItem`, and the existing `SaleType`) and
>   `sl-proto/src/types/object.rs` (`ObjectPropertiesFamily`).
> - Commands `BuyObject`, `BuyObjectInventory`, `RequestPayPrice`,
>   `RequestObjectPropertiesFamily`, `SpinObjectStart`, `SpinObjectUpdate`,
>   `SpinObjectStop`, `DuplicateObjectsOnRay`, `RezRestoreToWorld`,
>   `RezObjectFromNotecard`; the `Session` methods are `buy_object`,
>   `buy_object_inventory`, `request_pay_price`,
>   `request_object_properties_family`, `spin_object_start`,
>   `spin_object_update`, `spin_object_stop`, `duplicate_objects_on_ray`,
>   `rez_restore_to_world`, `rez_object_from_notecard`; the wire encoders are
>   the matching `send_*` functions in `sl-proto/src/session/circuit.rs`.
> - Events `PayPriceReply` and `ObjectPropertiesFamily` are decoded in
>   `sl-proto/src/session/methods.rs`.
> - Server events of the same names, plus `send_pay_price_reply` and
>   `send_object_properties_family`, are in `sl-proto/src/sim_session.rs`.
> - REPL commands `buy_object`, `buy_object_inventory`, `request_pay_price`,
>   `request_object_properties_family`, `spin_object_start`,
>   `spin_object_update`, `spin_object_stop`, `duplicate_objects_on_ray`,
>   `rez_restore_to_world`, `rez_object_from_notecard` (sale types accept a name
>   such as `copy` or a numeric code).
