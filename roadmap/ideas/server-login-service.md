---
id: server-login-service
title: Standalone login server
topic: server
status: ideas
origin: user request (2026-07) — size what a real server would involve
refs: [protocol-sim-login]
---

Context: [context/server.md](../context/server.md).

A separate login-server process (SL and OpenSim grid mode both have
one): terminates the XML-RPC/LLSD login endpoint, authenticates against
the accounts service (password hash, MFA/TOTP, TOS/critical-message
gates), consults presence (already-online handling), picks the start
region via the grid service (home/last/URI start locations), asks that
simulator to prepare the agent (circuit code, seed capability), and
builds the full login response — inventory skeleton, buddy list,
gestures, helper URIs — from the inventory/friends services.

The wire layer already exists ([[protocol-sim-login]] raises
`sl-wire`'s `LoginServer` to full fidelity); this idea is the *service*:
account lookups, session issuance, simulator handshake, redirects, rate
limiting/abuse protection.
