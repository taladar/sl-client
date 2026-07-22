---
id: viewer-network-debug-tools
title: Network / world debug tools
topic: viewer
status: ready
origin: Advanced/Develop menu survey (2026-07-22)
refs: [viewer-debug-consoles]
---

Context: [context/viewer.md](../context/viewer.md).

The Develop → Network / World debug utilities, as a checklist over the
session layer (most are thin toggles on machinery we own):

- [ ] Pause agent (`AgentPause` / `AgentResume` — protocol present)
- [ ] LLUDP message log on/off at runtime (write decoded traffic to a file;
      the sl-conformance journal format is the natural target)
- [ ] Velocity-interpolate / ping-interpolate object positions toggles
      (bypass dead-reckoning for diagnosis)
- [ ] Drop-a-packet + sustained packet-loss simulation (exercises the
      reliable-retransmit path — pairs with `test-reliable-retransmit`)
- [ ] Purge disk cache (assets + textures) with confirmation
- [ ] Sim sun override / fixed weather (region-side debug, estate gated)
- [ ] Interest-list 360° mode + reset (the caps toggle the reference sends
      for full-surround fetching — useful for the 360 snapshot task too)
- [ ] Dump simulator features to chat (`SimulatorFeatures` pretty-print)

Reference (Firestorm, read-only): `menu_viewer.xml` (Develop → Network /
World / Cache), `llviewermessage`, `llworld`.

Builds on: the session command surface and `sl-asset` caches.
