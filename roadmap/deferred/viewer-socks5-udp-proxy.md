---
id: viewer-socks5-udp-proxy
title: SOCKS5 proxy for the UDP circuit
topic: viewer
status: deferred
origin: split from viewer-preferences-network-cache-tab (2026-08-13)
refs: [viewer-preferences-network-cache-tab]
---

Context: [context/viewer.md](../context/viewer.md).

Proxy the **LLUDP circuit** through a SOCKS5 proxy, as the reference
viewer's Preferences → Setup → Proxy floater offers (`Socks5ProxyEnabled`,
host/port, optional username/password auth — `llsocks5.cpp` /
`llproxy.cpp`): a UDP ASSOCIATE handshake over TCP, then wrapping every
circuit datagram in the SOCKS5 UDP relay header.

Deferred out of [[viewer-preferences-network-cache-tab]], which shipped
the HTTP proxy for the reqwest stack only ("SOCKS for the UDP path is
explicitly out unless trivially available" — it was not: the circuit
sockets are plain tokio/std UDP sockets in sl-client-tokio /
sl-client-bevy with no proxy seam). Picking this up means: a SOCKS5 UDP
ASSOCIATE client (likely a small crate or hand-rolled — the TCP control
connection must stay open for the association's lifetime), a wrap/unwrap
layer at the circuit socket send/recv boundary, preference-tab rows
(enable, host:port, auth) beside the HTTP proxy rows, and failure
surfacing at login when the proxy is unreachable (the reference viewer
fails the login with a dialog).

Reference (Firestorm, read-only): `llproxy.cpp`, `llsocks5.cpp`,
`floater_preferences_proxy.xml`.
