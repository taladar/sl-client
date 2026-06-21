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

## Gestures

A gesture is an inventory asset that chains animations, sounds, chat and waits,
fired by a trigger word or keybinding. Uploading the gesture asset (over the
inventory/asset path) only stores it; the simulator also needs to know which
gestures are *active* for the session so it can preload them and watch for their
triggers. The client toggles that with `Command::ActivateGestures` (each entry
pairs the gesture's inventory item id with its asset id) and
`Command::DeactivateGestures` (by item id). Both are fire-and-forget — there is
no reply.

## Agent state and viewport

Beyond appearance, the viewer keeps the simulator informed about the agent's
movement mode and the physical viewport it is rendering into. None of these
have replies:

- **Run vs. walk** — `Command::SetAlwaysRun { always_run }` chooses whether
  ground movement runs or walks (`SetAlwaysRun`).
- **Pause / resume** — when the viewer stalls (a modal dialog, a long file
  operation) it sends `Command::PauseAgent` so the simulator stops streaming
  updates, then `Command::ResumeAgent` when it is reading the network again.
  Both carry a single monotonic serial number (shared between the two messages);
  the simulator ignores non-increasing values, so the session bumps it on every
  pause *and* resume.
- **Field of view** — `Command::SetAgentFov { vertical_angle }` reports the
  camera's vertical FOV in radians (`AgentFOV`); the simulator uses it for
  interest-list culling.
- **Window size** — `Command::SetAgentSize { height, width }` reports the
  viewport size in pixels (`AgentHeightWidth`), sent when the window is created
  or resized.

The opposite direction carries scripted control of the agent. After the agent
grants a script `PERMISSION_TAKE_CONTROLS` (see
[scripts](world.md) / the permission events), the simulator sends
`ScriptControlChange` — surfaced as `Event::ScriptControlChange` — telling the
client which movement controls the script is taking or releasing and whether
they still drive the avatar. The client can forcibly hand them all back with
`Command::ReleaseScriptControls` (`ForceScriptControlRelease`). Similarly, a
script granted `PERMISSION_CONTROL_CAMERA` drives the follow-camera via
`Event::SetFollowCamProperties` (a list of `llSetCameraParams`
parameter/value pairs) and releases it with `Event::ClearFollowCamProperties`.

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
> - Gestures: `Command::ActivateGestures` (with the `GestureActivation`
>   item-id/asset-id pairs) and `Command::DeactivateGestures`
>   (`Session::activate_gestures` / `deactivate_gestures`). A simulator sees
>   them as `ServerEvent::ActivateGestures` / `DeactivateGestures`.
> - Agent state: commands `SetAlwaysRun`, `PauseAgent`, `ResumeAgent`,
>   `SetAgentFov`, `SetAgentSize` (`Session::set_always_run` / `pause_agent` /
>   `resume_agent` / `set_agent_fov` / `set_agent_size`). The pause/resume
>   serial is the circuit's `pause_serial_num`. A simulator sees these as
>   `ServerEvent::SetAlwaysRun` / `AgentPause` / `AgentResume` / `AgentFov` /
>   `AgentHeightWidth`.
> - Scripted controls and camera: `Command::ReleaseScriptControls`
>   (`ForceScriptControlRelease`); events `Event::ScriptControlChange`
>   (`Vec<ScriptControl>`), `Event::SetFollowCamProperties` /
>   `ClearFollowCamProperties`. The `ScriptControl`, `FollowCamProperty` and
>   `FollowCamPropertyValue` types live in `sl-proto/src/types/script.rs`; a
>   simulator emits them with `SimSession::send_script_control_change` /
>   `send_set_follow_cam_properties` / `send_clear_follow_cam_properties` and
>   sees `ServerEvent::ForceScriptControlRelease`.
