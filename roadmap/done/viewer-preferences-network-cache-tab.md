---
id: viewer-preferences-network-cache-tab
title: Preferences — network & cache tab
topic: viewer
status: done
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

Done (2026-08-13): new tab module `preferences_network_cache.rs` —
bandwidth slider (50–3000 kbps, default 3000) driving
`Throttle::from_total` (new in sl-proto: the reference's
preset-interpolating split; replaces the hardcoded `preset_1000`),
HTTP proxy (one `host:port` for the whole reqwest stack, restart-scoped;
shared proxy-aware builders in sl-client-bevy **and** sl-client-tokio,
`--http-proxy` on sl-repl-tokio / sl-survey), texture + per-asset-cache
size ceilings (256–20000 MB, default 2048 = the previous fixed 2 GiB),
cache / chat-log location overrides, and clear-cache (purge-on-next-start
marker) + clear-inventory-cache (immediate) behind the existing
confirmation templates. The chat-log path control lives here (global
scope — the base is consumed pre-login). SOCKS for the UDP circuit split
off to [[viewer-socks5-udp-proxy]] (deferred).
