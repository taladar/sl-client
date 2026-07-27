---
id: server-inventory-service
title: Inventory service — per-agent trees and the library
topic: server
status: ideas
origin: user request (2026-07) — size what a real server would involve
refs: [protocol-sim-caps-inventory]
---

Context: [context/server.md](../context/server.md).

Persistent per-agent inventory: the folder tree, items (with the full
permission triplet — base/owner/next-owner — and sale info), links, the
system folders, and the shared read-only library.

- Backs AISv3 and the legacy fetch caps
  ([[protocol-sim-caps-inventory]] is the wire layer) plus the UDP
  descendents path; version counters per folder so client caches
  invalidate correctly.
- Mutations arrive from simulators (rez/derez, task inventory
  give/take), the message-routing service (inventory offers between
  agents), and login (skeleton reads).
- The permission-system interactions (next-owner masks applying on
  transfer, no-copy moves) are the part worth designing carefully —
  the client-side permission topic
  ([context/permission.md](permission.md)) documents the semantics
  already.
