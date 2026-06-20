# Appearance

Appearance is how an avatar looks: the **wearables** and **attachments** it has
on, and the **baked textures** that combine them into the final rendered skin.
This is one of the more involved subsystems because the heavy lifting (baking)
moved from the client to the server over the years.

## Wearables and attachments

- A **wearable** is an inventory item worn in a slot. Slots come in two kinds:
  *body parts* (shape, skin, hair, eyes — exactly one each) and *clothing/extra*
  layers (shirt, pants, shoes, jacket, gloves, skirt, alpha, tattoo, physics,
  universal, …, which can stack). Each wearable references the inventory item
  that defines it.
- An **attachment** is an object rezzed onto an attachment point on the avatar's
  body (or HUD). Attaching, detaching, dropping and wearing-from-inventory have
  their own [attachments](attachments.md) chapter.

The client tells the region what it is wearing (`Command::SetWearing`) and can
fetch the current set (`Command::RequestWearables`, → `Event::AgentWearables`).

## Baking: client-side vs server-side

To render an avatar, the layered wearables must be composited into a small set
of **baked textures** (head, upper body, lower body, …). Historically the
*client* did this and uploaded the results; modern Second Life does it
*server-side*:

- **Legacy** — the client composites and uploads via the `UploadBakedTexture`
  capability, then tells the region the resulting texture ids.
- **Server-side ("Sunshine")** — the client sends its wearables/visual params
  and the region bakes, via the `UpdateAvatarAppearance` capability. The client
  requests this with `Command::RequestServerAppearanceUpdate`.

Other avatars' finished appearance arrives as `Event::AvatarAppearance` (the
baked texture set and visual parameters); the client can also answer
cached-texture queries (`Command::RequestCachedTextures` →
`Event::CachedTextureResponse`) so unchanged bakes are not recomputed.

## Animations

Animations are assets the avatar plays. The client starts/stops them
(`Command::PlayAnimation` / `StopAnimation`, or `SetAnimations` for a batch),
and sees what others are playing through `Event::AvatarAnimation` (see also the
[avatars](world.md#avatars-in-the-region) section of the world chapter).

---

> **In this codebase**
>
> - Types are in `sl-proto/src/types/appearance.rs`: `WearableType`, `Wearable`,
>   `AvatarAppearance`, `AvatarAttachment`, `PlayingAnimation`. Texture-entry
>   (de)serialization is `decode_texture_entry` / `encode_texture_entry` in
>   `sl-proto/src/appearance.rs`.
> - Caps `CAP_UPDATE_AVATAR_APPEARANCE` (`UpdateAvatarAppearance`) and
>   `CAP_UPLOAD_BAKED_TEXTURE` (`UploadBakedTexture`); the LLSD request builders
>   are in `sl-wire/src/llsd.rs` (`build_update_avatar_appearance_request`,
>   `build_upload_baked_texture_request`); the driver is
>   `sl-client-tokio/src/appearance.rs`.
> - Commands `SetWearing`, `RequestWearables`, `SetAppearance`,
>   `RequestServerAppearanceUpdate`, `RequestCachedTextures`, `PlayAnimation`,
>   `StopAnimation`, `SetAnimations`; events `AvatarAppearance`,
>   `AgentWearables`, `ServerAppearanceUpdate`, `CachedTextureResponse`,
>   `AvatarAnimation`.
