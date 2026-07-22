---
id: viewer-preferences-network-cache-tab
title: Preferences — network & cache tab
topic: viewer
status: blocked
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-preferences-floater]
refs: [viewer-texture-vram-budget]
---

Context: [context/viewer.md](../context/viewer.md).

The **network & cache** tab: maximum bandwidth (drives the throttle presets
— `protocol-15` is done and unconsumed by any UI), disk-cache size limit
and location (the `sl-asset` caches + inventory cache), **clear cache**
with confirmation, and HTTP proxy settings (honoured by our reqwest-based
HTTP stack; SOCKS for the UDP path is explicitly out unless trivially
available). Each bound to the typed settings store.

Reference (Firestorm, read-only): `panel_preferences_setup.xml`,
`floater_preferences_proxy.xml`.

Deps: [[viewer-preferences-floater]].
