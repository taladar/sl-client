---
id: viewer-linden-home
title: Linden Home management menu entry
topic: viewer
status: ready
origin: main-menu survey (2026-07-23)
refs: [viewer-media-prim-browser]
---

Context: [context/viewer.md](../context/viewer.md).

World ▸ My Linden Home… (Second Life only): opens the Linden Homes
management web flow for premium members' Linden Home parcels. A small,
SL-gated web-launch item.

Scope:

- Menu entry gated on the grid being Second Life (hidden on OpenSim),
  opening the Linden Home management page in the embedded browser (the
  CEF web floater).
- Follow the reference's URL/session handling (the page needs the
  logged-in web session the OpenID auth flow provides).

Reference (Firestorm, read-only): `World.LindenHome`
(`menu_viewer.xml` World section, `grid_check="secondlife"`).

Builds on: the embedded browser (in progress,
[[viewer-media-prim-browser]] wired the engine) and web session auth.
