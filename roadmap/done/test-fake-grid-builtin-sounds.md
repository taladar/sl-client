---
id: test-fake-grid-builtin-sounds
title: The built-in UI sounds the viewer plays on arrival 404
topic: test
status: done
origin: noticed live-verifying test-fake-grid-animation-assets (2026-09-01)
points: 2
refs: [viewer-static-asset-library, test-fake-grid-builtin-textures]
---

Done (2026-09-08), the first way: **twelve** library ids answered with a
tone each, in the three places the texture half taught — the ids, the
bytes, and the grid that serves them each belonging somewhere different.

**`sl-proto` owns the ids.** They lived as UUID *strings* inside the
viewer's own `UiSound::default_asset`, where no grid fixture could reach
them and every one had to be re-parsed at every resolve. They are now
`UI_SOUND_{CLICK,TYPING,ALERT,INVALID_OP,MONEY_UP,MONEY_DOWN,
TELEPORT_OUT,SNAPSHOT,WINDOW_OPEN,WINDOW_CLOSE,IM_OR_OFFER,NEARBY_CHAT}`
in `sl-proto`'s new `sound` module, with `BUILTIN_UI_SOUNDS` over them,
re-exported through `sl-client-bevy` and `sl-client-tokio`.
`default_asset` returns a `Uuid` and the resolve chain no longer parses
anything. A viewer-side test pins the two ends together: the catalogue
and the shared list name the same twelve assets, in either direction —
a catalogue id missing from the list is a sound no fixture grid answers,
and a list id no entry names is bytes under a key nothing asks for.

**Twelve rather than the six the task counted.** Six is what an arrival
*prefetches* (the sounds enabled by default); the other six are fetched
the first time their event happens — a click, a window, an IM, a nearby
chat — which is a fetch that fails just as surely, only later and out of
sight of the arrival log.

**`sl-test-assets::builtin` owns the bytes.** `library_sounds()` writes
one Ogg Vorbis tone per id through the encoder
[[test-assets-sound-encoder]] built, at the pitch `ui_sound_pitch_hz`
gives it: a **whole-tone** series from A3 up, in the shared list's own
order, so the twelve span two octaves and end below A5. A whole tone
rather than a semitone because the point is telling one *by ear, in the
role* — a chime against a click against a shutter — and a semitone
between two unfamiliar quarter-second sounds is not something anyone
hears. Adding a sound to the list gives it the next pitch up rather than
renumbering the ones below it.

**The stock scenario serves them**, beside the twelve library textures,
the bump maps and the body parts. The library is now sound as well as
pixels.

**The second option in the task was already true.** `SoundCache` marks a
failed id `unavailable` and never re-requests it (only a capability
refresh re-arms one, for a genuinely transient failure), and sounds do
not go through the texture/mesh retry policy at all — so there was no
retry budget being burned here, and nothing to add. What a served-nothing
id cost was not log noise but the *whole session's* worth of that event
being silent: one failed fetch, then nothing ever again.

**Live-verified** against the stock scenario through `sl-crosscheck
--only sl-client --scenario stock`: the arrival logs six
`sound … decoded (0.25s, 1 ch)` lines — one per default-enabled sound,
which is what a prefetch asks for — and **no** `fetching sound … over
ViewerAsset` or `decoding sound …` warning at all. The other six resolve
the first time their event fires; that they are servable is the
end-to-end test's job, not an arrival's. That a played one is
*identifiable* is the part only an ear settles; what a run can pin is
that each is a distinct decodable quarter-second clip, and it does.

Covered by `sl-proto`'s two id tests (distinct, none nil), the viewer's
`the_catalogue_and_the_shared_list_agree`, `sl-test-assets`'
`every_built_in_ui_sound_stand_in_carries_its_own_pitch` (each stand-in
decoded back through `symphonium` — the decoder `sl-audio` plays clips
with — and required to be at least eight times louder at its own pitch
than at any of the other eleven) and its pitch-series test, the fake
grid's `the_stock_assets_hold_every_builtin_ui_sound`, and the
end-to-end `every_built_in_ui_sound_is_fetchable`, which pulls all twelve
over `ViewerAsset` through the real client stack and compares them
against the fixture bytes — so a *wrong* tone under an id fails there
rather than being noticed by ear months later.

Not covered: the reference viewer's own preload list is wider than our
catalogue (its footsteps, health, object-rez and Firestorm's pie-menu
sounds), so a Firestorm cross-check run still fetches a handful this
grid does not serve. Ours is the set this viewer can ask for; widening
it to the reference's is a separate call.

---

Context: [context/testing.md](../context/testing.md).

Arriving against the fake grid, `sl_viewer_platform::sound_cache` fails
six fetches — `104974e3-…`, `3d09f582-…`, `5e191c7b-…`, `77a018af-…`,
`a3f48b85-…`, `d7a9a565-…` — the built-in UI sounds a viewer plays for
its own events. A live grid serves them from its library; the fake grid
serves no asset it was not handed.

This task originally covered the built-in **animation** half as well
(`2408fe9e-…` `stand` and the rest). [[viewer-static-asset-library]]
closed that half: the viewer now ships Firestorm's 129 `.animatn` files
and every asset store answers from them before reaching the network, so
the animation fetches no longer happen at all.

The same answer is not available here. Firestorm ships **no** sounds —
its `static_assets` / `fs_static_assets` folders hold only animations,
wearables and gestures, and there is no `.wav`/`.ogg` anywhere in its
tree. So the bytes have to come from somewhere else:

- serve a synthetic sound per id from `scenario::default_assets`, the
  way [[test-fake-grid-builtin-textures]] now does for the sky and prim
  textures — a fake grid *is* a grid with a library, so answering a
  library id is honest, and that task settled the precedent; or
- teach `sound_cache` that a known built-in with no asset is not worth
  retrying, which removes the noise without inventing bytes, and is
  probably wanted regardless of the above.

Whichever way, `sl-audio` needs a decodable container: the reference's
built-in sounds are Ogg Vorbis, so a synthetic one has to be too — an
empty blob would fail to *decode* instead of failing to fetch, which is
no improvement.

That half is now available: [[test-assets-sound-encoder]] added
`sl-test-assets::sound::marker_tone`, a real Ogg Vorbis tone the fake
grid can serve under any id. So what is left here is the *decision*
above — six library ids answered with a tone each, or a `sound_cache`
that stops retrying a built-in nothing serves — plus a pitch per id if
the first is chosen, so a played built-in is identifiable by ear.

Acceptance: an arrival against the stock scenario logs no failed sound
fetch, and the UI sounds either play or are known-silent by design.
