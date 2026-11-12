---
id: server-fake-grid-scripted-avatar-clothes-not-shown
title: A dressed scripted avatar is drawn without its clothes
topic: server
status: done
origin: server-fake-grid-scripted-avatars (2026-11-12)
refs: [server-fake-grid-scripted-avatars]
---

Context: [context/lsl.md](../context/lsl.md).

## Observation

The `scripted-avatar` scene's `sitter()` is `dressed()` (its account wears
pants and shoes over its body parts), yet an `sl-crosscheck` capture of the
scene showed it with the bare body. The scripted avatar is an
`sl-client-tokio` client that does nothing about its appearance after login:
it sends no `AgentSetAppearance` and no bake request, so whether the grid
bakes its outfit, and what other viewers are told, is unexamined.

## Wanted

Find whether the grid's central bake of a dressed account needs the client's
trigger, or the viewer is not drawing a baked outfit it was sent; a dressed
scripted avatar shows its clothes to another viewer.

## Resolution (2026-11-12)

Neither guess: the grid did bake the dressed outfit at arrival, without any
trigger from the client, and the viewer drew what it was sent. The bake
itself left the clothes off. The dressed fixture's pants and shoes name no
texture, and `sl-bake` dropped any worn layer whose wearable names none, so
the lower-body bake held only the skin. The reference instead draws such a
layer with the slot's default (`LLViewerWearable::setTexturesToDefaults`,
`getDefaultTextureImageID`), and that default is `IMG_WHITE` for every
clothing and alpha layer.

`sl_proto::avatar_texture::default_layer_texture` now holds the reference's
per-slot defaults, `WearableAsset::drawn_layer_texture` gives a layer's texture
or that default (it replaces `supplies_layer`), and `sl-bake`'s
`assemble_layers` and `region_texture_ids` draw and name the default. The white
default is drawn without a fetch. The viewer's own client-side bake requests
the non-white defaults like any other layer texture, so on OpenSim it now
draws texture-less clothing too. `tests/scripted_avatars.rs` checks that a
viewer beside a dressed scripted avatar is shown a lower-body bake different
from an undressed one's, and it fails without the fix. An `sl-crosscheck`
capture of the `scripted-avatar` scene shows the sitter in white pants and
shoes.

On the OpenSim flavour the scripted avatar still shows the fixture's solid
placeholder bakes. There the client bakes, and the tokio client publishes
none, so a real OpenSim region would not show its clothes either.
