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
  `ObjectDetach`) or by inventory item id (`Command::RemoveAttachment`,
  `RemoveAttachment`, or the equivalent
  `Command::DetachAttachmentIntoInventory`, `DetachAttachmentIntoInv` — a
  distinct wire message the viewer also uses to detach a worn attachment into
  inventory by its item id).
- **Drop onto the ground** by region-local id (`Command::DropAttachments`,
  `ObjectDrop`): the object becomes an ordinary in-world prim at the avatar's
  location.

There is no dedicated reply message for these: the region confirms a change the
usual way, by pushing the affected object's `ObjectUpdate` (and, for what others
see, the attachment list inside `Event::AvatarAppearance`). When an object is
attached the region also kills its in-world copy.

The server side mirrors every inbound attachment message as a `ServerEvent`
(`AttachObject`, `DetachObjects`, `DropAttachments`, `RemoveAttachment`,
`RezAttachment`, `RezAttachments`, `DetachAttachmentIntoInventory`), so a
simulator built on `SimSession` observes exactly what a client wears.

---

> **In this codebase**
>
> - Types are in `sl-proto/src/types/appearance.rs`: `AttachmentPoint` (with
>   `to_code` / `from_code` / `with_mode` / `split_code` / `is_hud`),
>   `AttachmentMode` (`Add` / `Replace`), `DetachOrder` (`DetachAllFirst` /
>   `Keep`, the `RezAttachments` `FirstDetachAll` flag), and `RezAttachment`.
>   The attachment list on `AvatarAppearance` uses `AvatarAttachment`.
> - Commands `AttachObject`, `DetachObjects`, `DropAttachments`,
>   `RemoveAttachment`, `RezAttachment`, `RezAttachments`,
>   `DetachAttachmentIntoInventory`; the `Session` methods are `attach_object`,
>   `detach_objects`, `drop_attachments`, `remove_attachment`, `rez_attachment`,
>   `rez_attachments`, `detach_attachment_into_inventory` (the last takes the
>   attachment's `InventoryKey`); the wire encoders are `send_object_attach` /
>   `send_object_detach` / `send_object_drop` / `send_remove_attachment` /
>   `send_rez_single_attachment` / `send_rez_multiple_attachments` in
>   `sl-proto/src/session/circuit.rs`.
> - Server events of the same names are decoded in
>   `sl-proto/src/sim_session.rs`.
> - REPL commands `attach_object`, `detach_objects`, `drop_attachments`,
>   `remove_attachment`, `rez_attachment`, `rez_attachments` (attachment points
>   accept a name such as `righthand` or `hudtopright`, or a numeric code).
