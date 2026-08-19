---
id: viewer-udp-connection-port
title: Fixed local UDP port option
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-preferences-network-cache-tab, viewer-socks5-udp-proxy]
---

Context: [context/viewer.md](../context/viewer.md).

Bind the circuit's UDP socket to a user-chosen local source port
instead of an ephemeral one (`ConnectionPortEnabled` /
`ConnectionPort` on Firestorm's network panel), for strict-firewall
setups where only a known source port is allowed outbound. We always
bind an ephemeral port today (the session-socket setup in
`sl-client-bevy/src/lib.rs`).

Scope: one enable + port setting on the network & cache tab (done
[[viewer-preferences-network-cache-tab]]), read at socket-bind time and
restart-scoped like the HTTP proxy rows; the deferred
[[viewer-socks5-udp-proxy]] would sit in the same bind path.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/panel_preferences_setup.xml`,
`indra/newview/llstartup.cpp` (ConnectionPort use).
