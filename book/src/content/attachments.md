# Attachments

An **attachment** is an object worn on the avatar: rezzed onto an *attachment
point* — a body joint (hand, chest, skull, …) or one of the eight **HUD** slots
that float in front of the agent's own screen. Attachments are distinct from
[wearables](appearance.md) (the clothing/body layers worn in `WearableType`
slots) and from ordinary in-world objects, though an attachment can move freely
between those states: you can attach an object that is already rezzed in-world,
or rez one straight from inventory, and you can detach it back to inventory or
drop it onto the ground.

## Attachment points

`AttachmentPoint` enumerates Linden Lab's attachment-point list (the viewer's
`avatar_lad.xml`): `Default` (code `0`, "wherever the item was last worn"), the
body joints (codes `1`–`30`, `39`–`55`), and the HUD slots (codes `31`–`38`,
recognised by `AttachmentPoint::is_hud`). Unknown/future codes round-trip as
`Other(u8)`.

On the wire the point shares a byte with an **add** flag (`ATTACHMENT_ADD`,
`0x80`): with it set the object is *added* to the point alongside anything
already there; without it, the new object *replaces* what is worn there. The
flag is modelled separately from the point as an `AttachmentMode` enum
(`Add` / `Replace`) — every attaching command carries a `mode:
AttachmentMode` — and `AttachmentPoint::with_mode` / `split_code` combine and
separate the two.

## Attaching and detaching

The client can:

- **Attach an in-world object** it has selected (`Command::AttachObject`, the
  `ObjectAttach` message): by region-local id, to a point, at a rotation.
- **Wear from inventory** a single item (`Command::RezAttachment`,
  `RezSingleAttachmentFromInv`) or several at once
  (`Command::RezAttachments`, `RezMultipleAttachmentsFromInv`, whose
  `DetachOrder` says whether to first detach everything currently worn or keep
  it). Both take a `RezAttachment` describing the item, owner, point and
  attachment mode.
- **Detach back to inventory** by region-local id (`Command::DetachObjects`,
  `ObjectDetach`) or by inventory item id
  (`Command::DetachAttachmentIntoInventory`, `DetachAttachmentIntoInv`).
- **Drop onto the ground** by region-local id (`Command::DropAttachments`,
  `ObjectDrop`): the object becomes an ordinary in-world prim at the avatar's
  location.

There is no dedicated reply message for these: the region confirms a change the
usual way, by pushing the affected object's `ObjectUpdate` (and, for what others
see, the attachment list inside `Event::AvatarAppearance`). When an object is
attached the region also kills its in-world copy.

The server side mirrors every inbound attachment message as a `ServerEvent`
(`AttachObject`, `DetachObjects`, `DropAttachments`, `RezAttachment`,
`RezAttachments`, `DetachAttachmentIntoInventory`), so a simulator built on
`SimSession` observes exactly what a client wears.

## `RemoveAttachment` is not a viewer message

`message_template.msg` carries a `RemoveAttachment` (Low 332) that takes an
attachment point and an inventory item id, and it looks like the by-item-id
detach — but it is **not one a viewer may send**. Its template comment reads
"Simulator informs Dataserver that attachment has been taken off", the
counterpart of the `UpdateAttachment` (Low 331) directly above it, which spells
the direction out as "DO NOT ALLOW THIS FROM THE VIEWER". It travels
simulator → dataserver, not viewer → simulator.

Everything downstream agrees. OpenSim's `LLClientView` registers no handler for
it (`RemoveAttachment` in that tree is an internal `ScenePresence` method, not a
packet); the reference viewer never sends it — the name occurs only in the
generated prehash table. Sending it live is silently dropped: four such packets
were acknowledged with no error and all four attachments were still worn on the
next login, while `DetachAttachmentIntoInv` detached the same four at once.

So there is no client command, `Session` method or wire encoder for it here, and
`SimSession` does not decode it either. To take off a worn item by its item id,
use `Command::DetachAttachmentIntoInventory`; by region-local id, use
`Command::DetachObjects`. The message stays in `sl-wire`, which mirrors the
whole template.

---

> **In this codebase**
>
> - Types are in `sl-proto/src/types/appearance.rs`: `AttachmentPoint` (with
>   `to_code` / `from_code` / `with_mode` / `split_code` / `is_hud`),
>   `AttachmentMode` (`Add` / `Replace`), `DetachOrder` (`DetachAllFirst` /
>   `Keep`, the `RezAttachments` `FirstDetachAll` flag), and `RezAttachment`.
>   The attachment list on `AvatarAppearance` uses `AvatarAttachment`.
> - Commands `AttachObject`, `DetachObjects`, `DropAttachments`,
>   `RezAttachment`, `RezAttachments`, `DetachAttachmentIntoInventory`; the
>   `Session` methods are `attach_object`, `detach_objects`,
>   `drop_attachments`, `rez_attachment`, `rez_attachments`,
>   `detach_attachment_into_inventory` (the last takes the attachment's
>   `InventoryKey`); the wire encoders are `send_object_attach` /
>   `send_object_detach` / `send_object_drop` / `send_rez_single_attachment` /
>   `send_rez_multiple_attachments` in `sl-proto/src/session/circuit.rs`.
> - Server events of the same names are decoded in
>   `sl-proto/src/sim_session.rs`.
> - REPL commands `attach_object`, `detach_objects`, `drop_attachments`,
>   `rez_attachment`, `rez_attachments` (attachment points accept a name such as
>   `righthand` or `hudtopright`, or a numeric code).
