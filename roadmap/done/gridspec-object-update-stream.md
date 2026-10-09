---
id: gridspec-object-update-stream
title: Object update forms, the interest list and kills on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-world-ecs-store, protocol-cap-interest-list,
  server-fake-grid-object-update-forms, server-world-update-scheduling,
  gridspec-object-rez-derez, viewer-automation-set-a-setting,
  viewer-area-search, viewer-network-debug-tools]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-09)

Measured and written up in `book/src/gridspec/objects.md` § Updates.

- **Discover.** `object-update-decode` rewritten as a census in legs — the
  arrival, the draw distance down to 32 m, the camera to the far corner,
  the interest-list mode to `360` and back, the camera and the draw distance
  back — and a new `object-handshake-flags`, a login for each of five
  combinations of `RegionHandshakeReply` flags. Both read every message off
  a new event. Four and two runs on aditi (Ahern), three and two on
  OpenSim.
- **Findings.** Both grids send a region's objects as
  `ObjectUpdateCompressed`, four or five to a message, reliably, and avatars
  in full. Unless the viewer says its cache is empty (bit 1), both first
  name every object in an `ObjectUpdateCached` and wait to be asked. Second
  Life sends what is within the draw distance of the agent, a little more
  round the camera, and kills what goes out of range by its **linkset root
  alone**: 236 kills took 741 child prims with them unnamed, and all 888
  objects came back with the draw distance. OpenSim sends everything and
  culls nothing. "Send all cacheable objects" (bit 0) gets Second Life's
  whole region and three and a half times as much of each neighbour;
  OpenSim reads bit 1 only. Second Life sends an agent its own
  `AvatarAppearance` at arrival only for bit 2; OpenSim always.
  `InterestList` is Second Life's alone and answers `{mode, previous_mode}`;
  at this spot neither mode sent anything the other did not.
- **Client.** `Event::ObjectStreamBatch` (a message's form and what it
  named); `RegionHandshakeReplyFlags`, settable per session and by default
  "cache empty, understands self appearance" where it was zero; a kill
  takes the prims under what it names and leaves a seated avatar;
  `Command::SetInterestListMode` and `Event::InterestListMode` over the
  adopted `InterestList` capability, re-asked of each region arrived in by
  both runtimes; `sl-repl`'s `set_interest_list_mode`.
- **Fake grid.** `FakeSl` grants `InterestList` and answers it as measured,
  `FakeOpensim` withholds it; `FakeSl` sends the own appearance only to a
  viewer that asked (`BakePolicy::sends_own_appearance`), whichever of the
  handshake reply and the arrival comes first. Both cases run offline on
  both flavours for those rows. The forms, the probes, the range and what
  the mode changes are the large gap:
  [[server-fake-grid-object-update-forms]] (now ready) and
  [[server-world-update-scheduling]].
- **Viewer.** The 360° capture switches to the `360` list while it looks
  around. A new `e2e_objects`, on each fake flavour and on each live grid
  (`SL_E2E_GRID=opensim` and `=aditi`, both run): the viewer's world holds
  its region's objects once the arrival is over and its own avatar is given
  a baked appearance; with the draw distance taken to 32 m and back through
  the viewer's own setting, Second Life's out-of-range objects leave whole
  and return, and nothing changes where the grid does not cull.
- **Automation.** To drive that: `ReadSetting` / `WriteSetting` in the
  automation protocol, `Viewer::setting` / `set_setting`, and
  `sl-viewer-ctl setting <KEY> [VALUE]`
  ([[viewer-automation-set-a-setting]], done here).
- **A race the relogins found.** The fake grid's cleanup after a root's
  logout retired every session of the avatar that was no root agent — and
  so, about half the time, the session of the login that followed, which
  is none until its movement completes; the new viewer's seed request got a
  `404`. It retires the ended login's sessions only now, and both runtimes
  say the HTTP status of a seed answer that does not parse.
- **Not done here.** The planned "vary the flags; one kill per linkset or
  per prim" is answered for range kills; what a deletion or a take names
  is [[gridspec-object-rez-derez]]'s. A DELETE on
  `InterestList` got no answer in 30 s and no command sends one. No spot
  was found where `360` and `default` differ. Area search and the debug
  menu do not exist yet to switch the mode from
  ([[viewer-area-search]], [[viewer-network-debug-tools]]). A cache of our
  own to answer probes from is not planned.

## Known already

The fake grid sends full `ObjectUpdate`s of every object at arrival; never
compressed, terse or cached, ignores camera and draw distance. OpenSim sends
compressed and cached updates. Our `RegionHandshakeReply.flags` is 0 where
Firestorm sends cache / self-appearance bits.

## Discover

Extend `object-update-decode` to count each message type and record the
handshake flags sent; vary the flags; move the camera and draw distance and
count adds / kills (one kill per linkset or per prim?); both grids.

## Document

`book/src/gridspec/objects.md` § Updates.

## Fake grid

Large — [[server-fake-grid-object-update-forms]] and
[[server-world-update-scheduling]].

## Viewer

Cache probes answered, compressed / terse decoded, out-of-range kills re-added.

## Capabilities done in this task

[[protocol-cap-interest-list]]: the `InterestList` mode, measured by what
the update stream carries in `default` and `360` modes.
